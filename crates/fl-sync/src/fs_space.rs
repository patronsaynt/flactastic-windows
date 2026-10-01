//! Free space on the volume holding a path (the Mac's
//! `volumeAvailableCapacityForImportantUsage`): what this user can write.

use std::path::Path;

#[cfg(windows)]
pub fn available_bytes(path: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut avail = 0u64;
    let ok = unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut avail, std::ptr::null_mut(), std::ptr::null_mut()) };
    (ok != 0).then_some(avail)
}

#[cfg(unix)]
pub fn available_bytes(path: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut s) } != 0 {
        return None;
    }
    Some(s.f_bavail as u64 * s.f_frsize as u64)
}

#[cfg(not(any(windows, unix)))]
pub fn available_bytes(_: &Path) -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    #[test]
    fn reports_something_for_the_temp_dir() {
        assert!(super::available_bytes(&std::env::temp_dir()).is_some_and(|n| n > 0));
    }
}
