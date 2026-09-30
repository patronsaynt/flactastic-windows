//! `VisualizerBackdropCache`: the Lyrics mode's ground, baked once per
//! (image, tint): downsample to 480 px, gaussian blur, desaturate, darken,
//! raise contrast, then tint toward the album's dominant colour.

use std::sync::Arc;

use fl_core::model::artwork_content_id;
use fl_core::ArtistResolver;
use image::{imageops, DynamicImage, RgbImage};
use serde::Serialize;
use tauri::State;

use crate::state::AppState;

type St<'a> = State<'a, Arc<AppState>>;

const WORKING_WIDTH: f32 = 480.0;
const SIGMA: f32 = 20.0;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Backdrop {
    /// Stable identity: the artist when they have a photo (so consecutive
    /// tracks by them don't re-fade), else the track.
    key: String,
    /// Artwork-store id of the baked JPEG, if there was any image at all.
    image: Option<String>,
}

/// CIColorControls(saturation 0, brightness −0.33, contrast 1.15) then
/// CIColorMonochrome(intensity 0.8) toward `tint`.
pub fn bake(data: &[u8], tint: Option<[u8; 3]>) -> Option<Vec<u8>> {
    let src = image::load_from_memory(data).ok()?;
    let scale = (WORKING_WIDTH / src.width() as f32).min(1.0);
    let (w, h) = (((src.width() as f32) * scale).round().max(1.0) as u32, ((src.height() as f32) * scale).round().max(1.0) as u32);
    let small = src.resize_exact(w, h, imageops::FilterType::Triangle).to_rgb8();
    // The sigma is relative to the working width, so small sources blur
    // proportionally; `blur` clamps at the edges like `clampedToExtent`.
    let blurred = imageops::blur(&small, SIGMA * (w as f32 / WORKING_WIDTH));

    let tint = tint.map(|[r, g, b]| [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0]);
    let mut out = RgbImage::new(w, h);
    for (x, y, p) in blurred.enumerate_pixels() {
        let [r, g, b] = p.0.map(|c| c as f32 / 255.0);
        // Rec. 709 luma: saturation 0.
        let l = 0.2126 * r + 0.7152 * g + 0.0722 * b;
        // Brightness then contrast about mid-grey.
        let l = (((l - 0.33) - 0.5) * 1.15 + 0.5).clamp(0.0, 1.0);
        let px = match tint {
            // Monochrome: luminance carried by the tint colour, mixed 80%.
            Some([tr, tg, tb]) => [l * 0.2 + l * tr * 0.8, l * 0.2 + l * tg * 0.8, l * 0.2 + l * tb * 0.8],
            None => [l, l, l],
        };
        out.put_pixel(x, y, image::Rgb(px.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8)));
    }
    let mut bytes = Vec::new();
    let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(std::io::Cursor::new(&mut bytes), 90);
    DynamicImage::ImageRgb8(out).write_with_encoder(enc).ok()?;
    Some(bytes)
}

#[tauri::command]
pub async fn visualizer_backdrop(st: St<'_>, track_id: String) -> Result<Backdrop, String> {
    let st = st.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let Some(track) = st.tracks_by_id(&[track_id.clone()]).into_iter().next() else {
            return Ok(Backdrop { key: "empty".into(), image: None });
        };
        let track_key = format!("track:{track_id}");
        let credit = track.artist.clone().or_else(|| track.album_artist.clone());
        let artist_key = credit.as_deref().and_then(|c| {
            let tracks = st.library.read().tracks();
            ArtistResolver::new(&tracks).keys_for_credit(Some(c)).into_iter().next()
        });
        let photo = artist_key.as_deref().and_then(|k| {
            let store = st.artists.store.lock();
            let remote = st.artists.remote.lock();
            store.resolved_profile_image(k, &remote).map(<[u8]>::to_vec)
        });
        let key = match (&artist_key, &photo) {
            (Some(k), Some(_)) => format!("artist:{k}|{track_key}"),
            _ => track_key,
        };
        // Photo, else the album art so the screen is never empty.
        let Some(source) = photo.or_else(|| track.artwork.as_ref().map(|a| a.to_vec())) else {
            return Ok(Backdrop { key, image: None });
        };
        let tint = track.artwork.as_deref().and_then(crate::artists::dominant_color);
        let cache_id = format!(
            "x:backdrop:{}#{}",
            artwork_content_id(&source),
            tint.map_or("none".into(), |[r, g, b]| format!("{r:02x}{g:02x}{b:02x}"))
        );
        if st.artwork.original(&cache_id).is_none() {
            let baked = bake(&source, tint).ok_or("couldn't render the backdrop")?;
            st.artwork.insert_raw(&cache_id, baked);
        }
        Ok(Backdrop { key, image: Some(cache_id) })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bake_produces_a_small_tinted_jpeg() {
        let img = RgbImage::from_fn(1200, 800, |x, _| if x < 600 { image::Rgb([240, 240, 240]) } else { image::Rgb([20, 20, 20]) });
        let mut png = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let out = image::load_from_memory(&bake(&png, Some([255, 0, 0])).unwrap()).unwrap().to_rgb8();
        assert_eq!((out.width(), out.height()), (480, 320));
        // Bright side: red-dominant after the tint; dark side stays dark.
        let [r, g, b] = out.get_pixel(40, 160).0;
        assert!(r > g + 40 && r > b + 40, "{r} {g} {b}");
        assert!(out.get_pixel(440, 160).0[0] < 40);
    }
}
