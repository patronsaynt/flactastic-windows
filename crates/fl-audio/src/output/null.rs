//! Offline "device" for tests: pulls from the engine as fast as audio is
//! available and records it. Underrun silence is not recorded, so a capture
//! of a gapless queue is exactly the concatenated output.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::*;

pub struct NullBackend {
    pub rate: u32,
    pub channels: u16,
    pub captured: Arc<Mutex<Vec<f32>>>,
    /// `Some(x)`: consume at x times real time, like a device clock.
    /// `None`: as fast as audio is available.
    pub pace: Option<f64>,
}

impl NullBackend {
    pub fn new(rate: u32) -> Arc<NullBackend> {
        Arc::new(NullBackend { rate, channels: 2, captured: Arc::default(), pace: None })
    }

    /// Consumes at `speed` times real time.
    pub fn paced(rate: u32, speed: f64) -> Arc<NullBackend> {
        Arc::new(NullBackend { rate, channels: 2, captured: Arc::default(), pace: Some(speed) })
    }

    pub fn take(&self) -> Vec<f32> {
        std::mem::take(&mut *self.captured.lock().unwrap())
    }
}

struct NullStream {
    fmt: StreamFormat,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl OutputStream for NullStream {
    fn format(&self) -> StreamFormat {
        self.fmt
    }
    fn device_id(&self) -> &str {
        "null"
    }
    fn error(&self) -> Option<String> {
        None
    }
}

impl Drop for NullStream {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Backend for NullBackend {
    fn name(&self) -> &'static str {
        "null"
    }
    fn devices(&self) -> Vec<DeviceInfo> {
        vec![DeviceInfo { id: "null".into(), name: "Null output".into() }]
    }
    fn default_device(&self) -> Option<String> {
        Some("null".into())
    }
    fn available_sample_rates(&self, _: &str) -> Vec<f64> {
        vec![f64::from(self.rate)]
    }
    fn available_bit_depths(&self, _: &str, _: f64) -> Vec<i64> {
        vec![32]
    }
    fn current_format(&self, _: &str) -> Option<(f64, Option<i64>)> {
        Some((f64::from(self.rate), Some(32)))
    }
    fn apply_device_format(&self, _: &str, _: Option<f64>, _: Option<i64>) -> Result<(), String> {
        Ok(())
    }

    fn open(&self, _spec: &StreamSpec, mut render: Render) -> Result<Box<dyn OutputStream>, String> {
        let fmt = StreamFormat { sample_rate: self.rate, channels: self.channels, sample: SampleFormat::F32 };
        let stop = Arc::new(AtomicBool::new(false));
        let captured = self.captured.clone();
        let ch = usize::from(self.channels);
        let s2 = stop.clone();
        let block_time = self.pace.map(|x| Duration::from_secs_f64(512.0 / f64::from(self.rate) / x));
        let thread = std::thread::Builder::new()
            .name("null-output".into())
            .spawn(move || {
                let mut buf = vec![0f32; 512 * ch];
                let mut next = std::time::Instant::now();
                while !s2.load(Ordering::SeqCst) {
                    if let Some(bt) = block_time {
                        next += bt;
                        if let Some(wait) = next.checked_duration_since(std::time::Instant::now()) {
                            std::thread::sleep(wait);
                        }
                    }
                    let n = render(&mut buf, ch);
                    if n > 0 {
                        captured.lock().unwrap().extend_from_slice(&buf[..n * ch]);
                    }
                    if n < 512 && block_time.is_none() {
                        std::thread::sleep(Duration::from_millis(1));
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Box::new(NullStream { fmt, stop, thread: Some(thread) }))
    }

    fn watch(&self, _: Arc<dyn Fn(DeviceEvent) + Send + Sync>) {}
}
