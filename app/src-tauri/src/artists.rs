//! Artists: `ArtistStore` overrides, the Deezer `ArtistRemoteCache`, and
//! `ArtistImageFetcher` (lazy, deduplicated, four requests at a time).

use std::collections::HashSet;
use std::sync::Arc;

use crossbeam_channel::{unbounded, Sender};
use fl_core::apple_json::AppleDate;
use fl_core::library::all_artists;
use fl_core::stores::{ArtistOverride, ArtistRemoteCache, ArtistRemoteEntry, ArtistStore};
use fl_core::{Album, ArtistResolver, ArtistSummary, Track};
use parking_lot::Mutex;
use serde::Serialize;

use crate::artwork::ArtworkStore;

/// `DeezerClient`'s polite concurrency cap.
const FETCH_WORKERS: usize = 4;

pub struct Artists {
    pub store: Mutex<ArtistStore>,
    pub remote: Mutex<ArtistRemoteCache>,
    in_flight: Mutex<HashSet<String>>,
    tx: Sender<(String, String)>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtistDto {
    pub id: String,
    pub display_name: String,
    pub album_ids: Vec<String>,
    pub single_ids: Vec<String>,
    pub appears_on_ids: Vec<String>,
    pub track_count: usize,
    /// First album cover seen for the artist.
    pub artwork_sample: Option<String>,
    /// Grid image: override profile, else override banner, else Deezer.
    pub image: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtistDetailDto {
    pub artist: ArtistDto,
    /// Override banner, else Deezer picture, else the artwork sample.
    pub banner: Option<String>,
    /// Only a user-supplied banner is shown unblurred.
    pub banner_is_true: bool,
    /// 8×8 average of the banner, for the gradient (`dominantColor`).
    pub base_color: Option<[u8; 3]>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtistOverrideDto {
    pub display_name: Option<String>,
    pub banner: Option<String>,
    pub profile: Option<String>,
}

impl Artists {
    /// Loads both stores and starts the fetch workers. `on_image` runs after
    /// every completed fetch (hit or miss).
    pub fn new(app_data: &std::path::Path, on_image: Arc<dyn Fn() + Send + Sync>) -> Arc<Artists> {
        let mut store = ArtistStore::new(app_data);
        store.load();
        let mut remote = ArtistRemoteCache::new(app_data);
        remote.load();
        let (tx, rx) = unbounded::<(String, String)>();
        let me = Arc::new(Artists {
            store: Mutex::new(store),
            remote: Mutex::new(remote),
            in_flight: Mutex::default(),
            tx,
        });
        for i in 0..FETCH_WORKERS {
            let rx = rx.clone();
            let weak = Arc::downgrade(&me);
            let on_image = on_image.clone();
            std::thread::Builder::new()
                .name(format!("fl-artist-image-{i}"))
                .spawn(move || {
                    while let Ok((key, name)) = rx.recv() {
                        let Some(me) = weak.upgrade() else { return };
                        me.fetch(&key, &name);
                        me.in_flight.lock().remove(&key);
                        on_image();
                    }
                })
                .ok();
        }
        me
    }

    /// `ensureImage`: no-op when there's an override picture, a fresh cache
    /// entry, or a request already in flight.
    pub fn ensure_image(&self, key: &str, display_name: &str) {
        if self.store.lock().get(key).is_some_and(|o| o.profile_image.is_some()) {
            return;
        }
        if self.remote.lock().is_fresh(key, AppleDate::now()) {
            return;
        }
        if !self.in_flight.lock().insert(key.to_owned()) {
            return;
        }
        let _ = self.tx.send((key.to_owned(), display_name.to_owned()));
    }

    fn fetch(&self, key: &str, display_name: &str) {
        let entry = |deezer_id, profile_image, not_found| ArtistRemoteEntry {
            canonical_key: key.to_owned(),
            deezer_id,
            profile_image,
            fetched_at: AppleDate::now(),
            not_found,
        };
        let result = match fl_net::deezer::search_artist(display_name) {
            Ok(None) => Ok(entry(None, None, true)),
            Ok(Some(a)) => match a.picture_url.as_deref() {
                None => Ok(entry(Some(a.id), None, true)),
                Some(url) => fl_net::deezer::download_image(url).map(|d| entry(Some(a.id), Some(d), false)),
            },
            Err(e) => Err(e),
        };
        match result {
            Ok(e) => self.remote.lock().set(e),
            // Transient failure: leave no entry so a later visit retries.
            Err(e) => log::info!("[ArtistImageFetcher] {display_name}: {e}"),
        }
    }

    pub fn summaries(&self, tracks: &[Track], albums: &[Album]) -> Vec<ArtistSummary> {
        let resolver = ArtistResolver::new(tracks);
        let overrides = self.store.lock().display_overrides();
        all_artists(albums, &resolver, &overrides)
    }

    fn dto(&self, s: &ArtistSummary, art: &ArtworkStore) -> ArtistDto {
        let image = {
            let store = self.store.lock();
            let o = store.get(&s.id);
            match o.and_then(|o| o.profile_image.as_ref().or(o.banner_image.as_ref())) {
                Some(d) => Some(art.register_bytes(&format!("artist-override:{}", s.id), d.clone())),
                None => self
                    .remote
                    .lock()
                    .entries
                    .get(&s.id)
                    .and_then(|e| e.profile_image.clone())
                    .map(|d| art.register_bytes(&format!("artist-remote:{}", s.id), d)),
            }
        };
        let ids = |v: &[Album]| v.iter().map(|a| a.id.clone()).collect();
        ArtistDto {
            id: s.id.clone(),
            display_name: s.display_name.clone(),
            album_ids: ids(&s.albums),
            single_ids: ids(&s.singles),
            appears_on_ids: ids(&s.appears_on),
            track_count: s.track_count,
            artwork_sample: s.artwork_sample.as_ref().map(|a| art.register(a)),
            image,
        }
    }

    pub fn list(&self, tracks: &[Track], albums: &[Album], art: &ArtworkStore) -> Vec<ArtistDto> {
        self.summaries(tracks, albums).iter().map(|s| self.dto(s, art)).collect()
    }

    pub fn detail(&self, key: &str, tracks: &[Track], albums: &[Album], art: &ArtworkStore) -> Option<ArtistDetailDto> {
        let s = self.summaries(tracks, albums).into_iter().find(|s| s.id == key)?;
        let artist = self.dto(&s, art);
        let true_banner = self.store.lock().get(key).and_then(|o| o.banner_image.clone());
        let remote = self.remote.lock().entries.get(key).and_then(|e| e.profile_image.clone());
        let (banner_bytes, tag): (Option<Vec<u8>>, &str) = match (&true_banner, remote) {
            (Some(b), _) => (Some(b.clone()), "artist-banner"),
            (None, Some(r)) => (Some(r), "artist-remote"),
            (None, None) => (s.artwork_sample.as_ref().map(|a| a.to_vec()), ""),
        };
        let base_color = banner_bytes.as_deref().and_then(dominant_color);
        let banner = match (banner_bytes, tag) {
            (Some(b), t) if !t.is_empty() => Some(art.register_bytes(&format!("{t}:{key}"), b)),
            _ => artist.artwork_sample.clone(),
        };
        Some(ArtistDetailDto { artist, banner, banner_is_true: true_banner.is_some(), base_color })
    }

    pub fn override_dto(&self, key: &str, art: &ArtworkStore) -> ArtistOverrideDto {
        let store = self.store.lock();
        let o = store.get(key);
        ArtistOverrideDto {
            display_name: o.and_then(|o| o.display_name.clone()),
            banner: o
                .and_then(|o| o.banner_image.clone())
                .map(|b| art.register_bytes(&format!("artist-banner:{key}"), b)),
            profile: o
                .and_then(|o| o.profile_image.clone())
                .map(|b| art.register_bytes(&format!("artist-override:{key}"), b)),
        }
    }

    /// `ArtistEditorView.save`: an empty override is removed by `upsert`.
    pub fn save_override(&self, key: &str, display_name: Option<String>, banner: Option<Vec<u8>>, profile: Option<Vec<u8>>) {
        let name = display_name.map(|s| s.trim().to_owned()).filter(|s| !s.is_empty());
        self.store.lock().upsert(ArtistOverride {
            canonical_key: key.to_owned(),
            display_name: name,
            banner_image: banner,
            profile_image: profile,
        });
    }

    pub fn reset_override(&self, key: &str) {
        self.store.lock().remove(key);
    }
}

/// `ArtistDetailView.dominantColor`: the image drawn at 8×8, averaged.
pub fn dominant_color(data: &[u8]) -> Option<[u8; 3]> {
    let img = image::load_from_memory(data).ok()?;
    let small = img.resize_exact(8, 8, image::imageops::FilterType::Triangle).to_rgb8();
    let (mut r, mut g, mut b) = (0u32, 0u32, 0u32);
    for p in small.pixels() {
        r += u32::from(p[0]);
        g += u32::from(p[1]);
        b += u32::from(p[2]);
    }
    Some([(r / 64) as u8, (g / 64) as u8, (b / 64) as u8])
}

/// `ImageCropperView.commit`: crop `(x, y, w, h)` source pixels, scale to
/// `out_w × out_h` with high-quality filtering, encode PNG.
pub fn crop_png(data: &[u8], rect: [f64; 4], out_w: u32, out_h: u32) -> Option<Vec<u8>> {
    let img = image::load_from_memory(data).ok()?;
    let (iw, ih) = (img.width() as f64, img.height() as f64);
    // `CGRect.integral`, clamped to the image.
    let x0 = rect[0].floor().clamp(0.0, iw);
    let y0 = rect[1].floor().clamp(0.0, ih);
    let x1 = (rect[0] + rect[2]).ceil().clamp(x0 + 1.0, iw.max(x0 + 1.0));
    let y1 = (rect[1] + rect[3]).ceil().clamp(y0 + 1.0, ih.max(y0 + 1.0));
    let cropped = img.crop_imm(x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32);
    let out = cropped.resize_exact(out_w.max(1), out_h.max(1), image::imageops::FilterType::Lanczos3);
    let mut bytes = Vec::new();
    out.to_rgba8()
        .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
        .ok()?;
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 3]) -> Vec<u8> {
        let img = image::RgbImage::from_fn(w, h, |x, y| image::Rgb(f(x, y)));
        let mut out = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).unwrap();
        out
    }

    #[test]
    fn dominant_color_averages() {
        let d = png(16, 16, |x, _| if x < 8 { [200, 0, 0] } else { [0, 0, 200] });
        let [r, g, b] = dominant_color(&d).unwrap();
        assert!((90..=110).contains(&r) && g == 0 && (90..=110).contains(&b), "{r} {g} {b}");
        assert!(dominant_color(b"nope").is_none());
    }

    #[test]
    fn crop_takes_the_requested_region() {
        // Left half red, right half green; crop the right half.
        let d = png(100, 50, |x, _| if x < 50 { [255, 0, 0] } else { [0, 255, 0] });
        let out = crop_png(&d, [50.0, 0.0, 50.0, 50.0], 30, 10).unwrap();
        let img = image::load_from_memory(&out).unwrap().to_rgb8();
        assert_eq!((img.width(), img.height()), (30, 10));
        assert_eq!(img.get_pixel(15, 5).0, [0, 255, 0]);
    }
}
