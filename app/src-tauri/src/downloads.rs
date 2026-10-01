//! `DownloadCoordinator`: provider fetch → temp file → tag → move into the
//! library at `Artist/Album/NN - Title.ext` → debounced rescan.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::{bounded, Sender};
use fl_core::import_copy::{move_file, sanitize_download_component};
use fl_core::{AudioFileFormat, Track, Uid};
use fl_net::remote::{RemoteCoverArt, RemoteTrack};
use fl_tags::{ArtworkChange, TagWrite};
use parking_lot::Mutex;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::lucida::{is_cancelled, Lucida};
use crate::state::AppState;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum JobStatus {
    Queued,
    #[serde(rename_all = "camelCase")]
    Downloading { received_bytes: u64, total_bytes: Option<u64> },
    Tagging,
    Finishing,
    Completed { path: String },
    Failed { message: String },
    Cancelled,
    /// Already in the library; `path` is the existing file.
    Skipped { path: String },
}

impl JobStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed { .. } | Self::Failed { .. } | Self::Cancelled | Self::Skipped { .. })
    }
}

#[derive(Debug, Clone)]
pub enum Outcome {
    Completed(PathBuf),
    Skipped(PathBuf),
    Failed(String),
    Cancelled,
}

impl Outcome {
    fn status(&self) -> JobStatus {
        match self {
            Outcome::Completed(p) => JobStatus::Completed { path: p.to_string_lossy().into_owned() },
            Outcome::Skipped(p) => JobStatus::Skipped { path: p.to_string_lossy().into_owned() },
            Outcome::Failed(m) => JobStatus::Failed { message: m.clone() },
            Outcome::Cancelled => JobStatus::Cancelled,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: String,
    pub track: RemoteTrack,
    pub status: JobStatus,
    /// Keep Lucida's embedded tags (playlist rebuilds) instead of re-tagging.
    pub trust_embedded_metadata: bool,
}

struct Entry {
    job: Job,
    cancel: Arc<AtomicBool>,
    waiter: Option<Sender<Outcome>>,
}

pub struct Downloads {
    app: AppHandle,
    jobs: Mutex<Vec<Entry>>,
    refresh_gen: Arc<AtomicU64>,
}

impl Downloads {
    pub fn new(app: AppHandle) -> Arc<Self> {
        Arc::new(Downloads { app, jobs: Mutex::default(), refresh_gen: Arc::default() })
    }

    pub fn jobs(&self) -> Vec<Job> {
        self.jobs.lock().iter().map(|e| e.job.clone()).collect()
    }

    fn emit_list(&self) {
        let _ = self.app.emit("downloads://changed", ());
    }

    fn update(&self, id: &str, status: JobStatus) {
        let job = {
            let mut jobs = self.jobs.lock();
            let Some(e) = jobs.iter_mut().find(|e| e.job.id == id) else { return };
            // A cancelled job stays cancelled even if its thread reports late.
            if e.job.status.is_terminal() {
                return;
            }
            e.job.status = status;
            e.job.clone()
        };
        let _ = self.app.emit("downloads://job", job);
    }

    /// Sets the terminal status and resolves an `enqueue_and_await` waiter,
    /// exactly once.
    fn finish(&self, id: &str, outcome: Outcome) {
        let job = {
            let mut jobs = self.jobs.lock();
            let Some(e) = jobs.iter_mut().find(|e| e.job.id == id) else { return };
            if e.job.status.is_terminal() {
                return;
            }
            e.job.status = outcome.status();
            if let Some(w) = e.waiter.take() {
                let _ = w.send(outcome);
            }
            e.job.clone()
        };
        let _ = self.app.emit("downloads://job", job);
    }

    /// In-flight duplicates (same track + service, not terminal) are dropped.
    pub fn enqueue(self: &Arc<Self>, track: RemoteTrack) {
        {
            let jobs = self.jobs.lock();
            if jobs.iter().any(|e| {
                e.job.track.id == track.id && e.job.track.service_id == track.service_id && !e.job.status.is_terminal()
            }) {
                return;
            }
        }
        self.start(track, false, None);
    }

    /// Enqueue and block until a terminal state (`enqueueAndAwait`).
    /// `on_start` receives the job id before the wait, so a caller can cancel.
    pub fn enqueue_and_await_with(
        self: &Arc<Self>,
        track: RemoteTrack,
        trust_embedded_metadata: bool,
        on_start: impl FnOnce(&str),
    ) -> (String, Outcome) {
        let (tx, rx) = bounded(1);
        let id = self.start(track, trust_embedded_metadata, Some(tx));
        on_start(&id);
        let outcome = rx.recv().unwrap_or(Outcome::Cancelled);
        (id, outcome)
    }

    fn start(self: &Arc<Self>, track: RemoteTrack, trust: bool, waiter: Option<Sender<Outcome>>) -> String {
        let id = Uid::new_v4().to_string();
        let cancel = Arc::new(AtomicBool::new(false));
        self.jobs.lock().push(Entry {
            job: Job { id: id.clone(), track, status: JobStatus::Queued, trust_embedded_metadata: trust },
            cancel: cancel.clone(),
            waiter,
        });
        self.emit_list();
        let me = self.clone();
        let id2 = id.clone();
        std::thread::spawn(move || me.run(&id2, &cancel));
        id
    }

    pub fn cancel(&self, id: &str) {
        let hit = {
            let jobs = self.jobs.lock();
            jobs.iter().find(|e| e.job.id == id && !e.job.status.is_terminal()).map(|e| e.cancel.clone())
        };
        if let Some(c) = hit {
            c.store(true, Ordering::Relaxed);
            self.finish(id, Outcome::Cancelled);
        }
    }

    pub fn cancel_all(&self) {
        let ids: Vec<String> =
            self.jobs.lock().iter().filter(|e| !e.job.status.is_terminal()).map(|e| e.job.id.clone()).collect();
        for id in ids {
            self.cancel(&id);
        }
    }

    pub fn clear_completed(&self) {
        self.jobs.lock().retain(|e| !e.job.status.is_terminal());
        self.emit_list();
    }

    /// One rescan ~1.5 s after the most recent completion.
    fn schedule_library_refresh(&self) {
        let gen = self.refresh_gen.fetch_add(1, Ordering::SeqCst) + 1;
        let app = self.app.clone();
        let me_gen = self.refresh_gen.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(1500));
            if me_gen.load(Ordering::SeqCst) != gen {
                return;
            }
            if let Some(st) = app.try_state::<Arc<AppState>>() {
                st.refresh_library(&app);
            }
        });
    }

    fn run(self: &Arc<Self>, id: &str, cancel: &AtomicBool) {
        let Some(job) = self.jobs.lock().iter().find(|e| e.job.id == id).map(|e| e.job.clone()) else { return };
        let track = &job.track;
        let st = self.app.state::<Arc<AppState>>();
        let Some(root) = st.root_path() else {
            self.finish(id, Outcome::Failed("No music folder selected. Open Settings → Config first.".into()));
            return;
        };
        let library = st.library.read().tracks();
        if let Some(existing) = find_existing_match(track, &library) {
            self.finish(id, Outcome::Skipped(existing.path.clone()));
            return;
        }
        if track.service_id != "lucida" {
            self.finish(id, Outcome::Failed(format!("Provider \"{}\" no longer registered.", track.service_id)));
            return;
        }
        let lucida = self.app.state::<Arc<Lucida>>().inner().clone();
        let result = (|| -> Result<PathBuf, String> {
            self.update(id, JobStatus::Downloading { received_bytes: 0, total_bytes: None });
            let mut last = (0u64, Instant::now());
            let temp = lucida.fetch(track, cancel, |bytes| {
                if bytes.saturating_sub(last.0) >= 262_144 || last.1.elapsed() >= Duration::from_millis(100) {
                    last = (bytes, Instant::now());
                    self.update(id, JobStatus::Downloading { received_bytes: bytes, total_bytes: None });
                }
            })?;
            let cleanup = TempCleanup(temp.clone());
            if cancel.load(Ordering::Relaxed) {
                return Err(crate::lucida::CANCELLED.into());
            }
            let ext = temp.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_else(|| "flac".into());

            if job.trust_embedded_metadata {
                // Lucida embedded the real album/artist/cover; only restore
                // the playlist's canonical title.
                if !track.title.is_empty() {
                    self.update(id, JobStatus::Tagging);
                    let _ = fl_tags::override_title(&temp, &track.title);
                }
            } else {
                self.update(id, JobStatus::Tagging);
                tag(&temp, track)?;
            }

            self.update(id, JobStatus::Finishing);
            let final_path = final_destination(&root, track, &ext);
            std::fs::create_dir_all(final_path.parent().unwrap_or(&root)).map_err(|e| e.to_string())?;
            // Never overwrite the user's copy: add a short random suffix.
            let dest = if final_path.exists() {
                let stem = final_path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                final_path.with_file_name(format!("{stem} ({:08X}).{ext}", fastrand::u32(..)))
            } else {
                final_path
            };
            move_file(&temp, &dest).map_err(|e| e.to_string())?;
            drop(cleanup);
            Ok(dest)
        })();
        match result {
            Ok(dest) => {
                self.finish(id, Outcome::Completed(dest));
                self.schedule_library_refresh();
            }
            Err(e) if is_cancelled(&e) => self.finish(id, Outcome::Cancelled),
            Err(e) => self.finish(id, Outcome::Failed(e)),
        }
    }
}

/// Deletes the temp download (and its folder) unless moved into the library.
struct TempCleanup(PathBuf);
impl Drop for TempCleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
        if let Some(dir) = self.0.parent() {
            let _ = std::fs::remove_dir(dir);
        }
    }
}

/// Tags the temp file from the remote metadata we already have.
fn tag(path: &Path, track: &RemoteTrack) -> Result<(), String> {
    let artwork = fetch_artwork(track);
    let format = AudioFileFormat::classify(path).unwrap_or(AudioFileFormat::Flac);
    let joined = track.artists.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join("; ");
    let artist = (!joined.is_empty()).then_some(joined);
    let primary = track.artists.first().map(|a| a.name.clone());
    let mut staged = Track::new(path.to_path_buf(), track.title.clone(), format);
    staged.artist = artist.clone();
    staged.album = track.album.as_ref().map(|a| a.title.clone());
    staged.track_number = track.track_number;
    staged.year = track.album.as_ref().and_then(|a| a.release_year);
    let w = TagWrite {
        title: track.title.clone(),
        artist,
        album: track.album.as_ref().map(|a| a.title.clone()),
        year: track.album.as_ref().and_then(|a| a.release_year),
        genre: None,
        secondary_genres: vec![],
        track_number: track.track_number,
        artwork: artwork.map_or(ArtworkChange::Unchanged, ArtworkChange::Updated),
        album_artist: Some(primary),
        compilation: None,
        mix_compilation: None,
    };
    fl_tags::write(&staged, &w).map(|_| ()).map_err(|e| e.to_string())
}

fn fetch_artwork(track: &RemoteTrack) -> Option<Vec<u8>> {
    let arts = if !track.cover_art.is_empty() {
        &track.cover_art
    } else {
        track.album.as_ref().map(|a| &a.cover_art)?
    };
    let best = RemoteCoverArt::best(arts)?;
    fl_net::deezer::download_image(&best.url).ok()
}

/// `<root>/<first artist>/<album>/NN - <title>.<ext>`.
pub fn final_destination(root: &Path, track: &RemoteTrack, ext: &str) -> PathBuf {
    let artist = sanitize_download_component(track.artists.first().map_or("Unknown Artist", |a| a.name.as_str()));
    let album = sanitize_download_component(track.album.as_ref().map_or("Singles", |a| a.title.as_str()));
    let prefix = track.track_number.map(|n| format!("{n:02} - ")).unwrap_or_default();
    let file = format!("{}.{ext}", sanitize_download_component(&format!("{prefix}{}", track.title)));
    root.join(artist).join(album).join(file)
}

// MARK: - Duplicate detection

fn normalize(s: &str) -> String {
    s.trim().to_lowercase()
}

/// Version markers (" - ", brackets) flattened and whitespace collapsed, so
/// "Song - Radio Edit" and "Song (Radio Edit)" compare equal.
fn loose_title(s: &str) -> String {
    let mut t = s.to_lowercase();
    for ch in ['(', ')', '[', ']'] {
        t = t.replace(ch, " ");
    }
    t = t.replace(" - ", " ");
    t.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn artist_tokens(raw: Option<&str>) -> HashSet<String> {
    let Some(raw) = raw.filter(|r| !r.is_empty()) else { return HashSet::new() };
    let mut w = raw.to_lowercase();
    for marker in [" feat.", " feat ", " ft.", " ft ", " featuring "] {
        w = w.replace(marker, ";");
    }
    w.split([';', ',', '&', '/']).map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned).collect()
}

/// `^\d{1,3}\s*-\s*` stripped from a file stem.
fn strip_leading_number(stem: &str) -> &str {
    let digits = stem.chars().take_while(|c| c.is_ascii_digit()).count();
    if !(1..=3).contains(&digits) {
        return stem;
    }
    let rest = stem[digits..].trim_start();
    match rest.strip_prefix('-') {
        Some(r) => r.trim_start(),
        None => stem,
    }
}

/// Artist overlap vetoes when both sides have artists; the album name only
/// counts when one side lacks them.
fn overlaps(t: &Track, remote_artists: &HashSet<String>, remote_album: &str) -> bool {
    let mut lib = artist_tokens(t.artist.as_deref());
    lib.extend(artist_tokens(t.album_artist.as_deref()));
    if !remote_artists.is_empty() && !lib.is_empty() {
        return !remote_artists.is_disjoint(&lib);
    }
    !remote_album.is_empty() && normalize(t.album.as_deref().unwrap_or("")) == remote_album
}

/// The library track that is the same recording as `remote`: exact file
/// name, exact tag title, then loose variants of each (which require an
/// artist/album overlap). A bare title match only counts when the remote has
/// nothing to disambiguate on.
pub fn find_existing_match<'a>(remote: &RemoteTrack, tracks: &'a [Track]) -> Option<&'a Track> {
    let exact = normalize(&remote.title);
    if exact.is_empty() {
        return None;
    }
    let loose = loose_title(&remote.title);
    let joined = remote.artists.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join("; ");
    let remote_artists = artist_tokens(Some(&joined));
    let remote_album = remote.album.as_ref().map(|a| normalize(&a.title)).unwrap_or_default();

    struct Keyed<'a> {
        track: &'a Track,
        file_exact: String,
        file_loose: String,
        tag_exact: String,
        tag_loose: String,
    }
    let keyed: Vec<Keyed> = tracks
        .iter()
        .map(|t| {
            let stem = t.path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let stem = strip_leading_number(&stem).to_owned();
            Keyed {
                track: t,
                file_exact: normalize(&stem),
                file_loose: loose_title(&stem),
                tag_exact: normalize(&t.title),
                tag_loose: loose_title(&t.title),
            }
        })
        .collect();
    let can_disambiguate = !remote_artists.is_empty() || !remote_album.is_empty();
    let tiers: [(&str, for<'k> fn(&'k Keyed<'k>) -> &'k str, bool); 4] = [
        (&exact, |k| &k.file_exact, false),
        (&exact, |k| &k.tag_exact, false),
        (&loose, |k| &k.file_loose, true),
        (&loose, |k| &k.tag_loose, true),
    ];
    for (key, get, require_overlap) in tiers {
        if key.is_empty() {
            continue;
        }
        let hits: Vec<&Track> = keyed.iter().filter(|k| get(k) == key).map(|k| k.track).collect();
        if hits.is_empty() {
            continue;
        }
        if let Some(m) = hits.iter().find(|t| overlaps(t, &remote_artists, &remote_album)) {
            return Some(m);
        }
        if require_overlap || can_disambiguate {
            continue;
        }
        return Some(hits[0]);
    }
    None
}

// MARK: - Remote artwork (ArtworkAccent)

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteArtwork {
    /// Artwork-store id (served via `flart://`).
    pub id: String,
    /// A legible accent sampled from the cover, when it has a vibrant one.
    pub accent: Option<[u8; 3]>,
}

/// `ArtworkAccent.dominant`: 32×32 downsample, coarse RGB buckets weighted by
/// saturation (skipping near-black and greyish pixels), then the winner's
/// saturation and brightness lifted so it reads on the dark UI.
pub fn artwork_accent(data: &[u8]) -> Option<[u8; 3]> {
    let img = image::load_from_memory(data).ok()?;
    let small = img.resize_exact(32, 32, image::imageops::FilterType::Triangle).to_rgb8();
    let mut buckets: std::collections::HashMap<u32, [f64; 4]> = std::collections::HashMap::new();
    for p in small.pixels() {
        let (r, g, b) = (f64::from(p[0]) / 255.0, f64::from(p[1]) / 255.0, f64::from(p[2]) / 255.0);
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let sat = if max == 0.0 { 0.0 } else { (max - min) / max };
        if max < 0.12 || sat < 0.18 {
            continue;
        }
        let key = (((r * 4.0) as u32) << 6) | (((g * 4.0) as u32) << 3) | ((b * 4.0) as u32);
        let e = buckets.entry(key).or_insert([0.0; 4]);
        e[0] += sat;
        e[1] += r * sat;
        e[2] += g * sat;
        e[3] += b * sat;
    }
    let top = buckets.values().copied().fold(None::<[f64; 4]>, |best, e| match best {
        Some(b) if b[0] >= e[0] => Some(b),
        _ => Some(e),
    })?;
    if top[0] <= 0.0 {
        return None;
    }
    let (r, g, b) = (top[1] / top[0], top[2] / top[0], top[3] / top[0]);
    let (h, s, v) = rgb_to_hsv(r, g, b);
    let (r, g, b) = hsv_to_rgb(h, s.max(0.55), v.max(0.72));
    Some([(r * 255.0).round() as u8, (g * 255.0).round() as u8, (b * 255.0).round() as u8])
}

fn rgb_to_hsv(r: f64, g: f64, b: f64) -> (f64, f64, f64) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d == 0.0 {
        0.0
    } else if max == r {
        ((g - b) / d).rem_euclid(6.0) / 6.0
    } else if max == g {
        ((b - r) / d + 2.0) / 6.0
    } else {
        ((r - g) / d + 4.0) / 6.0
    };
    (h, if max == 0.0 { 0.0 } else { d / max }, max)
}

fn hsv_to_rgb(h: f64, s: f64, v: f64) -> (f64, f64, f64) {
    let h6 = (h * 6.0).rem_euclid(6.0);
    let c = v * s;
    let x = c * (1.0 - ((h6 % 2.0) - 1.0).abs());
    let (r, g, b) = match h6 as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    (r + m, g + m, b + m)
}

/// Downloads a remote cover into the artwork store and samples its accent.
#[tauri::command]
pub async fn remote_artwork(st: State<'_, Arc<AppState>>, url: String) -> Result<RemoteArtwork, String> {
    let st = st.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let bytes = fl_net::deezer::download_image(&url).map_err(|e| e.to_string())?;
        let accent = artwork_accent(&bytes);
        let id = st.artwork.register_bytes(&format!("remote:{url}"), bytes);
        Ok(RemoteArtwork { id, accent })
    })
    .await
    .map_err(|e| e.to_string())?
}

// MARK: - Commands

type St<'a> = State<'a, Arc<Downloads>>;

#[tauri::command]
pub fn download_jobs(st: St) -> Vec<Job> {
    st.jobs()
}

#[tauri::command]
pub fn download_enqueue(st: St, lucida: State<Arc<Lucida>>, tracks: Vec<RemoteTrack>, options: fl_net::lucida::Options) {
    for t in tracks {
        lucida.set_options(options.clone(), &t.id);
        st.enqueue(t);
    }
}

#[tauri::command]
pub fn download_cancel(st: St, id: String) {
    st.cancel(&id);
}

#[tauri::command]
pub fn download_cancel_all(st: St) {
    st.cancel_all();
}

#[tauri::command]
pub fn download_clear_completed(st: St) {
    st.clear_completed();
}

#[cfg(test)]
mod tests {
    use super::*;
    use fl_net::remote::{RemoteAlbumRef, RemoteArtist};

    fn remote(title: &str, artists: &[&str], album: Option<&str>) -> RemoteTrack {
        RemoteTrack {
            id: title.into(),
            title: title.into(),
            artists: artists.iter().map(|a| RemoteArtist::named(a)).collect(),
            album: album.map(|a| RemoteAlbumRef {
                id: a.into(),
                title: a.into(),
                url: None,
                cover_art: vec![],
                release_year: None,
                track_count: None,
            }),
            track_number: None,
            disc_number: None,
            duration_seconds: None,
            cover_art: vec![],
            url: None,
            service_id: "lucida".into(),
            is_lossless: true,
        }
    }

    fn lib(path: &str, title: &str, artist: Option<&str>, album: Option<&str>) -> Track {
        let mut t = Track::new(PathBuf::from(path), title.into(), AudioFileFormat::Flac);
        t.artist = artist.map(Into::into);
        t.album = album.map(Into::into);
        t
    }

    #[test]
    fn matching_tiers() {
        let tracks = vec![
            lib("/m/Avicii/X/03 - Miami 82 (Avicii).flac", "Miami 82 (Avicii)", Some("Syn Cole"), Some("X")),
            lib("/m/Other/Kiss.flac", "Kiss", Some("Prince"), Some("Kiss")),
        ];
        // Loose file-name match with artist overlap.
        let r = remote("Miami 82 - Avicii", &["Syn Cole"], None);
        assert!(find_existing_match(&r, &tracks).is_some());
        // Same title, different artists: not a duplicate.
        let r = remote("Kiss", &["Tom Jones"], Some("Kiss"));
        assert!(find_existing_match(&r, &tracks).is_none());
        // Nothing to disambiguate on: exact title is enough.
        let r = remote("Kiss", &[], None);
        assert!(find_existing_match(&r, &tracks).is_some());
        // feat. splits into tokens.
        let r = remote("Kiss", &["Prince feat. Someone"], None);
        assert!(find_existing_match(&r, &tracks).is_some());
    }

    #[test]
    fn accent_lifts_the_vibrant_bucket() {
        let mut img = image::RgbImage::new(16, 16);
        for (x, _, p) in img.enumerate_pixels_mut() {
            *p = if x < 12 { image::Rgb([10, 10, 10]) } else { image::Rgb([40, 60, 160]) };
        }
        let mut buf = std::io::Cursor::new(Vec::new());
        img.write_to(&mut buf, image::ImageFormat::Png).unwrap();
        let [r, g, b] = artwork_accent(buf.get_ref()).unwrap();
        assert!(b > r && b > g && b >= 183, "{r} {g} {b}");
        let mut grey = image::RgbImage::new(4, 4);
        grey.pixels_mut().for_each(|p| *p = image::Rgb([128, 128, 128]));
        let mut buf = std::io::Cursor::new(Vec::new());
        grey.write_to(&mut buf, image::ImageFormat::Png).unwrap();
        assert!(artwork_accent(buf.get_ref()).is_none());
    }

    #[test]
    fn leading_numbers_and_paths() {
        assert_eq!(strip_leading_number("03 - Song"), "Song");
        assert_eq!(strip_leading_number("1234 - Song"), "1234 - Song");
        assert_eq!(strip_leading_number("99 Luftballons"), "99 Luftballons");
        let mut r = remote("A/B: C", &["AC/DC"], Some("Live"));
        r.track_number = Some(3);
        let p = final_destination(Path::new("/root"), &r, "flac");
        assert_eq!(p, Path::new("/root").join("AC-DC").join("Live").join("03 - A-B- C.flac"));
        let s = remote("Solo", &[], None);
        assert_eq!(final_destination(Path::new("/r"), &s, "mp3"), Path::new("/r/Unknown Artist/Singles/Solo.mp3"));
    }
}
