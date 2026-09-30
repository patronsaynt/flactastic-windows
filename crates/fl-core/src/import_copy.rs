//! `ImportCopy` — filename sanitising, collision-free destinations, copying.

use std::path::{Path, PathBuf};

use crate::text::trim;

/// Windows device names that cannot be used as a file or folder name, with or
/// without an extension.
const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1", "LPT2",
    "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

pub fn is_reserved_windows_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or("").trim_end_matches(' ');
    RESERVED.iter().any(|r| r.eq_ignore_ascii_case(stem))
}

/// Strips characters that can't appear in a path component and returns
/// `fallback` if nothing usable is left.
///
/// Mac rules: drop `/ \ : * ? " < > |`, trim whitespace, prefix `_` to a
/// leading dot. Added for Windows (applied on every OS so plans match):
/// control characters are dropped, trailing dots/spaces trimmed, and
/// reserved device names prefixed with `_`.
pub fn sanitize(name: &str, fallback: &str) -> String {
    const INVALID: [char; 9] = ['/', '\\', ':', '*', '?', '"', '<', '>', '|'];
    let filtered: String = name.chars().filter(|c| !INVALID.contains(c) && !c.is_control()).collect();
    let mut cleaned = trim(&filtered).trim_end_matches(['.', ' ']).to_owned();
    cleaned = trim(&cleaned).to_owned();
    if cleaned.is_empty() {
        return fallback.to_owned();
    }
    if cleaned.starts_with('.') || is_reserved_windows_name(&cleaned) {
        cleaned.insert(0, '_');
    }
    cleaned
}

/// `[artist] - [album title]`.
pub fn album_folder_name(artist: Option<&str>, album: &str) -> String {
    let a = sanitize(artist.map(str::trim).unwrap_or(""), "Unknown Artist");
    let b = sanitize(album.trim(), "Untitled Album");
    format!("{a} - {b}")
}

/// `[artist] - [song title].[ext]`.
pub fn track_file_name(artist: Option<&str>, title: &str, ext: &str) -> String {
    let a = sanitize(artist.map(str::trim).unwrap_or(""), "Unknown Artist");
    let t = sanitize(title.trim(), "Untitled");
    if ext.is_empty() {
        format!("{a} - {t}")
    } else {
        format!("{a} - {t}.{ext}")
    }
}

fn split_ext(filename: &str) -> (&str, &str) {
    match filename.rfind('.') {
        Some(i) if i > 0 => (&filename[..i], &filename[i + 1..]),
        _ => (filename, ""),
    }
}

/// A path in `dir` that doesn't exist yet: `name (1).ext`, `name (2).ext`, …
pub fn unique_destination(filename: &str, dir: &Path) -> PathBuf {
    let (base, ext) = split_ext(filename);
    let mut candidate = dir.join(filename);
    let mut n = 1;
    while candidate.exists() {
        let name = if ext.is_empty() { format!("{base} ({n})") } else { format!("{base} ({n}).{ext}") };
        candidate = dir.join(name);
        n += 1;
    }
    candidate
}

/// Copies `source` into `dir` as `filename`; a no-op when it's already there.
pub fn copy_into(source: &Path, dir: &Path, filename: &str) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let would_be = dir.join(filename);
    if paths_equal(&would_be, source) {
        return Ok(source.to_path_buf());
    }
    let dest = unique_destination(filename, dir);
    std::fs::copy(source, &dest)?;
    Ok(dest)
}

/// Move that falls back to copy + delete across volumes.
pub fn move_file(from: &Path, to: &Path) -> std::io::Result<()> {
    match std::fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(e) if from.exists() && !to.exists() && is_cross_device(&e) => {
            std::fs::copy(from, to)?;
            std::fs::remove_file(from)
        }
        Err(e) => Err(e),
    }
}

fn is_cross_device(e: &std::io::Error) -> bool {
    // EXDEV on Unix, ERROR_NOT_SAME_DEVICE (17) on Windows.
    matches!(e.raw_os_error(), Some(18) if cfg!(unix)) || matches!(e.raw_os_error(), Some(17) if cfg!(windows))
}

fn paths_equal(a: &Path, b: &Path) -> bool {
    match (canonical(a), canonical(b)) {
        (Some(x), Some(y)) => x == y,
        _ => a == b,
    }
}

fn canonical(p: &Path) -> Option<PathBuf> {
    std::fs::canonicalize(p).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_rules() {
        assert_eq!(sanitize("AC/DC: Live?", "x"), "ACDC Live");
        assert_eq!(sanitize("  ", "Unknown"), "Unknown");
        assert_eq!(sanitize(".hidden", "x"), "_.hidden");
        assert_eq!(sanitize("CON", "x"), "_CON");
        assert_eq!(sanitize("nul.txt", "x"), "_nul.txt");
        assert_eq!(sanitize("Console", "x"), "Console");
        assert_eq!(sanitize("Mr. ", "x"), "Mr");
        assert_eq!(sanitize("tab\there", "x"), "tabhere");
    }

    #[test]
    fn unique_names() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.flac"), b"").unwrap();
        std::fs::write(dir.path().join("a (1).flac"), b"").unwrap();
        assert_eq!(unique_destination("a.flac", dir.path()), dir.path().join("a (2).flac"));
        assert_eq!(unique_destination("b.flac", dir.path()), dir.path().join("b.flac"));
    }
}
