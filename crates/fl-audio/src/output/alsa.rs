//! Linux output through ALSA.
//!
//! Two kinds of device:
//! - `default`: the desktop sound server (PipeWire, or PulseAudio) through
//!   its ALSA plugin. A rate choice sets PipeWire's graph clock
//!   (`clock.force-rate`), the Linux counterpart of the Mac changing the
//!   device's nominal rate, so the stream then passes through unresampled.
//! - `hw:CARD=…,DEV=…`: direct hardware, opened at exactly the chosen rate
//!   and bit depth with ALSA's own resampling off (bit-perfect, and exclusive
//!   by nature: the sound server can't use the card meanwhile).

use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use alsa::pcm::{Access, Format, HwParams, PCM};
use alsa::{Direction, ValueOr};

use super::*;

pub const DEFAULT_DEVICE: &str = "default";

pub struct AlsaBackend {
    watching: AtomicBool,
}

impl AlsaBackend {
    pub fn new() -> Arc<AlsaBackend> {
        Arc::new(AlsaBackend { watching: AtomicBool::new(false) })
    }
}

fn is_hw(id: &str) -> bool {
    id.starts_with("hw:")
}

/// Formats to try for a bit depth, best first.
fn formats_for_bits(bits: i64) -> &'static [SampleFormat] {
    match bits {
        16 => &[SampleFormat::I16],
        24 => &[SampleFormat::I24In32Lsb, SampleFormat::I24, SampleFormat::I32],
        _ => &[SampleFormat::I32, SampleFormat::F32],
    }
}

fn alsa_format(f: SampleFormat) -> Format {
    match f {
        SampleFormat::F32 => Format::FloatLE,
        SampleFormat::I16 => Format::S16LE,
        SampleFormat::I24 => Format::S243LE,
        SampleFormat::I24In32 | SampleFormat::I32 => Format::S32LE,
        SampleFormat::I24In32Lsb => Format::S24LE,
    }
}

fn open_pcm(id: &str) -> Result<PCM, String> {
    PCM::new(id, Direction::Playback, false).map_err(|e| format!("{id}: {e}"))
}

// MARK: - PipeWire graph clock

fn pw_metadata(args: &[&str]) -> Option<String> {
    let out = Command::new("pw-metadata").args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// `value:'48000'` from a `pw-metadata -n settings 0 <key>` line.
fn metadata_number(text: &str, key: &str) -> Option<f64> {
    let line = text.lines().find(|l| l.contains(&format!("key:'{key}'")))?;
    let v = line.split("value:'").nth(1)?.split('\'').next()?;
    v.parse().ok()
}

/// The sound server's current graph rate (forced rate if set, else the clock).
fn pipewire_rate() -> Option<f64> {
    let text = pw_metadata(&["-n", "settings", "0"])?;
    metadata_number(&text, "clock.force-rate").filter(|r| *r > 0.0).or_else(|| metadata_number(&text, "clock.rate"))
}

fn pipewire_allowed_rates() -> Option<Vec<f64>> {
    let text = pw_metadata(&["-n", "settings", "0", "clock.allowed-rates"])?;
    let line = text.lines().find(|l| l.contains("clock.allowed-rates"))?;
    let list = line.split("value:'").nth(1)?.split('\'').next()?;
    let v: Vec<f64> = list.trim_matches(|c| c == '[' || c == ']').split([',', ' ']).filter_map(|s| s.trim().parse().ok()).collect();
    (!v.is_empty()).then_some(v)
}

// MARK: - Backend

impl Backend for AlsaBackend {
    fn name(&self) -> &'static str {
        "ALSA"
    }

    fn devices(&self) -> Vec<DeviceInfo> {
        let server = if pw_metadata(&["-n", "settings", "0"]).is_some() { "PipeWire" } else { "Sound server" };
        let mut v = vec![DeviceInfo { id: DEFAULT_DEVICE.into(), name: format!("System Default ({server})") }];
        let Ok(hints) = alsa::device_name::HintIter::new_str(None, "pcm") else { return v };
        for h in hints {
            let Some(name) = h.name else { continue };
            if !is_hw(&name) || h.direction == Some(Direction::Capture) {
                continue;
            }
            // "HDA Intel PCH, ALC892 Analog\nDirect hardware device…"
            let desc = h.desc.as_deref().and_then(|d| d.lines().next()).unwrap_or(&name).to_string();
            v.push(DeviceInfo { id: name, name: format!("{desc} (direct)") });
        }
        v
    }

    fn default_device(&self) -> Option<String> {
        Some(DEFAULT_DEVICE.into())
    }

    fn available_sample_rates(&self, id: &str) -> Vec<f64> {
        if !is_hw(id) {
            // Through the sound server any rate plays; list the ones the
            // graph can switch to without resampling when it says.
            return pipewire_allowed_rates().unwrap_or_else(|| STANDARD_RATES[..6].to_vec());
        }
        let Ok(pcm) = open_pcm(id) else { return Vec::new() };
        let Ok(hw) = HwParams::any(&pcm) else { return Vec::new() };
        let _ = hw.set_rate_resample(false);
        let mut rates: Vec<f64> = STANDARD_RATES.iter().copied().filter(|r| hw.test_rate(*r as u32).is_ok()).collect();
        if rates.is_empty() {
            if let (Ok(lo), Ok(hi)) = (hw.get_rate_min(), hw.get_rate_max()) {
                rates = expand_rates(&[(f64::from(lo), f64::from(hi))]);
            }
        }
        rates
    }

    fn available_bit_depths(&self, id: &str, rate: f64) -> Vec<i64> {
        if !is_hw(id) {
            // The sound server mixes in float; depth is the server's business.
            return Vec::new();
        }
        let Ok(pcm) = open_pcm(id) else { return Vec::new() };
        let Ok(hw) = HwParams::any(&pcm) else { return Vec::new() };
        let _ = hw.set_rate_resample(false);
        if rate > 0.0 && hw.set_rate(rate as u32, ValueOr::Nearest).is_err() {
            return Vec::new();
        }
        [16i64, 24, 32]
            .into_iter()
            .filter(|b| formats_for_bits(*b).iter().any(|f| hw.test_format(alsa_format(*f)).is_ok()))
            .collect()
    }

    fn current_format(&self, id: &str) -> Option<(f64, Option<i64>)> {
        if is_hw(id) {
            // A card has no format of its own until opened.
            return None;
        }
        pipewire_rate().map(|r| (r, None))
    }

    fn apply_device_format(&self, id: &str, rate: Option<f64>, _bits: Option<i64>) -> Result<(), String> {
        if is_hw(id) {
            return Ok(()); // applied when the stream opens
        }
        let Some(rate) = rate else { return Ok(()) };
        if pw_metadata(&["-n", "settings", "0", "clock.force-rate", &format!("{}", rate as u32)]).is_none() {
            return Err("Couldn't set the PipeWire clock rate (pw-metadata unavailable)".into());
        }
        // The graph switches asynchronously; wait briefly like the Mac does.
        for _ in 0..50 {
            if pipewire_rate() == Some(rate) {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        Ok(())
    }

    fn open(&self, spec: &StreamSpec, render: Render) -> Result<Box<dyn OutputStream>, String> {
        let id = spec.device_id.clone().unwrap_or_else(|| DEFAULT_DEVICE.into());
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<StreamFormat, String>>();
        let stop = Arc::new(AtomicBool::new(false));
        let error = Arc::new(Mutex::new(None));
        let (s2, e2, spec2, id2) = (stop.clone(), error.clone(), spec.clone(), id.clone());
        let thread = std::thread::Builder::new()
            .name("fl-alsa".into())
            .spawn(move || render_thread(&id2, &spec2, render, &s2, &e2, ready_tx))
            .map_err(|e| e.to_string())?;
        let fmt = ready_rx.recv().map_err(|_| "render thread exited".to_string())??;
        Ok(Box::new(AlsaStream { fmt, id, stop, error, thread: Some(thread) }))
    }

    /// ALSA has no hot-plug callback; watch the card list.
    fn watch(&self, on_event: Arc<dyn Fn(DeviceEvent) + Send + Sync>) {
        if self.watching.swap(true, Ordering::AcqRel) {
            return;
        }
        std::thread::Builder::new()
            .name("fl-alsa-watch".into())
            .spawn(move || {
                let read = || std::fs::read_to_string("/proc/asound/cards").unwrap_or_default();
                let mut last = read();
                let mut last_rate = pipewire_rate();
                loop {
                    std::thread::sleep(Duration::from_secs(2));
                    let now = read();
                    if now != last {
                        last = now;
                        on_event(DeviceEvent::DevicesChanged);
                    }
                    let rate = pipewire_rate();
                    if rate != last_rate {
                        last_rate = rate;
                        on_event(DeviceEvent::FormatChanged(DEFAULT_DEVICE.into()));
                    }
                }
            })
            .ok();
    }
}

// MARK: - Stream

struct AlsaStream {
    fmt: StreamFormat,
    id: String,
    stop: Arc<AtomicBool>,
    error: Arc<Mutex<Option<String>>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl OutputStream for AlsaStream {
    fn format(&self) -> StreamFormat {
        self.fmt
    }
    fn device_id(&self) -> &str {
        &self.id
    }
    fn error(&self) -> Option<String> {
        self.error.lock().unwrap().clone()
    }
}

impl Drop for AlsaStream {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

struct Configured {
    pcm: PCM,
    fmt: StreamFormat,
    period: usize,
}

fn configure(id: &str, spec: &StreamSpec) -> Result<Configured, String> {
    let pcm = open_pcm(id)?;
    let hw_dev = is_hw(id);
    let fmt = {
        let hw = HwParams::any(&pcm).map_err(|e| e.to_string())?;
        hw.set_access(Access::RWInterleaved).map_err(|e| e.to_string())?;
        if hw_dev {
            // Never let ALSA resample behind our back.
            hw.set_rate_resample(false).map_err(|e| e.to_string())?;
        }
        let channels = if hw.test_channels(2).is_ok() { 2 } else { hw.get_channels_min().map_err(|e| e.to_string())?.max(1) };
        hw.set_channels(channels).map_err(|e| e.to_string())?;

        let rate = match spec.sample_rate {
            Some(r) => r as u32,
            None if !hw_dev => pipewire_rate().map_or(48_000, |r| r as u32),
            None => 48_000,
        };
        if hw_dev && spec.sample_rate.is_some() {
            hw.set_rate(rate, ValueOr::Nearest).map_err(|e| format!("{rate} Hz isn't supported by {id}: {e}"))?;
        } else {
            hw.set_rate_near(rate, ValueOr::Nearest).map_err(|e| e.to_string())?;
        }

        let preferred: Vec<SampleFormat> = match (hw_dev, spec.bit_depth) {
            (_, Some(b)) => formats_for_bits(b).iter().chain(formats_for_bits(32)).chain(formats_for_bits(24)).chain(formats_for_bits(16)).copied().collect(),
            // The server mixes in float: hand it float.
            (false, None) => vec![SampleFormat::F32, SampleFormat::I32, SampleFormat::I24In32Lsb, SampleFormat::I16],
            (true, None) => vec![SampleFormat::I32, SampleFormat::I24In32Lsb, SampleFormat::I24, SampleFormat::F32, SampleFormat::I16],
        };
        let sample = preferred
            .into_iter()
            .find(|f| hw.test_format(alsa_format(*f)).is_ok())
            .ok_or_else(|| format!("{id}: no usable sample format"))?;
        hw.set_format(alsa_format(sample)).map_err(|e| e.to_string())?;
        // ~200 ms of buffer in 4 periods: safe against scheduling hiccups,
        // irrelevant to gapless (the engine never starves between tracks).
        let _ = hw.set_buffer_time_near(200_000, ValueOr::Nearest);
        let _ = hw.set_period_time_near(50_000, ValueOr::Nearest);
        pcm.hw_params(&hw).map_err(|e| e.to_string())?;
        let cur = pcm.hw_params_current().map_err(|e| e.to_string())?;
        StreamFormat {
            sample_rate: cur.get_rate().map_err(|e| e.to_string())?,
            channels: cur.get_channels().map_err(|e| e.to_string())? as u16,
            sample,
        }
    };
    let period = pcm.hw_params_current().and_then(|h| h.get_period_size()).map_err(|e| e.to_string())? as usize;
    // Start once a period is queued.
    {
        let sw = pcm.sw_params_current().map_err(|e| e.to_string())?;
        let _ = sw.set_start_threshold(period as alsa::pcm::Frames);
        pcm.sw_params(&sw).map_err(|e| e.to_string())?;
    }
    Ok(Configured { pcm, fmt, period: period.max(64) })
}

fn render_thread(
    id: &str,
    spec: &StreamSpec,
    mut render: Render,
    stop: &AtomicBool,
    error: &Mutex<Option<String>>,
    ready: std::sync::mpsc::Sender<Result<StreamFormat, String>>,
) {
    let c = match configure(id, spec) {
        Ok(c) => c,
        Err(e) => {
            let _ = ready.send(Err(e));
            return;
        }
    };
    let _ = ready.send(Ok(c.fmt));
    promote_thread();

    let ch = usize::from(c.fmt.channels);
    let bps = bytes_per_sample(c.fmt.sample);
    let mut scratch = vec![0f32; c.period * ch];
    let mut bytes = vec![0u8; c.period * ch * bps];
    let mut q = Quantizer::default();
    let io = c.pcm.io_bytes();

    while !stop.load(Ordering::Acquire) {
        render(&mut scratch, ch);
        q.write(&scratch, c.fmt.sample, &mut bytes);
        let mut off = 0;
        while off < bytes.len() && !stop.load(Ordering::Acquire) {
            match io.writei(&bytes[off..]) {
                Ok(frames) => off += frames * ch * bps,
                Err(e) => {
                    // Underrun (EPIPE) / suspend: recover and carry on.
                    if c.pcm.try_recover(e, true).is_err() {
                        *error.lock().unwrap() = Some(format!("{id}: {e}"));
                        return;
                    }
                }
            }
        }
    }
    let _ = c.pcm.drop();
}

/// Asks for real-time scheduling when allowed (rtkit / limits.conf); the
/// 200 ms buffer copes without it.
fn promote_thread() {
    // SCHED_FIFO priority 10, best effort.
    #[repr(C)]
    struct SchedParam {
        priority: i32,
    }
    extern "C" {
        fn pthread_self() -> usize;
        fn pthread_setschedparam(thread: usize, policy: i32, param: *const SchedParam) -> i32;
    }
    const SCHED_FIFO: i32 = 1;
    unsafe {
        let _ = pthread_setschedparam(pthread_self(), SCHED_FIFO, &SchedParam { priority: 10 });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pw_metadata_output() {
        let text = "Found \"settings\" metadata 32\nupdate: id:0 key:'log.level' value:'2' type:''\nupdate: id:0 key:'clock.rate' value:'48000' type:''\nupdate: id:0 key:'clock.force-rate' value:'0' type:''\n";
        assert_eq!(metadata_number(text, "clock.rate"), Some(48_000.0));
        assert_eq!(metadata_number(text, "clock.force-rate"), Some(0.0));
        assert_eq!(metadata_number(text, "clock.quantum"), None);
    }

    /// Lists devices and probes capabilities; opens nothing for playback.
    #[test]
    fn enumerate_devices_read_only() {
        let b = AlsaBackend::new();
        for d in b.devices() {
            let rates = b.available_sample_rates(&d.id);
            let bits = b.available_bit_depths(&d.id, 0.0);
            eprintln!("{} [{}]: rates {rates:?} bits {bits:?} current {:?}", d.name, d.id, b.current_format(&d.id));
        }
    }

    /// Renders silence through the default device for 300 ms.
    #[test]
    fn default_stream_renders_silence() {
        let b = AlsaBackend::new();
        let n = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let n2 = n.clone();
        let stream = match b.open(&StreamSpec::default(), Box::new(move |buf, _| {
            buf.fill(0.0);
            n2.fetch_add(1, Ordering::Relaxed);
            0
        })) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("no default ALSA device here ({e}); skipping");
                return;
            }
        };
        eprintln!("format {:?}", stream.format());
        std::thread::sleep(Duration::from_millis(300));
        assert!(stream.error().is_none(), "{:?}", stream.error());
        drop(stream);
        assert!(n.load(Ordering::Relaxed) > 0);
    }
}
