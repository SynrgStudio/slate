//! Atomic replacement and optimistic external-edit protection. Never truncate first.
use sha2::{Digest as _, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
pub(crate) type Fingerprint = [u8; 32];
pub(crate) fn fingerprint(bytes: &[u8]) -> Fingerprint {
    Sha256::digest(bytes).into()
}
pub(crate) fn disk_fingerprint(path: &Path) -> Result<Option<Fingerprint>, String> {
    match File::open(path) {
        Ok(mut file) => {
            let mut hash = Sha256::new();
            let mut block = [0u8; 65536];
            loop {
                let n = file.read(&mut block).map_err(|e| e.to_string())?;
                if n == 0 {
                    break;
                }
                hash.update(&block[..n]);
            }
            Ok(Some(hash.finalize().into()))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}
#[derive(Debug)]
pub(crate) enum SaveError {
    Conflict,
    Io(String),
}
impl std::fmt::Display for SaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Conflict => write!(f, "file changed externally or destination already exists"),
            Self::Io(e) => write!(f, "{e}"),
        }
    }
}
pub(crate) fn checked_save(
    path: &Path,
    bytes: &[u8],
    expected: Option<Fingerprint>,
) -> Result<Fingerprint, SaveError> {
    checked_save_staged(path, bytes, expected, || {})
}
fn checked_save_staged(
    path: &Path,
    bytes: &[u8],
    expected: Option<Fingerprint>,
    after_prepare: impl FnOnce(),
) -> Result<Fingerprint, SaveError> {
    reject_special_file(path).map_err(SaveError::Io)?;
    if disk_fingerprint(path).map_err(SaveError::Io)? != expected {
        return Err(SaveError::Conflict);
    }
    let temp = prepare(path, bytes).map_err(SaveError::Io)?;
    after_prepare();
    // Check again after staging: a slow disk write must not silently replace a newer file.
    reject_special_file(path).map_err(SaveError::Io)?;
    if disk_fingerprint(path).map_err(SaveError::Io)? != expected {
        return Err(SaveError::Conflict);
    }
    install(&temp.0, path, expected.is_some()).map_err(SaveError::Io)?;
    sync_parent(path);
    Ok(fingerprint(bytes))
}
pub(crate) fn atomic_replace(path: &Path, bytes: &[u8]) -> Result<(), String> {
    reject_special_file(path)?;
    let exists = path.try_exists().map_err(|e| e.to_string())?;
    let temp = prepare(path, bytes)?;
    install(&temp.0, path, exists)?;
    sync_parent(path);
    Ok(())
}
struct Temp(PathBuf);
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
static NEXT: AtomicU64 = AtomicU64::new(1);
fn prepare(path: &Path, bytes: &[u8]) -> Result<Temp, String> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let name = path.file_name().ok_or("destination has no file name")?;
    let temp = loop {
        let mut name = name.to_os_string();
        name.push(format!(
            ".slate-{}-{}.tmp",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let candidate = parent.join(name);
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&candidate) {
            Ok(mut file) => {
                let temp = Temp(candidate);
                if let Ok(metadata) = fs::metadata(path) {
                    file.set_permissions(metadata.permissions())
                        .map_err(|e| e.to_string())?;
                }
                file.write_all(bytes)
                    .and_then(|_| file.sync_all())
                    .map_err(|e| e.to_string())?;
                break temp;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.to_string()),
        }
    };
    Ok(temp)
}
fn reject_special_file(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(meta) => {
            if !meta.is_file() || meta.file_type().is_symlink() {
                return Err("Not a regular file; use Save As to a new file".into());
            }
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if meta.file_attributes() & 0x400 != 0 {
                    return Err("Reparse-point file: use Save As".into());
                }
            }
            if meta.permissions().readonly() {
                return Err("Read-only file: use Save As".into());
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                if meta.nlink() > 1 {
                    return Err("Hard-linked file: use Save As".into());
                }
            }
            #[cfg(windows)]
            if windows::links(path)? > 1 {
                return Err("Hard-linked file: use Save As".into());
            }
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}
#[cfg(not(windows))]
fn install(temp: &Path, target: &Path, exists: bool) -> Result<(), String> {
    if exists {
        fs::rename(temp, target).map_err(|e| e.to_string())
    } else {
        fs::hard_link(temp, target).map_err(|e| e.to_string())?;
        fs::remove_file(temp).map_err(|e| e.to_string())
    }
}
#[cfg(windows)]
fn install(temp: &Path, target: &Path, exists: bool) -> Result<(), String> {
    windows::install(temp, target, exists)
}
fn sync_parent(path: &Path) {
    #[cfg(unix)]
    {
        if let Ok(dir) = File::open(
            path.parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new(".")),
        ) {
            let _ = dir.sync_all();
        }
    }
    #[cfg(not(unix))]
    let _ = path;
}
#[cfg(windows)]
mod windows {
    use super::*;
    use std::{
        ffi::c_void,
        os::windows::{ffi::OsStrExt, io::AsRawHandle},
    };
    #[repr(C)]
    struct Info {
        attributes: u32,
        creation: [u32; 2],
        access: [u32; 2],
        write: [u32; 2],
        volume: u32,
        size_high: u32,
        size_low: u32,
        links: u32,
        index_high: u32,
        index_low: u32,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn ReplaceFileW(
            target: *const u16,
            temp: *const u16,
            backup: *const u16,
            flags: u32,
            exclude: *mut c_void,
            reserved: *mut c_void,
        ) -> i32;
        fn MoveFileExW(source: *const u16, target: *const u16, flags: u32) -> i32;
        fn GetFileInformationByHandle(handle: *mut c_void, info: *mut Info) -> i32;
    }
    pub fn links(path: &Path) -> Result<u32, String> {
        let file = File::open(path).map_err(|e| e.to_string())?;
        let mut info = std::mem::MaybeUninit::<Info>::uninit();
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), info.as_mut_ptr()) } == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok(unsafe { info.assume_init() }.links)
    }
    pub fn install(temp: &Path, target: &Path, exists: bool) -> Result<(), String> {
        let wide = |p: &Path| {
            p.as_os_str()
                .encode_wide()
                .chain(Some(0))
                .collect::<Vec<_>>()
        };
        let target = wide(target);
        let temp = wide(temp);
        let ok = unsafe {
            if exists {
                ReplaceFileW(
                    target.as_ptr(),
                    temp.as_ptr(),
                    std::ptr::null(),
                    0,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            } else {
                MoveFileExW(temp.as_ptr(), target.as_ptr(), 8)
            }
        };
        if ok == 0 {
            Err(std::io::Error::last_os_error().to_string())
        } else {
            Ok(())
        }
    }
}
#[cfg(test)]
#[path = "safe_file_tests.rs"]
mod tests;
