//! WASAPI output (Windows).
//!
//! - **Shared** (default): the stream runs at the endpoint's mix format. When
//!   the user picks a rate or bit depth, `apply_device_format` changes the
//!   endpoint's *device format* through `IPolicyConfig` (what the Sound control
//!   panel's "Default Format" does) — the macOS "set nominal rate / physical
//!   format" equivalent — and the engine resamples to that one fixed rate.
//! - **Exclusive** (opt-in): the stream opens directly at the chosen format;
//!   the engine's quantizer converts to the device's integer format.
//!
//! Event-driven render thread with MMCSS "Pro Audio" priority.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use windows::core::{implement, interface, IUnknown, IUnknown_Vtbl, GUID, HRESULT, PCWSTR, PWSTR};
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Foundation::{CloseHandle, HANDLE, PROPERTYKEY, S_OK, WAIT_OBJECT_0};
use windows::Win32::Media::Audio::*;
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_MULTITHREADED, STGM_READ,
};
use windows::Win32::System::Threading::{
    AvRevertMmThreadCharacteristics, AvSetMmThreadCharacteristicsW, CreateEventW, WaitForSingleObject,
};
use windows::Win32::System::Com::StructuredStorage::PropVariantToStringAlloc;

use super::*;

const WAVE_FORMAT_PCM: u16 = 1;
const WAVE_FORMAT_IEEE_FLOAT: u16 = 3;
const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;
const SUBTYPE_PCM: GUID = GUID::from_u128(0x00000001_0000_0010_8000_00aa00389b71);
const SUBTYPE_FLOAT: GUID = GUID::from_u128(0x00000003_0000_0010_8000_00aa00389b71);
const SPEAKER_FRONT_LEFT: u32 = 0x1;
const SPEAKER_FRONT_RIGHT: u32 = 0x2;
/// `PKEY_AudioEngine_DeviceFormat` {f19f064d-082c-4e27-bc73-6882a1bb8e4c}, 0.
const PKEY_DEVICE_FORMAT: PROPERTYKEY =
    PROPERTYKEY { fmtid: GUID::from_u128(0xf19f064d_082c_4e27_bc73_6882a1bb8e4c), pid: 0 };

// MARK: - IPolicyConfig (undocumented, stable since Windows 7)

#[allow(non_snake_case)]
mod policy {
    use super::*;

    #[interface("f8679f50-850a-41cf-9c72-430f290290c8")]
    pub unsafe trait IPolicyConfig: IUnknown {
    fn GetMixFormat(&self, device: PCWSTR, format: *mut *mut WAVEFORMATEX) -> HRESULT;
    fn GetDeviceFormat(&self, device: PCWSTR, default: i32, format: *mut *mut WAVEFORMATEX) -> HRESULT;
    fn ResetDeviceFormat(&self, device: PCWSTR) -> HRESULT;
    fn SetDeviceFormat(&self, device: PCWSTR, endpoint: *const WAVEFORMATEX, mix: *const WAVEFORMATEX) -> HRESULT;
    fn GetProcessingPeriod(&self, device: PCWSTR, default: i32, def: *mut i64, min: *mut i64) -> HRESULT;
    fn SetProcessingPeriod(&self, device: PCWSTR, period: *const i64) -> HRESULT;
    fn GetShareMode(&self, device: PCWSTR, mode: *mut c_void) -> HRESULT;
    fn SetShareMode(&self, device: PCWSTR, mode: *const c_void) -> HRESULT;
    fn GetPropertyValue(&self, device: PCWSTR, store: i32, key: *const PROPERTYKEY, value: *mut PROPVARIANT) -> HRESULT;
    fn SetPropertyValue(&self, device: PCWSTR, store: i32, key: *const PROPERTYKEY, value: *const PROPVARIANT) -> HRESULT;
    fn SetDefaultEndpoint(&self, device: PCWSTR, role: ERole) -> HRESULT;
    fn SetEndpointVisibility(&self, device: PCWSTR, visible: i32) -> HRESULT;
    }

    /// `IPolicyConfig::SetDeviceFormat(device, fmt, fmt)`.
    pub fn set_device_format(device: PCWSTR, fmt: *const WAVEFORMATEX) -> Result<(), String> {
        unsafe {
            let policy: IPolicyConfig = CoCreateInstance(&CLSID_POLICY_CONFIG_CLIENT, None, CLSCTX_ALL).map_err(err)?;
            let hr = policy.SetDeviceFormat(device, fmt, fmt);
            if hr.is_err() {
                return Err(format!("SetDeviceFormat failed ({:#010x})", hr.0 as u32));
            }
        }
        Ok(())
    }
}

const CLSID_POLICY_CONFIG_CLIENT: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);

// MARK: - Helpers

fn com() {
    // Idempotent per thread; RPC_E_CHANGED_MODE just means STA is already set.
    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
}

fn err(e: windows::core::Error) -> String {
    format!("{e} ({:#010x})", e.code().0 as u32)
}

fn enumerator() -> Result<IMMDeviceEnumerator, String> {
    com();
    unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }.map_err(err)
}

unsafe fn take_pwstr(p: PWSTR) -> String {
    let s = p.to_string().unwrap_or_default();
    CoTaskMemFree(Some(p.0 as *const c_void));
    s
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn device(id: &str) -> Result<IMMDevice, String> {
    let w = wide(id);
    unsafe { enumerator()?.GetDevice(PCWSTR(w.as_ptr())) }.map_err(err)
}

fn device_id(d: &IMMDevice) -> String {
    unsafe { d.GetId().map(|p| take_pwstr(p)).unwrap_or_default() }
}

fn friendly_name(d: &IMMDevice) -> Option<String> {
    unsafe {
        let store = d.OpenPropertyStore(STGM_READ).ok()?;
        let v = store.GetValue(&PKEY_Device_FriendlyName).ok()?;
        let s = PropVariantToStringAlloc(&v).ok()?;
        Some(take_pwstr(s))
    }
}

/// A WAVEFORMATEXTENSIBLE for `rate`/`channels` in `fmt`.
fn extensible(rate: u32, channels: u16, fmt: SampleFormat) -> WAVEFORMATEXTENSIBLE {
    let (container, valid, sub) = match fmt {
        SampleFormat::F32 => (32, 32, SUBTYPE_FLOAT),
        SampleFormat::I16 => (16, 16, SUBTYPE_PCM),
        SampleFormat::I24 => (24, 24, SUBTYPE_PCM),
        SampleFormat::I24In32 | SampleFormat::I24In32Lsb => (32, 24, SUBTYPE_PCM),
        SampleFormat::I32 => (32, 32, SUBTYPE_PCM),
    };
    let block = channels * container / 8;
    let mask = if channels == 1 { 0x4 } else if channels == 2 { SPEAKER_FRONT_LEFT | SPEAKER_FRONT_RIGHT } else { (1u32 << channels) - 1 };
    WAVEFORMATEXTENSIBLE {
        Format: WAVEFORMATEX {
            wFormatTag: WAVE_FORMAT_EXTENSIBLE,
            nChannels: channels,
            nSamplesPerSec: rate,
            nAvgBytesPerSec: rate * u32::from(block),
            nBlockAlign: block,
            wBitsPerSample: container,
            cbSize: (std::mem::size_of::<WAVEFORMATEXTENSIBLE>() - std::mem::size_of::<WAVEFORMATEX>()) as u16,
        },
        Samples: WAVEFORMATEXTENSIBLE_0 { wValidBitsPerSample: valid },
        dwChannelMask: mask,
        SubFormat: sub,
    }
}

/// Reads a (possibly extensible) WAVEFORMATEX into rate/channels/sample format.
unsafe fn describe(p: *const WAVEFORMATEX) -> Option<StreamFormat> {
    let f = p.as_ref()?;
    let (tag, valid) = if f.wFormatTag == WAVE_FORMAT_EXTENSIBLE && f.cbSize >= 22 {
        let x = &*(p as *const WAVEFORMATEXTENSIBLE);
        let sub = x.SubFormat;
        let tag = if sub == SUBTYPE_FLOAT { WAVE_FORMAT_IEEE_FLOAT } else if sub == SUBTYPE_PCM { WAVE_FORMAT_PCM } else { 0 };
        (tag, x.Samples.wValidBitsPerSample)
    } else {
        (f.wFormatTag, f.wBitsPerSample)
    };
    let sample = match (tag, f.wBitsPerSample, valid) {
        (WAVE_FORMAT_IEEE_FLOAT, 32, _) => SampleFormat::F32,
        (WAVE_FORMAT_PCM, 16, _) => SampleFormat::I16,
        (WAVE_FORMAT_PCM, 24, _) => SampleFormat::I24,
        (WAVE_FORMAT_PCM, 32, 24) => SampleFormat::I24In32,
        (WAVE_FORMAT_PCM, 32, _) => SampleFormat::I32,
        _ => return None,
    };
    Some(StreamFormat { sample_rate: f.nSamplesPerSec, channels: f.nChannels, sample })
}

fn client(d: &IMMDevice) -> Result<IAudioClient, String> {
    unsafe { d.Activate::<IAudioClient>(CLSCTX_ALL, None) }.map_err(err)
}

fn mix_format(c: &IAudioClient) -> Result<StreamFormat, String> {
    unsafe {
        let p = c.GetMixFormat().map_err(err)?;
        let f = describe(p);
        CoTaskMemFree(Some(p as *const c_void));
        f.ok_or_else(|| "unsupported mix format".into())
    }
}

fn exclusive_supported(c: &IAudioClient, rate: u32, channels: u16, fmt: SampleFormat) -> bool {
    let wf = extensible(rate, channels, fmt);
    unsafe { c.IsFormatSupported(AUDCLNT_SHAREMODE_EXCLUSIVE, &wf.Format, None) == S_OK }
}

/// Integer formats for a bit depth, preferred container first.
fn formats_for_bits(bits: i64) -> &'static [SampleFormat] {
    match bits {
        16 => &[SampleFormat::I16],
        24 => &[SampleFormat::I24In32, SampleFormat::I24],
        32 => &[SampleFormat::I32, SampleFormat::F32],
        _ => &[],
    }
}

/// The endpoint's device format (what "Default Format" shows).
fn device_format(d: &IMMDevice) -> Option<StreamFormat> {
    unsafe {
        let store = d.OpenPropertyStore(STGM_READ).ok()?;
        let v = store.GetValue(&PKEY_DEVICE_FORMAT).ok()?;
        // VT_BLOB holding a WAVEFORMATEX(-TENSIBLE).
        let raw = &v as *const PROPVARIANT as *const u8;
        let vt = *(raw as *const u16);
        if vt != 65 {
            return None;
        }
        let blob = &*(raw.add(8) as *const windows::Win32::System::Com::BLOB);
        if (blob.cbSize as usize) < std::mem::size_of::<WAVEFORMATEX>() {
            return None;
        }
        describe(blob.pBlobData as *const WAVEFORMATEX)
    }
}

// MARK: - Backend

/// Keeps the endpoint-notification registration alive. Created in the MTA,
/// whose COM objects are free-threaded, so moving it between threads is sound.
struct Registration(IMMDeviceEnumerator, IMMNotificationClient);
unsafe impl Send for Registration {}
unsafe impl Sync for Registration {}

pub struct WasapiBackend {
    notifier: Mutex<Option<Registration>>,
}

impl WasapiBackend {
    pub fn new() -> Arc<WasapiBackend> {
        Arc::new(WasapiBackend { notifier: Mutex::new(None) })
    }
}

impl Backend for WasapiBackend {
    fn name(&self) -> &'static str {
        "WASAPI"
    }

    fn devices(&self) -> Vec<DeviceInfo> {
        let Ok(e) = enumerator() else { return vec![] };
        unsafe {
            let Ok(col) = e.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) else { return vec![] };
            let n = col.GetCount().unwrap_or(0);
            (0..n)
                .filter_map(|i| col.Item(i).ok())
                .map(|d| {
                    let id = device_id(&d);
                    DeviceInfo { name: friendly_name(&d).unwrap_or_else(|| id.clone()), id }
                })
                .collect()
        }
    }

    fn default_device(&self) -> Option<String> {
        let e = enumerator().ok()?;
        unsafe { e.GetDefaultAudioEndpoint(eRender, eConsole) }.ok().map(|d| device_id(&d))
    }

    fn available_sample_rates(&self, id: &str) -> Vec<f64> {
        let Ok(d) = device(id) else { return vec![] };
        let Ok(c) = client(&d) else { return vec![] };
        let ch = mix_format(&c).map(|f| f.channels).unwrap_or(2);
        STANDARD_RATES
            .iter()
            .copied()
            .filter(|r| [16, 24, 32].iter().flat_map(|b| formats_for_bits(*b)).any(|f| exclusive_supported(&c, *r as u32, ch, *f)))
            .collect()
    }

    fn available_bit_depths(&self, id: &str, rate: f64) -> Vec<i64> {
        let Ok(d) = device(id) else { return vec![] };
        let Ok(c) = client(&d) else { return vec![] };
        let ch = mix_format(&c).map(|f| f.channels).unwrap_or(2);
        let rate = if rate > 0.0 { rate } else { device_format(&d).map(|f| f64::from(f.sample_rate)).unwrap_or(48_000.0) };
        [16i64, 24, 32]
            .into_iter()
            .filter(|b| formats_for_bits(*b).iter().any(|f| exclusive_supported(&c, rate as u32, ch, *f)))
            .collect()
    }

    fn current_format(&self, id: &str) -> Option<(f64, Option<i64>)> {
        let d = device(id).ok()?;
        let f = device_format(&d).or_else(|| client(&d).ok().and_then(|c| mix_format(&c).ok()))?;
        Some((f64::from(f.sample_rate), Some(i64::from(f.sample.bits()))))
    }

    /// `IPolicyConfig::SetDeviceFormat` — changes a system audio setting.
    fn apply_device_format(&self, id: &str, rate: Option<f64>, bits: Option<i64>) -> Result<(), String> {
        let d = device(id)?;
        let c = client(&d)?;
        let current = device_format(&d).or_else(|| mix_format(&c).ok()).ok_or("no device format")?;
        let rate = rate.map(|r| r as u32).unwrap_or(current.sample_rate);
        let bits = bits.unwrap_or(i64::from(current.sample.bits()));
        if rate == current.sample_rate && i64::from(current.sample.bits()) == bits {
            return Ok(());
        }
        let fmt = formats_for_bits(bits)
            .iter()
            .copied()
            .find(|f| exclusive_supported(&c, rate, current.channels, *f))
            .ok_or_else(|| format!("{bits}-bit / {rate} Hz isn't supported by this device"))?;
        let wf = extensible(rate, current.channels, fmt);
        let w = wide(id);
        policy::set_device_format(PCWSTR(w.as_ptr()), &wf.Format)
    }

    fn open(&self, spec: &StreamSpec, render: Render) -> Result<Box<dyn OutputStream>, String> {
        let id = match &spec.device_id {
            Some(id) => id.clone(),
            None => self.default_device().ok_or("no output device")?,
        };
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<StreamFormat, String>>();
        let stop = Arc::new(AtomicBool::new(false));
        let error = Arc::new(Mutex::new(None));
        let (s2, e2, spec2, id2) = (stop.clone(), error.clone(), spec.clone(), id.clone());
        let thread = std::thread::Builder::new()
            .name("fl-wasapi".into())
            .spawn(move || render_thread(&id2, &spec2, render, &s2, &e2, ready_tx))
            .map_err(|e| e.to_string())?;
        let fmt = ready_rx.recv().map_err(|_| "render thread exited".to_string())??;
        Ok(Box::new(WasapiStream { fmt, id, stop, error, thread: Some(thread) }))
    }

    fn watch(&self, on_event: Arc<dyn Fn(DeviceEvent) + Send + Sync>) {
        let Ok(e) = enumerator() else { return };
        let client: IMMNotificationClient = Notifier { on_event }.into();
        if unsafe { e.RegisterEndpointNotificationCallback(&client) }.is_ok() {
            *self.notifier.lock().unwrap() = Some(Registration(e, client));
        }
    }
}

impl Drop for WasapiBackend {
    fn drop(&mut self) {
        if let Some(Registration(e, c)) = self.notifier.lock().unwrap().take() {
            let _ = unsafe { e.UnregisterEndpointNotificationCallback(&c) };
        }
    }
}

// MARK: - Stream

struct WasapiStream {
    fmt: StreamFormat,
    id: String,
    stop: Arc<AtomicBool>,
    error: Arc<Mutex<Option<String>>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl OutputStream for WasapiStream {
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

impl Drop for WasapiStream {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

struct Initialized {
    client: IAudioClient,
    fmt: StreamFormat,
    exclusive: bool,
}

fn initialize(d: &IMMDevice, spec: &StreamSpec, event: HANDLE) -> Result<Initialized, String> {
    let c = client(d)?;
    if !spec.exclusive {
        let fmt = mix_format(&c)?;
        unsafe {
            let p = c.GetMixFormat().map_err(err)?;
            let r = c.Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_EVENTCALLBACK | AUDCLNT_STREAMFLAGS_NOPERSIST,
                0,
                0,
                p,
                None,
            );
            CoTaskMemFree(Some(p as *const c_void));
            r.map_err(err)?;
            c.SetEventHandle(event).map_err(err)?;
        }
        return Ok(Initialized { client: c, fmt, exclusive: false });
    }

    // Exclusive: the requested rate/bits, else the device format.
    let base = device_format(d).or_else(|| mix_format(&c).ok()).ok_or("no device format")?;
    let rate = spec.sample_rate.map(|r| r as u32).unwrap_or(base.sample_rate);
    let bits = spec.bit_depth.unwrap_or(i64::from(base.sample.bits()));
    let candidates: Vec<SampleFormat> =
        formats_for_bits(bits).iter().chain(formats_for_bits(24)).chain(formats_for_bits(16)).copied().collect();
    let sample = candidates
        .into_iter()
        .find(|f| exclusive_supported(&c, rate, base.channels, *f))
        .ok_or_else(|| format!("{rate} Hz isn't supported in exclusive mode"))?;
    let wf = extensible(rate, base.channels, sample);
    let mut period = 0i64;
    unsafe {
        c.GetDevicePeriod(Some(&mut period), None).map_err(err)?;
        let flags = AUDCLNT_STREAMFLAGS_EVENTCALLBACK | AUDCLNT_STREAMFLAGS_NOPERSIST;
        let mut c = c;
        if let Err(e) = c.Initialize(AUDCLNT_SHAREMODE_EXCLUSIVE, flags, period, period, &wf.Format, None) {
            if e.code() != AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED {
                return Err(err(e));
            }
            // Align the period to the buffer size the driver wants.
            let frames = c.GetBufferSize().map_err(err)?;
            period = (10_000_000.0 * f64::from(frames) / f64::from(rate) + 0.5) as i64;
            c = client(d)?;
            c.Initialize(AUDCLNT_SHAREMODE_EXCLUSIVE, flags, period, period, &wf.Format, None).map_err(err)?;
        }
        c.SetEventHandle(event).map_err(err)?;
        Ok(Initialized {
            client: c,
            fmt: StreamFormat { sample_rate: rate, channels: base.channels, sample },
            exclusive: true,
        })
    }
}

fn render_thread(
    id: &str,
    spec: &StreamSpec,
    mut render: Render,
    stop: &AtomicBool,
    error: &Mutex<Option<String>>,
    ready: std::sync::mpsc::Sender<Result<StreamFormat, String>>,
) {
    com();
    let mut task_index = 0u32;
    let mmcss = unsafe { AvSetMmThreadCharacteristicsW(windows::core::w!("Pro Audio"), &mut task_index) }.ok();

    let setup = (|| -> Result<(Initialized, IAudioRenderClient, HANDLE, u32), String> {
        let d = device(id)?;
        let event = unsafe { CreateEventW(None, false, false, None) }.map_err(err)?;
        let init = initialize(&d, spec, event)?;
        let frames = unsafe { init.client.GetBufferSize() }.map_err(err)?;
        let rc: IAudioRenderClient = unsafe { init.client.GetService() }.map_err(err)?;
        Ok((init, rc, event, frames))
    })();
    let (init, rc, event, buffer_frames) = match setup {
        Ok(v) => v,
        Err(e) => {
            let _ = ready.send(Err(e));
            return;
        }
    };
    let _ = ready.send(Ok(init.fmt));

    let ch = usize::from(init.fmt.channels);
    let bps = bytes_per_sample(init.fmt.sample);
    let mut scratch = vec![0f32; buffer_frames as usize * ch];
    let mut q = Quantizer::default();

    let fill = |frames: u32, scratch: &mut Vec<f32>, q: &mut Quantizer, render: &mut Render| -> Result<(), windows::core::Error> {
        if frames == 0 {
            return Ok(());
        }
        unsafe {
            let data = rc.GetBuffer(frames)?;
            let n = frames as usize * ch;
            render(&mut scratch[..n], ch);
            let dst = std::slice::from_raw_parts_mut(data, n * bps);
            q.write(&scratch[..n], init.fmt.sample, dst);
            rc.ReleaseBuffer(frames, 0)
        }
    };

    let result = (|| -> Result<(), windows::core::Error> {
        // Prime one buffer so the stream starts without a glitch.
        fill(buffer_frames, &mut scratch, &mut q, &mut render)?;
        unsafe { init.client.Start()? };
        while !stop.load(Ordering::Acquire) {
            if unsafe { WaitForSingleObject(event, 200) } != WAIT_OBJECT_0 {
                continue;
            }
            let avail = if init.exclusive {
                buffer_frames
            } else {
                buffer_frames - unsafe { init.client.GetCurrentPadding()? }
            };
            fill(avail, &mut scratch, &mut q, &mut render)?;
        }
        Ok(())
    })();
    unsafe {
        let _ = init.client.Stop();
        let _ = CloseHandle(event);
        if let Some(h) = mmcss {
            let _ = AvRevertMmThreadCharacteristics(h);
        }
    }
    if let Err(e) = result {
        *error.lock().unwrap() = Some(err(e));
    }
}

// MARK: - Notifications

#[implement(IMMNotificationClient)]
struct Notifier {
    on_event: Arc<dyn Fn(DeviceEvent) + Send + Sync>,
}

impl IMMNotificationClient_Impl for Notifier_Impl {
    fn OnDeviceStateChanged(&self, _id: &PCWSTR, _state: DEVICE_STATE) -> windows::core::Result<()> {
        (self.on_event)(DeviceEvent::DevicesChanged);
        Ok(())
    }
    fn OnDeviceAdded(&self, _id: &PCWSTR) -> windows::core::Result<()> {
        (self.on_event)(DeviceEvent::DevicesChanged);
        Ok(())
    }
    fn OnDeviceRemoved(&self, _id: &PCWSTR) -> windows::core::Result<()> {
        (self.on_event)(DeviceEvent::DevicesChanged);
        Ok(())
    }
    fn OnDefaultDeviceChanged(&self, flow: EDataFlow, role: ERole, _id: &PCWSTR) -> windows::core::Result<()> {
        if flow == eRender && role == eConsole {
            (self.on_event)(DeviceEvent::DevicesChanged);
        }
        Ok(())
    }
    fn OnPropertyValueChanged(&self, id: &PCWSTR, key: &PROPERTYKEY) -> windows::core::Result<()> {
        if key.fmtid == PKEY_DEVICE_FORMAT.fmtid {
            let id = unsafe { id.to_string() }.unwrap_or_default();
            (self.on_event)(DeviceEvent::FormatChanged(id));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Read-only: lists devices and probes formats; changes nothing.
    #[test]
    fn enumerate_devices_read_only() {
        let b = WasapiBackend::new();
        let devices = b.devices();
        eprintln!("default: {:?}", b.default_device());
        for d in &devices {
            let rates = b.available_sample_rates(&d.id);
            let cur = b.current_format(&d.id);
            let bits = cur.map(|(r, _)| b.available_bit_depths(&d.id, r)).unwrap_or_default();
            eprintln!("{} | current {:?} | exclusive rates {:?} | bits@current {:?}", d.name, cur, rates, bits);
        }
    }

    /// Opens the default device in shared mode and renders silence for 0.5 s.
    #[test]
    fn shared_stream_renders_silence() {
        use std::sync::atomic::AtomicUsize;
        let b = WasapiBackend::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let c2 = calls.clone();
        let s = b
            .open(&StreamSpec::default(), Box::new(move |buf, _ch| {
                buf.fill(0.0);
                c2.fetch_add(1, Ordering::Relaxed);
                0
            }))
            .expect("open shared stream");
        eprintln!("shared format: {:?}", s.format());
        std::thread::sleep(std::time::Duration::from_millis(500));
        assert!(s.error().is_none(), "{:?}", s.error());
        assert!(calls.load(Ordering::Relaxed) > 5, "render callback should run every period");
    }

    /// Audible: a quiet 440 Hz tone for 1 s. `cargo test -p fl-audio -- --ignored tone`
    #[test]
    #[ignore]
    fn tone() {
        let b = WasapiBackend::new();
        let mut phase = 0f64;
        let s = b
            .open(&StreamSpec::default(), Box::new(move |buf, ch| {
                let frames = buf.len() / ch;
                for f in 0..frames {
                    let v = (phase.sin() * 0.1) as f32;
                    phase += 2.0 * std::f64::consts::PI * 440.0 / 48_000.0;
                    buf[f * ch..(f + 1) * ch].fill(v);
                }
                frames
            }))
            .unwrap();
        std::thread::sleep(std::time::Duration::from_secs(1));
        assert!(s.error().is_none());
    }

    #[test]
    fn extensible_layout() {
        let f = extensible(96_000, 2, SampleFormat::I24In32);
        let (align, cb) = (f.Format.nBlockAlign, f.Format.cbSize);
        assert_eq!((align, cb), (8, 22));
        let d = unsafe { describe(&f.Format) }.unwrap();
        assert_eq!(d, StreamFormat { sample_rate: 96_000, channels: 2, sample: SampleFormat::I24In32 });
    }
}
