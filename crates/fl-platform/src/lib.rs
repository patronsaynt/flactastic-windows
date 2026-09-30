//! OS integration: file identity, app directories, trash, secrets.

pub mod identity;

use std::path::PathBuf;

pub use identity::NativeFileIdentity;

/// Per-user directories.
///
/// | | Windows | Linux |
/// |---|---|---|
/// | config (settings.json) | `%APPDATA%\FLACtastic` | `$XDG_CONFIG_HOME/flactastic` |
/// | data (artists.json, caches the Mac keeps in Application Support) | `%APPDATA%\FLACtastic` | `$XDG_DATA_HOME/flactastic` |
/// | cache (thumbnails, lyrics backdrops) | `%LOCALAPPDATA%\FLACtastic\Cache` | `$XDG_CACHE_HOME/flactastic` |
#[derive(Debug, Clone)]
pub struct AppDirs {
    pub config: PathBuf,
    pub data: PathBuf,
    pub cache: PathBuf,
}

impl AppDirs {
    pub fn resolve() -> std::io::Result<AppDirs> {
        let missing = || std::io::Error::other("no home directory");
        let dirs = if cfg!(windows) {
            let roaming = dirs::config_dir().ok_or_else(missing)?.join("FLACtastic");
            let local = dirs::cache_dir().ok_or_else(missing)?.join("FLACtastic").join("Cache");
            AppDirs { config: roaming.clone(), data: roaming, cache: local }
        } else {
            AppDirs {
                config: dirs::config_dir().ok_or_else(missing)?.join("flactastic"),
                data: dirs::data_dir().ok_or_else(missing)?.join("flactastic"),
                cache: dirs::cache_dir().ok_or_else(missing)?.join("flactastic"),
            }
        };
        for d in [&dirs.config, &dirs.data, &dirs.cache] {
            std::fs::create_dir_all(d)?;
        }
        Ok(dirs)
    }
}

/// Moves a file to the Recycle Bin / freedesktop trash.
pub fn move_to_trash(path: &std::path::Path) -> Result<(), String> {
    trash::delete(path).map_err(|e| e.to_string())
}

/// Secrets in Windows Credential Manager / Secret Service. There is no
/// plaintext fallback: when the store is unavailable, callers fail loudly.
pub mod secrets {
    const SERVICE: &str = "FLACtastic";

    fn entry(account: &str) -> Result<keyring::Entry, String> {
        keyring::Entry::new(SERVICE, account).map_err(|e| e.to_string())
    }

    pub fn get(account: &str) -> Result<Option<Vec<u8>>, String> {
        match entry(account)?.get_secret() {
            Ok(v) => Ok(Some(v)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    pub fn set(account: &str, secret: &[u8]) -> Result<(), String> {
        entry(account)?.set_secret(secret).map_err(|e| e.to_string())
    }

    pub fn delete(account: &str) -> Result<(), String> {
        match entry(account)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}
