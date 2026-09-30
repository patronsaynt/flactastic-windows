//! `AudioOutputManager` (`AudioOutputDevices.swift`): the user's output
//! selection, the effective device's capabilities, and hot-plug handling.
//!
//! Gapless contract: the engine resamples every track to one fixed device
//! rate, so nothing here changes the rate between tracks. Config is applied
//! only at launch or on an explicit user / hardware event.

use std::sync::Arc;

use serde::Serialize;

use crate::engine::Engine;
use crate::output::{Backend, DeviceInfo, StreamSpec};

/// The persisted choice (`flactastic.outputDeviceUID` / `outputSampleRate` /
/// `outputBitDepth` / `outputExclusiveMode`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OutputSelection {
    pub device_id: Option<String>,
    pub sample_rate: Option<f64>,
    pub bit_depth: Option<i64>,
    pub exclusive: bool,
}

/// What the Settings → Audio pane shows.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputStatus {
    pub devices: Vec<DeviceInfo>,
    pub default_device_id: Option<String>,
    pub effective_device_id: Option<String>,
    pub selected_device_id: Option<String>,
    pub selected_sample_rate: Option<f64>,
    pub selected_bit_depth: Option<i64>,
    pub exclusive: bool,
    pub is_selected_device_missing: bool,
    pub available_sample_rates: Vec<f64>,
    pub available_bit_depths: Vec<i64>,
    pub current_sample_rate: Option<f64>,
    pub current_bit_depth: Option<i64>,
    /// The engine's actual stream rate, for the "Output" readout.
    pub stream_sample_rate: Option<u32>,
    pub backend: &'static str,
}

pub struct OutputManager {
    backend: Arc<dyn Backend>,
    pub selection: OutputSelection,
    devices: Vec<DeviceInfo>,
    default_device: Option<String>,
    available_sample_rates: Vec<f64>,
    available_bit_depths: Vec<i64>,
    current_sample_rate: Option<f64>,
    current_bit_depth: Option<i64>,
}

impl OutputManager {
    pub fn new(backend: Arc<dyn Backend>, selection: OutputSelection) -> OutputManager {
        OutputManager {
            backend,
            selection,
            devices: Vec::new(),
            default_device: None,
            available_sample_rates: Vec::new(),
            available_bit_depths: Vec::new(),
            current_sample_rate: None,
            current_bit_depth: None,
        }
    }

    /// The saved device when connected, otherwise the system default.
    pub fn effective_device(&self) -> Option<String> {
        if let Some(id) = &self.selection.device_id {
            if self.devices.iter().any(|d| &d.id == id) {
                return Some(id.clone());
            }
        }
        self.default_device.clone()
    }

    pub fn is_selected_device_missing(&self) -> bool {
        self.selection.device_id.as_ref().is_some_and(|id| !self.devices.iter().any(|d| &d.id == id))
    }

    /// Launch: enumerate and apply the saved configuration before any playback.
    pub fn start(&mut self, engine: &mut Engine) {
        self.refresh_devices();
        self.apply(engine);
    }

    pub fn select_device(&mut self, engine: &mut Engine, id: Option<String>) {
        if id == self.selection.device_id {
            return;
        }
        self.selection.device_id = id;
        self.refresh_capabilities();
        // Drop choices the new device can't honour.
        if self.selection.sample_rate.is_some_and(|r| !self.available_sample_rates.contains(&r)) {
            self.selection.sample_rate = None;
        }
        self.drop_unsupported_bit_depth();
        self.apply(engine);
    }

    pub fn select_sample_rate(&mut self, engine: &mut Engine, rate: Option<f64>) {
        if rate == self.selection.sample_rate {
            return;
        }
        self.selection.sample_rate = rate;
        self.refresh_capabilities();
        self.drop_unsupported_bit_depth();
        self.apply(engine);
    }

    pub fn select_bit_depth(&mut self, engine: &mut Engine, bits: Option<i64>) {
        if bits == self.selection.bit_depth {
            return;
        }
        self.selection.bit_depth = bits;
        self.apply(engine);
    }

    /// Windows-only exclusive mode toggle.
    pub fn set_exclusive(&mut self, engine: &mut Engine, on: bool) {
        if on == self.selection.exclusive {
            return;
        }
        self.selection.exclusive = on;
        self.apply(engine);
    }

    fn drop_unsupported_bit_depth(&mut self) {
        if self.selection.bit_depth.is_some_and(|b| !self.available_bit_depths.contains(&b)) {
            self.selection.bit_depth = None;
        }
    }

    /// Saved rate / bit depth are used only when the effective device
    /// supports them, so falling back to the default device never wipes the
    /// user's choice.
    fn apply(&mut self, engine: &mut Engine) {
        let Some(device) = self.effective_device() else { return };
        let rates = self.backend.available_sample_rates(&device);
        let rate = self.selection.sample_rate.filter(|r| rates.contains(r));
        let current = self.backend.current_format(&device);
        let depth_rate = rate.or(current.map(|c| c.0)).unwrap_or(0.0);
        let depths = self.backend.available_bit_depths(&device, depth_rate);
        let bits = self.selection.bit_depth.filter(|b| depths.contains(b));

        // A pinned device that's missing plays through the default, which the
        // engine follows by passing no ID.
        let device_id = if self.selection.device_id.as_deref() == Some(device.as_str()) { Some(device) } else { None };
        engine.apply_output(StreamSpec { device_id, sample_rate: rate, bit_depth: bits, exclusive: self.selection.exclusive });
        self.refresh_capabilities();
    }

    /// Devices added/removed or the default moved. Re-routes only when the
    /// device we should be using actually changed.
    pub fn handle_system_change(&mut self, engine: &mut Engine) {
        let previous = self.effective_device();
        self.refresh_devices();
        if self.effective_device() != previous {
            self.apply(engine);
        } else {
            engine.handle_configuration_change();
        }
    }

    /// A device format changed underneath us (display refresh plus letting the
    /// engine rebuild if its stream no longer matches).
    pub fn handle_format_change(&mut self, engine: &mut Engine) {
        self.refresh_capabilities();
        engine.handle_configuration_change();
    }

    pub fn refresh_devices(&mut self) {
        self.devices = self.backend.devices();
        self.default_device = self.backend.default_device();
        self.refresh_capabilities();
    }

    fn refresh_capabilities(&mut self) {
        let Some(device) = self.effective_device() else {
            self.available_sample_rates.clear();
            self.available_bit_depths.clear();
            self.current_sample_rate = None;
            self.current_bit_depth = None;
            return;
        };
        self.available_sample_rates = self.backend.available_sample_rates(&device);
        let cur = self.backend.current_format(&device);
        self.current_sample_rate = cur.map(|c| c.0);
        self.current_bit_depth = cur.and_then(|c| c.1);
        let depth_rate = self.selection.sample_rate.or(self.current_sample_rate).unwrap_or(0.0);
        self.available_bit_depths = self.backend.available_bit_depths(&device, depth_rate);
    }

    pub fn status(&self, engine: &Engine) -> OutputStatus {
        OutputStatus {
            devices: self.devices.clone(),
            default_device_id: self.default_device.clone(),
            effective_device_id: self.effective_device(),
            selected_device_id: self.selection.device_id.clone(),
            selected_sample_rate: self.selection.sample_rate,
            selected_bit_depth: self.selection.bit_depth,
            exclusive: self.selection.exclusive,
            is_selected_device_missing: self.is_selected_device_missing(),
            available_sample_rates: self.available_sample_rates.clone(),
            available_bit_depths: self.available_bit_depths.clone(),
            current_sample_rate: self.current_sample_rate,
            current_bit_depth: self.current_bit_depth,
            stream_sample_rate: engine.output_format().map(|f| f.sample_rate),
            backend: self.backend.name(),
        }
    }
}
