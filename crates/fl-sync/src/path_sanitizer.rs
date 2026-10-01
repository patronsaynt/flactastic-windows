//! `PathSanitizer`: turns a peer's untrusted relative path into one that is
//! guaranteed to land inside our library root. Structural attacks are
//! rejected (never repaired), Unicode is folded to NFC before any check, each
//! component goes through the import sanitiser, and the result is re-checked
//! against the root after resolving existing (possibly symlinked) ancestors.

use std::path::{Component, Path, PathBuf};

use fl_core::import_copy::sanitize;
use fl_core::AudioFileFormat;
use unicode_normalization::UnicodeNormalization;

/// 255 UTF-8 bytes per component (APFS/HFS+; NTFS allows 255 UTF-16 units).
pub const MAX_COMPONENT_BYTES: usize = 255;
pub const MAX_DEPTH: usize = 32;
pub const MAX_TOTAL_BYTES: usize = 3072;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Rejection {
    #[error("Path is empty.")]
    Empty,
    #[error("Path contains a null byte.")]
    NullByte,
    #[error("Path is absolute.")]
    AbsolutePath,
    #[error("Path contains a parent-directory reference.")]
    ParentTraversal,
    #[error("Path references a home directory.")]
    HomeReference,
    #[error("Path contains a current-directory reference.")]
    CurrentDirectoryComponent,
    #[error("Path contains an empty component.")]
    EmptyComponent,
    #[error("Path component is too long: {0}")]
    ComponentTooLong(String),
    #[error("Path is {0} levels deep.")]
    TooDeep(usize),
    #[error("Path is {0} bytes long.")]
    TooLong(usize),
    #[error("Unsupported file type: .{0}")]
    UnsupportedFormat(String),
    #[error("Path resolves outside the library: {0}")]
    EscapesRoot(String),
}

/// Validates and normalises an untrusted relative path: sanitised components
/// joined with `/`, no leading or trailing separator.
pub fn sanitize_relative_path(raw: &str) -> Result<String, Rejection> {
    if raw.is_empty() {
        return Err(Rejection::Empty);
    }
    if raw.as_bytes().contains(&0) {
        return Err(Rejection::NullByte);
    }
    if raw.len() > MAX_TOTAL_BYTES {
        return Err(Rejection::TooLong(raw.len()));
    }
    let normalised: String = raw.nfc().collect();
    if normalised.starts_with('/') || normalised.starts_with('\\') {
        return Err(Rejection::AbsolutePath);
    }
    if normalised.starts_with('~') {
        return Err(Rejection::HomeReference);
    }
    // A drive letter from a Windows-shaped path.
    let mut chars = normalised.chars();
    if let (Some(c0), Some(':')) = (chars.next(), chars.next()) {
        if c0.is_alphabetic() {
            return Err(Rejection::AbsolutePath);
        }
    }

    // Both separators split: the worst case is rejecting a path a peer could
    // have sent.
    let raw_components: Vec<&str> = normalised.split(['/', '\\']).collect();
    let mut safe = Vec::new();
    for (i, c) in raw_components.iter().enumerate() {
        if c.is_empty() {
            if i == raw_components.len() - 1 {
                continue;
            }
            return Err(Rejection::EmptyComponent);
        }
        if *c == "." {
            return Err(Rejection::CurrentDirectoryComponent);
        }
        if *c == ".." {
            return Err(Rejection::ParentTraversal);
        }
        if c.starts_with('~') {
            return Err(Rejection::HomeReference);
        }
        // Dot-only components are refused before sanitising, which would
        // otherwise disguise "..." as "_...".
        if !c.chars().any(|ch| ch != '.') {
            return Err(Rejection::ParentTraversal);
        }
        if c.len() > MAX_COMPONENT_BYTES {
            return Err(Rejection::ComponentTooLong((*c).to_owned()));
        }
        let cleaned = sanitize(c, "Unknown");
        if cleaned.is_empty() {
            return Err(Rejection::EmptyComponent);
        }
        safe.push(cleaned);
    }
    if safe.is_empty() {
        return Err(Rejection::Empty);
    }
    if safe.len() > MAX_DEPTH {
        return Err(Rejection::TooDeep(safe.len()));
    }
    Ok(safe.join("/"))
}

/// As `sanitize_relative_path`, plus the extension must be a known audio
/// format, so a peer can't drop scripts or libraries into the library.
pub fn sanitize_audio_relative_path(raw: &str) -> Result<String, Rejection> {
    let path = sanitize_relative_path(raw)?;
    let file = path.rsplit('/').next().unwrap_or(&path);
    let ext = match file.rfind('.') {
        Some(i) if i > 0 => &file[i + 1..],
        _ => "",
    };
    if AudioFileFormat::classify_ext(ext).is_none() {
        return Err(Rejection::UnsupportedFormat(ext.to_owned()));
    }
    Ok(path)
}

fn canonical(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

/// True when `path` is `root` or beneath it, compared by component.
pub fn is_contained(path: &Path, root: &Path) -> bool {
    fn norm(p: &Path) -> Vec<Component<'_>> {
        p.components().filter(|c| !matches!(c, Component::CurDir)).collect()
    }
    let (p, r) = (norm(path), norm(root));
    p.len() >= r.len() && p[..r.len()] == r[..]
}

/// Resolves a sanitised path under `root`, proving the result is still inside
/// it after each existing ancestor's symlinks are followed. Components we
/// create ourselves are plain directories.
pub fn resolved_destination(sanitized: &str, root: &Path) -> Result<PathBuf, Rejection> {
    let canonical_root = canonical(root);
    let mut current = canonical_root.clone();
    for comp in sanitized.split('/') {
        current = current.join(comp);
        if std::fs::symlink_metadata(&current).is_ok() {
            let resolved = canonical(&current);
            if !is_contained(&resolved, &canonical_root) {
                return Err(Rejection::EscapesRoot(current.display().to_string()));
            }
            current = resolved;
        }
    }
    if !is_contained(&current, &canonical_root) {
        return Err(Rejection::EscapesRoot(current.display().to_string()));
    }
    Ok(current)
}

pub fn destination_for_incoming_track(remote_relative_path: &str, root: &Path) -> Result<PathBuf, Rejection> {
    let safe = sanitize_audio_relative_path(remote_relative_path)?;
    resolved_destination(&safe, root)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rejects(p: &str) -> bool {
        sanitize_relative_path(p).is_err()
    }

    #[test]
    fn traversal_absolute_and_malformed() {
        for a in ["../secrets.flac", "Music/../../../.zshrc", "Artist/Album/../../../../etc/passwd", "..", "../", "a/../b.flac"] {
            assert!(rejects(a), "{a}");
        }
        for a in ["....//x.flac", ".../x.flac", "a/..../b.flac", "./x.flac"] {
            assert!(rejects(a), "{a}");
        }
        for a in [
            "/etc/passwd",
            "/Users/someone/Library/x.flac",
            "~/Library/LaunchAgents/evil.plist",
            "~root/x.flac",
            "\\\\server\\share\\x.flac",
            "C:\\Windows\\System32\\x.flac",
            "a/~/b.flac",
        ] {
            assert!(rejects(a), "{a}");
        }
        assert!(rejects("Artist/song.flac\0.txt"));
        for a in ["", "//x.flac", "a//b.flac"] {
            assert!(rejects(a), "{a:?}");
        }
        let decomposed: String = "Arti\u{0301}st/..".nfc().collect();
        assert!(rejects(&decomposed));
    }

    #[test]
    fn limits() {
        let long = "a".repeat(MAX_COMPONENT_BYTES + 1);
        assert!(rejects(&format!("Artist/{long}.flac")));
        let deep = vec!["d"; MAX_DEPTH + 1].join("/");
        assert!(rejects(&format!("{deep}/x.flac")));
        assert!(rejects(&format!("{}x.flac", "a/".repeat(MAX_TOTAL_BYTES))));
    }

    #[test]
    fn acceptance() {
        let p = "Boards of Canada/Geogaddi/01 Ready Lets Go.flac";
        assert_eq!(sanitize_relative_path(p).unwrap(), p);
        assert_eq!(sanitize_relative_path("song.flac").unwrap(), "song.flac");
        assert_eq!(sanitize_relative_path("Artist/Album/").unwrap(), "Artist/Album");
        let r = sanitize_relative_path("AC:DC/Album?/song*.flac").unwrap();
        assert!(!r.contains([':', '?', '*']) && r.ends_with(".flac"));
        assert!(sanitize_relative_path(".hidden.flac").unwrap().starts_with('_'));
        // Windows device names can't be created as files.
        assert_eq!(sanitize_relative_path("CON/x.flac").unwrap(), "_CON/x.flac");
    }

    #[test]
    fn audio_extension_gate() {
        for a in ["Artist/evil.dylib", "evil.plist", "Artist/Album/run.sh", "noextension", "song.flac.txt"] {
            assert!(sanitize_audio_relative_path(a).is_err(), "{a}");
        }
        for ext in ["flac", "mp3", "wav", "wave", "aif", "aiff", "m4a", "aac", "FLAC", "Mp3"] {
            assert!(sanitize_audio_relative_path(&format!("Artist/song.{ext}")).unwrap().ends_with(ext));
        }
    }

    #[test]
    fn containment_is_componentwise() {
        let root = Path::new("/Users/x/Library");
        assert!(is_contained(Path::new("/Users/x/Library/a.flac"), root));
        assert!(is_contained(root, root));
        assert!(!is_contained(Path::new("/Users/x/Library Backup/a.flac"), root));
        assert!(!is_contained(Path::new("/Users/x"), root));
    }

    #[test]
    fn resolution() {
        let base = tempfile::tempdir().unwrap();
        let root = base.path().join("library");
        let outside = base.path().join("outside");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let d = destination_for_incoming_track("Artist/Album/01 Song.flac", &root).unwrap();
        assert!(is_contained(&d, &canonical(&root)));
        assert_eq!(d.file_name().unwrap(), "01 Song.flac");

        #[cfg(unix)]
        let linked = std::os::unix::fs::symlink(&outside, root.join("Music")).is_ok();
        // Symlinks need Developer Mode (or admin) on Windows; a junction
        // escapes the same way and needs neither.
        #[cfg(windows)]
        let linked = std::os::windows::fs::symlink_dir(&outside, root.join("Music")).is_ok()
            || std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(root.join("Music"))
                .arg(&outside)
                .output()
                .is_ok_and(|o| o.status.success());
        assert!(linked || cfg!(not(any(unix, windows))));
        if linked {
            let safe = sanitize_audio_relative_path("Music/song.flac").unwrap();
            assert!(matches!(resolved_destination(&safe, &root), Err(Rejection::EscapesRoot(_))));
        }
    }
}
