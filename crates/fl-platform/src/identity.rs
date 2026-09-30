//! Per-file track identity.
//!
//! - Windows: NTFS alternate data stream `file:com.flactastic.trackID`, the
//!   same name macOS writes when it stores the xattr on an SMB share.
//! - Linux: xattr `user.com.flactastic.trackID` (Linux only allows the
//!   `user.` namespace for regular files).
//! - macOS (dev/testing builds only): xattr `com.flactastic.trackID`.
//!
//! Writing an ADS bumps the file's LastWriteTime on NTFS; the original
//! modification time is restored so the metadata cache and sync hashes stay
//! valid, matching the Mac where setting an xattr leaves mtime alone.

use std::path::Path;

use fl_core::track_ids::{FileIdentity, XATTR_NAME};
use fl_core::Uid;

pub struct NativeFileIdentity;

fn parse(bytes: &[u8]) -> Option<Uid> {
    if bytes.len() != 36 {
        return None;
    }
    Uid::parse(std::str::from_utf8(bytes).ok()?)
}

#[cfg(windows)]
mod imp {
    use super::*;
    use std::io::{Read, Write};

    fn stream_path(p: &Path) -> std::path::PathBuf {
        let mut s = p.as_os_str().to_owned();
        s.push(":");
        s.push(XATTR_NAME);
        s.into()
    }

    pub fn read(p: &Path) -> Option<Uid> {
        let f = std::fs::File::open(stream_path(p)).ok()?;
        let mut buf = Vec::with_capacity(37);
        f.take(37).read_to_end(&mut buf).ok()?;
        parse(&buf)
    }

    pub fn write(p: &Path, id: Uid) -> std::io::Result<()> {
        let modified = std::fs::metadata(p)?.modified().ok();
        {
            let mut f = std::fs::File::create(stream_path(p))?;
            f.write_all(id.uuid_string().as_bytes())?;
        }
        if let Some(m) = modified {
            let f = std::fs::OpenOptions::new().write(true).open(p)?;
            f.set_modified(m)?;
        }
        Ok(())
    }
}

#[cfg(unix)]
mod imp {
    use super::*;

    fn name() -> String {
        if cfg!(target_os = "macos") {
            XATTR_NAME.to_owned()
        } else {
            format!("user.{XATTR_NAME}")
        }
    }

    pub fn read(p: &Path) -> Option<Uid> {
        parse(&xattr::get(p, name()).ok()??)
    }

    pub fn write(p: &Path, id: Uid) -> std::io::Result<()> {
        xattr::set(p, name(), id.uuid_string().as_bytes())
    }
}

impl FileIdentity for NativeFileIdentity {
    fn read(&self, path: &Path) -> Option<Uid> {
        imp::read(path)
    }

    fn write(&self, path: &Path, id: Uid) {
        if let Err(e) = imp::write(path, id) {
            log::debug!("[TrackIDStore] could not store identity on {}: {e}", path.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_preserves_mtime() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("track ünï.flac");
        std::fs::write(&p, b"audio").unwrap();
        let before = std::fs::metadata(&p).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));

        let ident = NativeFileIdentity;
        assert_eq!(ident.read(&p), None);
        let id = Uid::new_v4();
        ident.write(&p, id);
        assert_eq!(ident.read(&p), Some(id));
        assert_eq!(std::fs::read(&p).unwrap(), b"audio", "main stream untouched");
        #[cfg(windows)]
        assert_eq!(std::fs::metadata(&p).unwrap().modified().unwrap(), before);
        let _ = before;
    }
}
