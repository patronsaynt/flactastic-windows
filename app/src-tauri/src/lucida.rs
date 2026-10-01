//! `LucidaWebController` + `LucidaWebProvider`: a hidden webview window
//! parked on https://lucida.to so every API call and file download carries the
//! browser's own Cloudflare clearance.
//!
//! The page gets no IPC: bridge promises park their JSON envelope on `window`
//! and Rust polls it back with `eval_with_callback`. Downloads go through the
//! webview's own download manager (`on_download`), which only accepts ones we
//! triggered. The webview keeps its own data directory, so clearing site data
//! never touches the main window's storage.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::{bounded, unbounded, Receiver, Sender};
use fl_net::lucida::{self as wire, Options};
use fl_net::odesli::AmazonMatcher;
use fl_net::remote::{RemoteResolve, RemoteTrack};
use parking_lot::{Condvar, Mutex};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tauri::webview::{DownloadEvent, NewWindowResponse, PageLoadEvent};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

const LABEL: &str = "lucida";
const ENTRY_URL: &str = "https://lucida.to/";

/// Minimum spacing between job initiations, plus jitter: Lucida rate-limits
/// job *starts* per IP.
const MIN_INITIATION: Duration = Duration::from_millis(1750);
const INITIATION_JITTER_MS: u64 = 500;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "message")]
pub enum Phase {
    Idle,
    Loading,
    Ready,
    Failed(String),
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub id: u64,
    pub timestamp: i64,
    pub kind: &'static str,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LucidaState {
    pub phase: Phase,
    pub needs_user_challenge: bool,
}

struct Inner {
    phase: Phase,
    /// The last load landed on a Cloudflare check that needs a real click.
    needs_user_challenge: bool,
    /// The challenge is surfaced at most once per session; later interstitials
    /// fall through to normal failures.
    prompted_this_session: bool,
    log: VecDeque<LogEntry>,
}

enum DlEvent {
    Started(PathBuf),
    Finished(bool),
}

/// Download bookkeeping between `start_download` and the `on_download` hook.
#[derive(Default)]
struct Downloads {
    /// The one download we're waiting to see requested (triggers are serialized).
    pending: Option<(PathBuf, Sender<DlEvent>)>,
    /// Destination → listener for requested, unfinished downloads.
    active: HashMap<PathBuf, Sender<DlEvent>>,
}

pub struct Lucida {
    app: AppHandle,
    data_dir: PathBuf,
    inner: Mutex<Inner>,
    ready: Condvar,
    seq: AtomicU64,
    next_initiation: Mutex<Instant>,
    /// Options keyed by `RemoteTrack.id`, stamped just before enqueueing.
    options: Mutex<HashMap<String, Options>>,
    downloads: Mutex<Downloads>,
    /// Serializes trigger → `Requested` so each event maps to its caller.
    download_gate: Mutex<()>,
    pub amazon: Arc<AmazonMatcher>,
}

/// `window.__flac` helpers (verbatim endpoints from the Mac bridge) plus the
/// result mailbox Rust polls.
const BRIDGE_SCRIPT: &str = r#"
(function () {
  if (window.__flac) return;
  const enc = encodeURIComponent;
  async function call(fn) {
    try {
      const value = await fn();
      return JSON.stringify({ ok: true, value });
    } catch (e) {
      return JSON.stringify({ ok: false, error: String(e && e.message || e) });
    }
  }
  const out = {};
  window.__flacRun = function (id, fn) {
    Promise.resolve().then(fn).then(
      (s) => { out[id] = typeof s === 'string' ? s : JSON.stringify({ ok: false, error: 'bridge returned no envelope' }); },
      (e) => { out[id] = JSON.stringify({ ok: false, error: String(e && e.message || e) }); });
  };
  window.__flacTake = function (id) {
    if (!(id in out)) return null;
    const v = out[id];
    delete out[id];
    return v;
  };
  window.__flac = {
    ping: () => call(async () => {
      const r = await fetch('/api/load?url=' + enc('https://lucida.to/'));
      return await r.json();
    }),
    loadGet: (innerPath) => call(async () => {
      const r = await fetch('/api/load?url=' + enc(innerPath));
      if (!r.ok) throw new Error('loadGet HTTP ' + r.status);
      return await r.json();
    }),
    streamV2: (body, force) => call(async () => {
      let u = '/api/load?url=' + enc('/api/fetch/stream/v2');
      if (force) u += '&force=' + enc(force);
      const r = await fetch(u, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      });
      if (!r.ok) throw new Error('streamV2 HTTP ' + r.status);
      return await r.json();
    }),
    pollRequest: (handoff, server) => call(async () => {
      const inner = '/api/fetch/request/' + handoff;
      const u = '/api/load?url=' + enc(inner) + '&force=' + enc(server);
      const r = await fetch(u);
      if (!r.ok) throw new Error('pollRequest HTTP ' + r.status);
      return await r.json();
    }),
    metadata: (serviceURL) => call(async () => {
      const inner = '/api/fetch/metadata?url=' + enc(serviceURL);
      const r = await fetch('/api/load?url=' + enc(inner));
      if (!r.ok) throw new Error('metadata HTTP ' + r.status);
      return await r.json();
    }),
    download: (url) => {
      const a = document.createElement('a');
      a.href = url;
      a.download = '';
      a.rel = 'noopener';
      a.style.display = 'none';
      document.body.appendChild(a);
      a.click();
      a.remove();
      return true;
    },
  };
})();
"#;

/// Cloudflare "Just a moment…" / managed-challenge markers.
const CF_PROBE: &str = r#"(function () {
  const t = (document.title || '').toLowerCase();
  if (t.includes('just a moment') || t.includes('attention required')) return true;
  if (document.querySelector('#challenge-form, #challenge-stage, #challenge-running')) return true;
  if (document.querySelector('script[src*="/cdn-cgi/challenge-platform/"]')) return true;
  return false;
})()"#;

#[derive(Deserialize)]
struct Envelope<T> {
    ok: bool,
    value: Option<T>,
    error: Option<String>,
}

fn js_string(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

/// `addingPercentEncoding(withAllowedCharacters: .urlQueryAllowed)`.
fn percent(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        let keep = b.is_ascii_alphanumeric() || b"-._~!$&'()*+,;=:@/?".contains(&b);
        if keep {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

pub fn is_cancelled(e: &str) -> bool {
    e == CANCELLED
}
pub const CANCELLED: &str = "__cancelled__";

impl Lucida {
    pub fn new(app: AppHandle, data_dir: PathBuf) -> Arc<Self> {
        Arc::new(Lucida {
            app,
            data_dir,
            inner: Mutex::new(Inner {
                phase: Phase::Idle,
                needs_user_challenge: false,
                prompted_this_session: false,
                log: VecDeque::new(),
            }),
            ready: Condvar::new(),
            seq: AtomicU64::new(1),
            next_initiation: Mutex::new(Instant::now()),
            options: Mutex::default(),
            downloads: Mutex::default(),
            download_gate: Mutex::new(()),
            amazon: Arc::new(AmazonMatcher::new()),
        })
    }

    // MARK: - State + log

    pub fn state(&self) -> LucidaState {
        let i = self.inner.lock();
        LucidaState { phase: i.phase.clone(), needs_user_challenge: i.needs_user_challenge }
    }

    pub fn log(&self) -> Vec<LogEntry> {
        self.inner.lock().log.iter().cloned().collect()
    }

    pub fn clear_log(&self) {
        self.inner.lock().log.clear();
        let _ = self.app.emit("lucida://log", ());
    }

    fn add_log(&self, kind: &'static str, message: impl Into<String>) {
        let mut message = message.into();
        if message.len() > 4000 {
            let mut cut = 4000;
            while !message.is_char_boundary(cut) {
                cut -= 1;
            }
            message.truncate(cut);
            message.push('…');
        }
        log::debug!("[Lucida] {kind}: {message}");
        {
            let mut i = self.inner.lock();
            let id = self.seq.fetch_add(1, Ordering::Relaxed);
            i.log.push_back(LogEntry { id, timestamp: chrono::Utc::now().timestamp_millis(), kind, message });
            while i.log.len() > 200 {
                i.log.pop_front();
            }
        }
        let _ = self.app.emit("lucida://log", ());
    }

    fn set_phase(&self, phase: Phase, needs_challenge: Option<bool>) {
        {
            let mut i = self.inner.lock();
            i.phase = phase.clone();
            if let Some(n) = needs_challenge {
                i.needs_user_challenge = n;
            }
        }
        self.ready.notify_all();
        let st = self.state();
        self.show_challenge_window(st.needs_user_challenge);
        let _ = self.app.emit("lucida://state", st);
    }

    // MARK: - Webview

    fn window(&self) -> Option<WebviewWindow> {
        self.app.get_webview_window(LABEL)
    }

    /// Created on first use (Download tab / first bridge call), never at
    /// launch: the webview spawns its own browser process.
    fn ensure_window(self: &Arc<Self>) -> Result<(WebviewWindow, bool), String> {
        if let Some(w) = self.window() {
            return Ok((w, false));
        }
        let url: tauri::Url = ENTRY_URL.parse().map_err(|e| format!("{e}"))?;
        let (me_load, me_dl, me_new) = (self.clone(), self.clone(), self.clone());
        let _ = std::fs::create_dir_all(&self.data_dir);
        // The engine's own user agent: a spoofed one would disagree with the
        // TLS fingerprint Cloudflare sees.
        let w = WebviewWindowBuilder::new(&self.app, LABEL, WebviewUrl::External(url))
            .title("Verify with Lucida")
            .inner_size(560.0, 600.0)
            .visible(false)
            .skip_taskbar(true)
            .data_directory(self.data_dir.clone())
            .initialization_script(BRIDGE_SCRIPT)
            .on_page_load(move |wv, payload| {
                if matches!(payload.event(), PageLoadEvent::Finished) {
                    let me = me_load.clone();
                    let url = payload.url().to_string();
                    let _ = wv;
                    std::thread::spawn(move || me.did_finish(&url));
                }
            })
            .on_download(move |_wv, ev| me_dl.on_download(ev))
            .on_new_window(move |url, _| {
                // `window.open` from the page must never navigate our only view.
                me_new.add_log("info", format!("swallowed window.open: {url}"));
                NewWindowResponse::Deny
            })
            .build()
            .map_err(|e| e.to_string())?;
        let hide = w.clone();
        w.on_window_event(move |e| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = e {
                api.prevent_close();
                let _ = hide.hide();
            }
        });
        Ok((w, true))
    }

    /// Shows the webview (centered over the main window) while Cloudflare
    /// needs a click; hides it once cleared.
    fn show_challenge_window(&self, show: bool) {
        let Some(w) = self.window() else { return };
        if show {
            if let Some(main) = self.app.get_webview_window("main") {
                if let (Ok(pos), Ok(size), Ok(own)) = (main.outer_position(), main.outer_size(), w.outer_size()) {
                    let x = pos.x + (size.width as i32 - own.width as i32) / 2;
                    let y = pos.y + (size.height as i32 - own.height as i32) / 2;
                    let _ = w.set_position(tauri::PhysicalPosition::new(x, y));
                }
            }
            let _ = w.set_skip_taskbar(false);
            let _ = w.show();
            let _ = w.set_focus();
        } else if w.is_visible().unwrap_or(false) {
            let _ = w.hide();
            let _ = w.set_skip_taskbar(true);
        }
    }

    pub fn reveal_challenge(self: &Arc<Self>) {
        if self.state().needs_user_challenge {
            self.show_challenge_window(true);
        }
    }

    /// Dismisses the challenge prompt without clearing it; waiting calls then
    /// fail normally.
    pub fn dismiss_challenge(&self) {
        let phase = self.inner.lock().phase.clone();
        if phase == Phase::Loading {
            self.set_phase(Phase::Failed("Cloudflare verification was dismissed.".into()), Some(false));
        } else {
            self.set_phase(phase, Some(false));
        }
    }

    /// Begin loading lucida.to (idempotent).
    pub fn warm_up(self: &Arc<Self>) {
        {
            let mut i = self.inner.lock();
            if i.phase != Phase::Idle {
                return;
            }
            i.phase = Phase::Loading;
        }
        let _ = self.app.emit("lucida://state", self.state());
        self.add_log("nav", format!("load {ENTRY_URL}"));
        let me = self.clone();
        // Window creation from a command thread can deadlock on Windows.
        std::thread::spawn(move || match me.ensure_window() {
            // A fresh window loads the entry page itself; an existing (e.g.
            // cleared) one is navigated back.
            Ok((_, true)) => {}
            Ok((w, false)) => {
                let _ = w.navigate(ENTRY_URL.parse().unwrap());
            }
            Err(e) => {
                me.add_log("error", format!("webview failed: {e}"));
                me.set_phase(Phase::Failed(e), None);
            }
        });
    }

    pub fn reload(self: &Arc<Self>) {
        let phase = self.inner.lock().phase.clone();
        self.add_log("nav", format!("reload (phase={phase:?})"));
        self.set_phase(Phase::Loading, None);
        let me = self.clone();
        std::thread::spawn(move || match me.ensure_window() {
            Ok((_, true)) => {}
            Ok((w, false)) => {
                let _ = w.navigate(ENTRY_URL.parse().unwrap());
            }
            Err(e) => me.set_phase(Phase::Failed(e), None),
        });
    }

    /// Wipes the webview's cookies, cache and storage and returns to idle so
    /// the next warm-up renegotiates Cloudflare.
    pub fn clear_site_data(self: &Arc<Self>) {
        self.add_log("info", "clearing site data…");
        if let Some(w) = self.window() {
            if let Err(e) = w.clear_all_browsing_data() {
                self.add_log("error", format!("clear failed: {e}"));
                return;
            }
            let _ = w.navigate("about:blank".parse().unwrap());
        }
        {
            let mut i = self.inner.lock();
            i.prompted_this_session = false;
        }
        self.set_phase(Phase::Idle, Some(false));
        self.add_log("ok", "site data cleared; ready to warmUp()");
    }

    /// Shows the live webview for inspection (Settings → Debug).
    pub fn show_webview(self: &Arc<Self>) {
        if self.window().is_none() {
            self.warm_up();
        }
        let me = self.clone();
        std::thread::spawn(move || {
            for _ in 0..50 {
                if let Some(w) = me.window() {
                    let _ = w.set_skip_taskbar(false);
                    let _ = w.show();
                    let _ = w.set_focus();
                    return;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        });
    }

    fn did_finish(self: &Arc<Self>, url: &str) {
        if url.starts_with("about:") {
            return;
        }
        self.add_log("nav", format!("didFinish {url}"));
        let cf = match self.eval_value(CF_PROBE, Duration::from_secs(10)) {
            Ok(v) => v == "true",
            Err(e) => {
                self.add_log("error", format!("bridge install failed: {e}"));
                self.set_phase(Phase::Failed(e), None);
                return;
            }
        };
        if cf {
            // Even with a CF banner the API may answer; if so, don't bother
            // the user.
            if self.bridge_ping_succeeds() {
                self.add_log("info", "cf markers present but bridge ping ok; treating as ready");
                self.set_phase(Phase::Ready, Some(false));
                self.add_log("ok", "bridge installed; ready");
                return;
            }
            let already = {
                let mut i = self.inner.lock();
                std::mem::replace(&mut i.prompted_this_session, true)
            };
            if already {
                self.add_log("info", "cloudflare reappeared; suppressing repeat prompt this session");
                return;
            }
            self.add_log("info", "cloudflare challenge detected; awaiting clearance");
            self.set_phase(Phase::Loading, Some(true));
            return;
        }
        self.set_phase(Phase::Ready, Some(false));
        self.add_log("ok", "bridge installed; ready");
    }

    fn bridge_ping_succeeds(&self) -> bool {
        match self.run_async("window.__flac && window.__flac.ping()", Duration::from_secs(20)) {
            Ok(s) => serde_json::from_str::<Envelope<serde_json::Value>>(&s).map(|e| e.ok).unwrap_or(false),
            Err(_) => false,
        }
    }

    // MARK: - JS bridge

    /// Evaluates a synchronous expression and returns its JSON-serialized value.
    fn eval_value(&self, js: &str, timeout: Duration) -> Result<String, String> {
        let w = self.window().ok_or("Lucida webview is not open")?;
        let (tx, rx) = bounded(1);
        w.eval_with_callback(js, move |s| {
            let _ = tx.send(s);
        })
        .map_err(|e| e.to_string())?;
        rx.recv_timeout(timeout).map_err(|_| "Lucida webview did not respond".to_owned())
    }

    /// Runs a promise-returning expression and waits for its string result.
    fn run_async(&self, expr: &str, timeout: Duration) -> Result<String, String> {
        let w = self.window().ok_or("Lucida webview is not open")?;
        let id = self.seq.fetch_add(1, Ordering::Relaxed);
        w.eval(format!("window.__flacRun({id}, () => ({expr}))")).map_err(|e| e.to_string())?;
        let deadline = Instant::now() + timeout;
        loop {
            std::thread::sleep(Duration::from_millis(80));
            let raw = self.eval_value(&format!("window.__flacTake && window.__flacTake({id})"), Duration::from_secs(10))?;
            if let Ok(Some(s)) = serde_json::from_str::<Option<String>>(&raw) {
                return Ok(s);
            }
            if Instant::now() > deadline {
                return Err("Lucida didn't answer in time.".into());
            }
        }
    }

    /// Suspends until the bridge is usable (`awaitReady`).
    pub fn await_ready(self: &Arc<Self>) -> Result<(), String> {
        let mut i = self.inner.lock();
        if i.phase == Phase::Idle {
            drop(i);
            self.warm_up();
            i = self.inner.lock();
        }
        let deadline = Instant::now() + Duration::from_secs(600);
        loop {
            match &i.phase {
                Phase::Ready => return Ok(()),
                Phase::Failed(m) => return Err(format!("Lucida unavailable: {m}")),
                Phase::Idle => return Err("Lucida unavailable: not loaded".into()),
                Phase::Loading => {
                    if self.ready.wait_until(&mut i, deadline).timed_out() {
                        return Err("Lucida unavailable: timed out loading lucida.to".into());
                    }
                }
            }
        }
    }

    /// `callBridge`: run a helper and decode its `{ok, value, error}` envelope.
    fn call<T: DeserializeOwned>(self: &Arc<Self>, expr: &str) -> Result<T, String> {
        self.await_ready()?;
        self.add_log("bridge", expr.chars().take(220).collect::<String>());
        let result = (|| {
            let s = self.run_async(expr, Duration::from_secs(120))?;
            let env: Envelope<serde_json::Value> =
                serde_json::from_str(&s).map_err(|e| format!("Could not decode response: {e}"))?;
            if !env.ok {
                let msg = env.error.unwrap_or_else(|| "lucida bridge error".into());
                self.add_log("error", format!("bridge !ok: {msg}"));
                // The browser's word for a worker that answered without CORS
                // headers (an error page) or not at all.
                if msg == "Failed to fetch" || msg == "Load failed" {
                    return Err("Lucida's server didn't answer. It may be busy or down — try again in a moment.".into());
                }
                return Err(msg);
            }
            let value = env.value.ok_or("Could not decode response: bridge ok=true but value missing")?;
            let preview: String = s.chars().take(600).collect();
            self.add_log("ok", format!("bridge ok ({} chars): {preview}{}", s.len(), if s.len() > 600 { "…" } else { "" }));
            serde_json::from_value(value).map_err(|e| format!("Could not decode response: {e}"))
        })();
        if let Err(e) = &result {
            self.add_log("error", format!("bridge threw: {e}"));
        }
        result
    }

    // MARK: - Provider

    pub fn set_options(&self, options: Options, track_id: &str) {
        self.options.lock().insert(track_id.to_owned(), options);
    }

    pub fn resolve(self: &Arc<Self>, url: &str) -> Result<RemoteResolve, String> {
        let raw: wire::Metadata = self.call(&format!("window.__flac.metadata({})", js_string(url)))?;
        if let Some(e) = raw.failure() {
            return Err(e);
        }
        Ok(raw.to_remote(url))
    }

    fn await_initiation_slot(&self) {
        let wait = {
            let mut next = self.next_initiation.lock();
            let now = Instant::now();
            let slot = (*next).max(now);
            *next = slot + MIN_INITIATION + Duration::from_millis(fastrand::u64(0..=INITIATION_JITTER_MS));
            slot - now
        };
        if !wait.is_zero() {
            std::thread::sleep(wait);
        }
    }

    fn request_job(self: &Arc<Self>, url: &str, options: &Options) -> Result<(String, String), String> {
        let body = wire::stream_request(url, options);
        self.await_initiation_slot();
        let r: wire::InitiateResponse = self.call(&format!("window.__flac.streamV2({body}, null)"))?;
        match (r.handoff, r.name) {
            (Some(h), Some(n)) => Ok((h, n)),
            _ => Err(r.error.unwrap_or_else(|| "Lucida couldn't start this download.".into())),
        }
    }

    /// Spotify is Lucida's least reliable source: on failure, retry once with
    /// the Amazon Music equivalent from Odesli.
    fn initiate_job(self: &Arc<Self>, url: &str, options: &Options) -> Result<(String, String), String> {
        match self.request_job(url, options) {
            Ok(v) => Ok(v),
            Err(e) => {
                let host = fl_net::spotify::split_url(url).map(|(_, h, _)| h).unwrap_or_default();
                if host != "open.spotify.com" && host != "spotify.com" {
                    return Err(e);
                }
                match self.amazon.amazon_url(url) {
                    Some(amazon) => self.request_job(&amazon, options),
                    None => Err(e),
                }
            }
        }
    }

    fn wait_for_completion(self: &Arc<Self>, handoff: &str, server: &str, cancel: &AtomicBool) -> Result<(), String> {
        let deadline = Instant::now() + Duration::from_secs(180);
        while Instant::now() < deadline {
            if cancel.load(Ordering::Relaxed) {
                return Err(CANCELLED.into());
            }
            let r: wire::PollResponse =
                self.call(&format!("window.__flac.pollRequest({}, {})", js_string(handoff), js_string(server)))?;
            match r.status.as_deref() {
                Some("completed") => return Ok(()),
                Some("error") => return Err(r.message.unwrap_or_else(|| "lucida job failed".into())),
                _ => {}
            }
            std::thread::sleep(Duration::from_millis(750));
        }
        Err("lucida job timed out".into())
    }

    /// `getStream`: start a job, wait for it, then download the result through
    /// the webview. Returns the finished temp file.
    pub fn fetch(
        self: &Arc<Self>,
        track: &RemoteTrack,
        cancel: &AtomicBool,
        mut progress: impl FnMut(u64),
    ) -> Result<PathBuf, String> {
        self.await_ready()?;
        let url = track.url.clone().ok_or("No streaming provider claims lucida://missing.")?;
        let opts = self.options.lock().remove(&track.id).unwrap_or_default();
        let (handoff, server) = self.initiate_job(&url, &opts)?;
        self.wait_for_completion(&handoff, &server, cancel)?;
        let inner = format!("/api/fetch/request/{handoff}/download");
        let outer = format!("https://lucida.to/api/load?url={}&force={}&redirect=true", percent(&inner), percent(&server));
        self.download(&outer, cancel, &mut progress)
    }

    // MARK: - Downloads

    fn on_download(&self, ev: DownloadEvent<'_>) -> bool {
        match ev {
            DownloadEvent::Requested { url, destination } => {
                let mut d = self.downloads.lock();
                let Some((dir, tx)) = d.pending.take() else {
                    drop(d);
                    self.add_log("info", format!("blocked unexpected download: {url}"));
                    return false;
                };
                let name = destination
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .filter(|n| !n.is_empty())
                    .unwrap_or_else(|| "download".into());
                let ext = Path::new(&name).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
                let dest = dir.join(if ext.is_empty() { "track".to_owned() } else { format!("track.{ext}") });
                *destination = dest.clone();
                d.active.insert(dest.clone(), tx.clone());
                let _ = tx.send(DlEvent::Started(dest));
                drop(d);
                self.add_log("info", format!("download started: {name}"));
                true
            }
            DownloadEvent::Finished { url, path, success } => {
                let mut d = self.downloads.lock();
                let key = path.as_ref().and_then(|p| d.active.keys().find(|k| *k == p).cloned());
                let key = key.or_else(|| (d.active.len() == 1).then(|| d.active.keys().next().cloned()).flatten());
                if let Some(tx) = key.and_then(|k| d.active.remove(&k)) {
                    let _ = tx.send(DlEvent::Finished(success));
                }
                drop(d);
                self.add_log(if success { "ok" } else { "error" }, format!("download finished ({success}): {url}"));
                true
            }
            _ => true,
        }
    }

    fn download(self: &Arc<Self>, url: &str, cancel: &AtomicBool, progress: &mut impl FnMut(u64)) -> Result<PathBuf, String> {
        let dir = std::env::temp_dir().join("flactastic-dl").join(format!("{:016x}", fastrand::u64(..)));
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let (tx, rx) = unbounded();
        let dest = {
            let _gate = self.download_gate.lock();
            self.downloads.lock().pending = Some((dir.clone(), tx));
            self.run_async(&format!("Promise.resolve(window.__flac.download({})).then(() => 'ok')", js_string(url)), Duration::from_secs(20))
                .inspect_err(|_| self.downloads.lock().pending = None)?;
            match rx.recv_timeout(Duration::from_secs(60)) {
                Ok(DlEvent::Started(p)) => p,
                _ => {
                    self.downloads.lock().pending = None;
                    let _ = std::fs::remove_dir_all(&dir);
                    return Err("Lucida's file never started downloading.".into());
                }
            }
        };
        let result = wait_download(&rx, &dir, cancel, progress);
        match result {
            Ok(()) => Ok(fix_extension(dest)),
            Err(e) => {
                // A cancelled transfer keeps running in the engine; clean up
                // its folder once it lands.
                let dir2 = dir.clone();
                std::thread::spawn(move || {
                    let _ = rx.recv_timeout(Duration::from_secs(1800));
                    let _ = std::fs::remove_dir_all(&dir2);
                });
                let _ = std::fs::remove_dir_all(&dir);
                Err(e)
            }
        }
    }
}

fn dir_size(dir: &Path) -> u64 {
    std::fs::read_dir(dir)
        .map(|rd| rd.flatten().filter_map(|e| e.metadata().ok()).filter(|m| m.is_file()).map(|m| m.len()).sum())
        .unwrap_or(0)
}

fn wait_download(rx: &Receiver<DlEvent>, dir: &Path, cancel: &AtomicBool, progress: &mut impl FnMut(u64)) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(3600);
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(CANCELLED.into());
        }
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(DlEvent::Finished(true)) => {
                progress(dir_size(dir));
                return Ok(());
            }
            Ok(DlEvent::Finished(false)) => return Err("The download failed.".into()),
            Ok(DlEvent::Started(_)) => {}
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => progress(dir_size(dir)),
            Err(_) => return Err("The download failed.".into()),
        }
        if Instant::now() > deadline {
            return Err("The download timed out.".into());
        }
    }
}

/// Names a file with no usable extension from its magic bytes.
fn fix_extension(path: PathBuf) -> PathBuf {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    if !ext.is_empty() && ext != "bin" {
        return path;
    }
    let mut head = [0u8; 12];
    let n = std::fs::File::open(&path).and_then(|mut f| std::io::Read::read(&mut f, &mut head)).unwrap_or(0);
    let head = &head[..n];
    let guessed = if head.starts_with(b"fLaC") {
        "flac"
    } else if head.starts_with(b"ID3") || (head.len() > 1 && head[0] == 0xFF && head[1] & 0xE0 == 0xE0) {
        "mp3"
    } else if head.len() >= 8 && &head[4..8] == b"ftyp" {
        "m4a"
    } else if head.starts_with(b"OggS") {
        "ogg"
    } else if head.starts_with(b"RIFF") {
        "wav"
    } else {
        return path;
    };
    let to = path.with_extension(guessed);
    match std::fs::rename(&path, &to) {
        Ok(()) => to,
        Err(_) => path,
    }
}


// MARK: - Commands

type L<'a> = tauri::State<'a, Arc<Lucida>>;

#[tauri::command]
pub fn lucida_state(l: L) -> LucidaState {
    l.state()
}

#[tauri::command]
pub fn lucida_log(l: L) -> Vec<LogEntry> {
    l.log()
}

#[tauri::command]
pub fn lucida_clear_log(l: L) {
    l.clear_log();
}

#[tauri::command]
pub fn lucida_warm_up(l: L) {
    l.warm_up();
}

#[tauri::command]
pub fn lucida_reload(l: L) {
    l.reload();
}

#[tauri::command]
pub fn lucida_clear_site_data(l: L) {
    l.clear_site_data();
}

#[tauri::command]
pub fn lucida_show_webview(l: L) {
    l.show_webview();
}

#[tauri::command]
pub fn lucida_reveal_challenge(l: L) {
    l.reveal_challenge();
}

#[tauri::command]
pub fn lucida_dismiss_challenge(l: L) {
    l.dismiss_challenge();
}

/// Debug builds only: run a promise-returning expression in the Lucida page
/// and return its string result (stands in for the Mac's Web Inspector).
#[tauri::command]
pub async fn lucida_debug_eval(l: L<'_>, expr: String) -> Result<String, String> {
    if !cfg!(debug_assertions) {
        return Err("unavailable".into());
    }
    let me = l.inner().clone();
    tauri::async_runtime::spawn_blocking(move || me.run_async(&expr, Duration::from_secs(60)))
        .await
        .map_err(|e| e.to_string())?
}

/// `StreamerRegistry.resolve`: route the pasted URL by host.
#[tauri::command]
pub async fn download_resolve(l: L<'_>, url: String) -> Result<RemoteResolve, String> {
    let url = url.trim().to_owned();
    let Some((scheme, host, _)) = fl_net::spotify::split_url(&url) else {
        return Err("That URL doesn't look valid.".into());
    };
    if scheme != "https" && scheme != "http" {
        return Err("That URL doesn't look valid.".into());
    }
    if !wire::claims(&host) {
        return Err(format!("No streaming provider claims {host}."));
    }
    let me = l.inner().clone();
    tauri::async_runtime::spawn_blocking(move || me.resolve(&url)).await.map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_matches_url_query_allowed() {
        assert_eq!(percent("/api/fetch/request/ab c/download"), "/api/fetch/request/ab%20c/download");
        assert_eq!(percent("srv#1"), "srv%231");
    }

    #[test]
    fn magic_extension() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("track");
        std::fs::write(&p, b"fLaC\0\0\0\x22").unwrap();
        assert_eq!(fix_extension(p).extension().unwrap(), "flac");
        let q = d.path().join("x.mp3");
        std::fs::write(&q, b"fLaC").unwrap();
        assert_eq!(fix_extension(q.clone()), q);
    }
}
