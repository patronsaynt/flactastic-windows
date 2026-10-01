//! `DiscordPresenceService`'s IPC half: framed JSON to the local Discord
//! client. Windows uses the `\\.\pipe\discord-ipc-N` named pipes; Linux the
//! `discord-ipc-N` sockets under `$XDG_RUNTIME_DIR` (plus the Flatpak and Snap
//! sandboxes), then the temp directories. Silently no-ops when Discord isn't
//! running and reconnects with backoff.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

/// FLACtastic's Discord application.
pub const CLIENT_ID: &str = "1498596791971741756";

/// A connected transport.
trait Conn: Read + Write + Send {
    /// Discards pending reply frames; false once Discord has hung up.
    fn drain(&mut self) -> bool;
    /// Waits up to `timeout` for at least one readable byte.
    fn wait_readable(&mut self, timeout: Duration) -> bool;
}

#[cfg(windows)]
mod transport {
    use super::*;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::Pipes::PeekNamedPipe;

    pub struct Pipe(std::fs::File);

    fn available(f: &std::fs::File) -> Option<u32> {
        let mut avail = 0u32;
        let ok = unsafe {
            PeekNamedPipe(f.as_raw_handle() as _, std::ptr::null_mut(), 0, std::ptr::null_mut(), &mut avail, std::ptr::null_mut())
        };
        (ok != 0).then_some(avail)
    }

    impl Read for Pipe {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.0.read(buf)
        }
    }

    impl Write for Pipe {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.write(buf)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            self.0.flush()
        }
    }

    impl Conn for Pipe {
        // A blocking read would also stall writes on this synchronous handle,
        // so only read what PeekNamedPipe says is already there.
        fn drain(&mut self) -> bool {
            let mut buf = [0u8; 4096];
            loop {
                match available(&self.0) {
                    None => return false,
                    Some(0) => return true,
                    Some(n) => {
                        let take = (n as usize).min(buf.len());
                        if self.0.read(&mut buf[..take]).map_or(true, |r| r == 0) {
                            return false;
                        }
                    }
                }
            }
        }

        fn wait_readable(&mut self, timeout: Duration) -> bool {
            let deadline = Instant::now() + timeout;
            loop {
                match available(&self.0) {
                    Some(n) if n > 0 => return true,
                    None => return false,
                    _ => {}
                }
                if Instant::now() >= deadline {
                    return false;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }

    pub fn candidates() -> Vec<PathBuf> {
        (0..10).map(|n| PathBuf::from(format!(r"\\.\pipe\discord-ipc-{n}"))).collect()
    }

    pub fn connect(path: &std::path::Path) -> Option<Box<dyn Conn>> {
        let f = std::fs::OpenOptions::new().read(true).write(true).open(path).ok()?;
        Some(Box::new(Pipe(f)))
    }
}

#[cfg(unix)]
mod transport {
    use super::*;
    use std::os::unix::net::UnixStream;

    pub struct Sock(UnixStream);

    impl Read for Sock {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.0.read(buf)
        }
    }

    impl Write for Sock {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.write(buf)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            self.0.flush()
        }
    }

    impl Conn for Sock {
        fn drain(&mut self) -> bool {
            if self.0.set_nonblocking(true).is_err() {
                return false;
            }
            let mut buf = [0u8; 4096];
            let alive = loop {
                match self.0.read(&mut buf) {
                    Ok(0) => break false,
                    Ok(_) => continue,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break true,
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => break false,
                }
            };
            alive && self.0.set_nonblocking(false).is_ok()
        }

        fn wait_readable(&mut self, _timeout: Duration) -> bool {
            // The socket's read timeout bounds the following read.
            true
        }
    }

    /// `$XDG_RUNTIME_DIR`, `$TMPDIR`, `$TMP`, `$TEMP`, `/tmp`, each also with
    /// the Flatpak and Snap sub-paths.
    pub fn candidates() -> Vec<PathBuf> {
        let mut bases: Vec<PathBuf> = ["XDG_RUNTIME_DIR", "TMPDIR", "TMP", "TEMP"]
            .iter()
            .filter_map(|k| std::env::var_os(k))
            .map(PathBuf::from)
            .collect();
        bases.push(PathBuf::from("/tmp"));
        let mut out = Vec::new();
        for base in bases {
            for sub in ["", "app/com.discordapp.Discord", ".flatpak/com.discordapp.Discord/xdg-run", "snap.discord"] {
                let dir = if sub.is_empty() { base.clone() } else { base.join(sub) };
                for n in 0..10 {
                    let p = dir.join(format!("discord-ipc-{n}"));
                    if !out.contains(&p) {
                        out.push(p);
                    }
                }
            }
        }
        out
    }

    pub fn connect(path: &std::path::Path) -> Option<Box<dyn Conn>> {
        let s = UnixStream::connect(path).ok()?;
        // Keep a wedged Discord from blocking us forever.
        let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
        let _ = s.set_write_timeout(Some(Duration::from_secs(2)));
        Some(Box::new(Sock(s)))
    }
}

/// Clamps a Discord activity string to 2…128 UTF-8 bytes: pads with a space
/// when too short, truncates on a character boundary with "…" when too long.
pub fn clamp_field(s: &str) -> String {
    const MAX: usize = 128;
    if (2..=MAX).contains(&s.len()) {
        return s.to_owned();
    }
    if s.len() < 2 {
        return format!("{s} ");
    }
    let budget = MAX - "…".len();
    let mut out = String::new();
    for ch in s.chars() {
        if out.len() + ch.len_utf8() > budget {
            break;
        }
        out.push(ch);
    }
    out.push('…');
    out
}

fn frame(opcode: u32, body: &Value) -> Vec<u8> {
    let body = serde_json::to_vec(body).unwrap_or_default();
    let mut f = Vec::with_capacity(8 + body.len());
    f.extend_from_slice(&opcode.to_le_bytes());
    f.extend_from_slice(&(body.len() as u32).to_le_bytes());
    f.extend_from_slice(&body);
    f
}

fn nonce() -> String {
    format!("{:016x}{:016x}", fastrand_u64(), fastrand_u64())
}

fn fastrand_u64() -> u64 {
    let mut b = [0u8; 8];
    let _ = getrandom::fill(&mut b);
    u64::from_le_bytes(b)
}

pub struct Activity<'a> {
    pub details: &'a str,
    pub state: &'a str,
    pub album: &'a str,
    pub artwork_url: Option<&'a str>,
    pub start_ms: Option<i64>,
    pub end_ms: Option<i64>,
}

/// The `SET_ACTIVITY` activity object ("Listening to", type 2). A resolved
/// cover goes large with the logo as the small overlay; otherwise the logo
/// stays large.
pub fn activity_json(a: &Activity) -> Value {
    let large_text = clamp_field(if a.album.is_empty() { "FLACtastic" } else { a.album });
    let assets = match a.artwork_url.filter(|u| !u.is_empty()) {
        Some(url) => json!({
            "large_image": url,
            "large_text": large_text,
            "small_image": "flactastic_logo",
            "small_text": "FLACtastic",
        }),
        None => json!({ "large_image": "flactastic_logo", "large_text": large_text }),
    };
    let mut activity = json!({
        "type": 2,
        "details": clamp_field(if a.details.is_empty() { "Unknown Track" } else { a.details }),
        "state": clamp_field(if a.state.is_empty() { "FLACtastic" } else { a.state }),
        "assets": assets,
    });
    if let (Some(s), Some(e)) = (a.start_ms, a.end_ms) {
        activity["timestamps"] = json!({ "start": s, "end": e });
    }
    activity
}

pub struct DiscordIpc {
    client_id: String,
    conn: Option<Box<dyn Conn>>,
    next_reconnect: Instant,
    backoff: Duration,
    paths: Vec<PathBuf>,
}

impl DiscordIpc {
    pub fn new(client_id: &str) -> Self {
        Self::with_paths(client_id, transport::candidates())
    }

    fn with_paths(client_id: &str, paths: Vec<PathBuf>) -> Self {
        DiscordIpc {
            client_id: client_id.to_owned(),
            conn: None,
            next_reconnect: Instant::now(),
            backoff: Duration::from_secs(1),
            paths,
        }
    }

    pub fn set_activity(&mut self, a: &Activity) -> bool {
        self.send_activity(activity_json(a))
    }

    pub fn clear_activity(&mut self) -> bool {
        self.send_activity(Value::Null)
    }

    pub fn disconnect(&mut self) {
        self.conn = None;
    }

    fn send_activity(&mut self, activity: Value) -> bool {
        if !self.ensure_connected() {
            return false;
        }
        let payload = json!({
            "cmd": "SET_ACTIVITY",
            "nonce": nonce(),
            "args": { "pid": std::process::id(), "activity": activity },
        });
        let conn = self.conn.as_mut().unwrap();
        if conn.write_all(&frame(1, &payload)).is_err() {
            self.conn = None;
            return false;
        }
        // Discord answers every SET_ACTIVITY; consume replies so they don't
        // pile up, and notice early when Discord has hung up.
        if !conn.drain() {
            self.conn = None;
        }
        true
    }

    fn ensure_connected(&mut self) -> bool {
        if self.conn.is_some() {
            return true;
        }
        if Instant::now() < self.next_reconnect {
            return false;
        }
        for p in &self.paths {
            let Some(mut c) = transport::connect(p) else { continue };
            if handshake(&mut *c, &self.client_id) {
                self.conn = Some(c);
                self.backoff = Duration::from_secs(1);
                return true;
            }
        }
        self.next_reconnect = Instant::now() + self.backoff;
        self.backoff = (self.backoff * 2).min(Duration::from_secs(30));
        false
    }
}

/// Sends the handshake and reads (discards) the READY frame.
fn handshake(c: &mut dyn Conn, client_id: &str) -> bool {
    if c.write_all(&frame(0, &json!({ "v": 1, "client_id": client_id }))).is_err() {
        return false;
    }
    if !c.wait_readable(Duration::from_secs(2)) {
        return false;
    }
    let mut header = [0u8; 8];
    if c.read_exact(&mut header).is_err() {
        return false;
    }
    let len = u32::from_le_bytes([header[4], header[5], header[6], header[7]]) as usize;
    if len >= 1_000_000 {
        return false;
    }
    let mut body = vec![0u8; len];
    c.read_exact(&mut body).is_ok()
}

// MARK: - Album artwork (iTunes Search)

/// A per-album cover URL from Apple's keyless iTunes Search API (Discord
/// proxies arbitrary HTTPS images), upgraded from 100×100 to 512×512.
pub fn itunes_album_artwork(artist: &str, album: &str) -> Option<String> {
    if album.is_empty() {
        return None;
    }
    let term = if artist.is_empty() { album.to_owned() } else { format!("{artist} {album}") };
    let mut resp = crate::agent()
        .get("https://itunes.apple.com/search")
        .query("term", &term)
        .query("entity", "album")
        .query("limit", "1")
        .call()
        .ok()?;
    let v: Value = resp.body_mut().read_json().ok()?;
    let small = v.pointer("/results/0/artworkUrl100")?.as_str()?;
    Some(small.replace("100x100bb", "512x512bb"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamps_fields() {
        assert_eq!(clamp_field("a"), "a ");
        assert_eq!(clamp_field("ok"), "ok");
        let long = "é".repeat(100);
        let c = clamp_field(&long);
        assert!(c.len() <= 128 && c.ends_with('…'));
        assert_eq!(c.chars().filter(|&ch| ch == 'é').count(), 62);
    }

    #[test]
    fn activity_shape() {
        let a = activity_json(&Activity {
            details: "Song",
            state: "by A",
            album: "",
            artwork_url: Some("https://x/512x512bb.jpg"),
            start_ms: Some(10),
            end_ms: Some(20),
        });
        assert_eq!(a["type"], 2);
        assert_eq!(a["assets"]["large_image"], "https://x/512x512bb.jpg");
        assert_eq!(a["assets"]["small_image"], "flactastic_logo");
        assert_eq!(a["assets"]["large_text"], "FLACtastic");
        assert_eq!(a["timestamps"]["end"], 20);
        let b = activity_json(&Activity { details: "", state: "", album: "LP", artwork_url: None, start_ms: None, end_ms: None });
        assert_eq!(b["details"], "Unknown Track");
        assert_eq!(b["assets"]["large_image"], "flactastic_logo");
        assert!(b.get("timestamps").is_none());
    }

    #[test]
    fn frames_are_little_endian() {
        let f = frame(1, &json!({"a": 1}));
        assert_eq!(&f[..4], &[1, 0, 0, 0]);
        assert_eq!(u32::from_le_bytes([f[4], f[5], f[6], f[7]]) as usize, f.len() - 8);
    }

    /// A fake Discord: accept, read the handshake, answer READY, then read
    /// one SET_ACTIVITY frame and reply.
    #[cfg(windows)]
    #[test]
    fn handshake_and_activity_over_a_pipe() {
        use std::os::windows::io::FromRawHandle;
        use windows_sys::Win32::Storage::FileSystem::PIPE_ACCESS_DUPLEX;
        use windows_sys::Win32::System::Pipes::{ConnectNamedPipe, CreateNamedPipeW, PIPE_TYPE_BYTE, PIPE_WAIT};

        let name = format!(r"\\.\pipe\fl-discord-test-{}", std::process::id());
        let wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
        let h = unsafe { CreateNamedPipeW(wide.as_ptr(), PIPE_ACCESS_DUPLEX, PIPE_TYPE_BYTE | PIPE_WAIT, 1, 65536, 65536, 0, std::ptr::null()) };
        assert!(!h.is_null() && h as isize != -1);
        let h = h as usize;
        let server = std::thread::spawn(move || {
            let h = h as windows_sys::Win32::Foundation::HANDLE;
            unsafe { ConnectNamedPipe(h, std::ptr::null_mut()) };
            let mut f = unsafe { std::fs::File::from_raw_handle(h as _) };
            let read_frame = |f: &mut std::fs::File| -> (u32, Value) {
                let mut hdr = [0u8; 8];
                f.read_exact(&mut hdr).unwrap();
                let len = u32::from_le_bytes(hdr[4..8].try_into().unwrap()) as usize;
                let mut body = vec![0u8; len];
                f.read_exact(&mut body).unwrap();
                (u32::from_le_bytes(hdr[..4].try_into().unwrap()), serde_json::from_slice(&body).unwrap())
            };
            let (op, hs) = read_frame(&mut f);
            assert_eq!((op, hs["client_id"].as_str()), (0, Some("123")));
            f.write_all(&frame(1, &json!({"cmd": "DISPATCH", "evt": "READY"}))).unwrap();
            let (op, act) = read_frame(&mut f);
            f.write_all(&frame(1, &json!({"cmd": "SET_ACTIVITY"}))).unwrap();
            (op, act)
        });
        let mut ipc = DiscordIpc::with_paths("123", vec![PathBuf::from(&name)]);
        let ok = ipc.set_activity(&Activity {
            details: "Song",
            state: "by A",
            album: "LP",
            artwork_url: None,
            start_ms: None,
            end_ms: None,
        });
        assert!(ok);
        let (op, act) = server.join().unwrap();
        assert_eq!(op, 1);
        assert_eq!(act["cmd"], "SET_ACTIVITY");
        assert_eq!(act["args"]["activity"]["details"], "Song");
        assert_eq!(act["args"]["pid"], std::process::id());
    }

    #[test]
    fn no_discord_backs_off() {
        let mut ipc = DiscordIpc::with_paths("1", vec![PathBuf::from("/nonexistent/discord-ipc-0")]);
        assert!(!ipc.clear_activity());
        // Within the backoff window we don't even try.
        assert!(!ipc.ensure_connected());
        assert_eq!(ipc.backoff, Duration::from_secs(2));
    }
}
