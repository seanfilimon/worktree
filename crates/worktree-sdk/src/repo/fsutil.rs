//! Small filesystem helpers shared by the repository and the object store.
//!
//! All metadata writes go through [`write_atomic`] so that a crash or a
//! concurrent reader never observes a half-written file.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::error::Result;

/// Number of attempts for a rename that fails with a transient sharing
/// violation (Windows reports these as `PermissionDenied`).
const RENAME_ATTEMPTS: u32 = 8;

/// Prefix of temp files created by atomic writes. The working-directory
/// walker and ref listings skip names with this prefix.
pub(crate) const TEMP_PREFIX: &str = ".wt-tmp-";

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A temp-file path next to `target` that no other writer (thread or
/// process) will pick.
pub(crate) fn temp_path_for(target: &Path) -> PathBuf {
    let n = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp = format!("{TEMP_PREFIX}{name}-{}-{n}-{nanos}", std::process::id());
    target.with_file_name(tmp)
}

/// Write a fresh temp file containing `data` next to `target`.
pub(crate) fn write_temp(target: &Path, data: &[u8]) -> io::Result<PathBuf> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = temp_path_for(target);
    let mut file = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
    if let Err(e) = file.write_all(data).and_then(|()| file.flush()) {
        drop(file);
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(tmp)
}

/// Rename `from` onto `to`, retrying transient Windows sharing violations.
pub(crate) fn rename_with_retry(from: &Path, to: &Path) -> io::Result<()> {
    let mut attempt = 0;
    loop {
        match fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(e)
                if e.kind() == io::ErrorKind::PermissionDenied && attempt + 1 < RENAME_ATTEMPTS =>
            {
                attempt += 1;
                std::thread::sleep(Duration::from_millis(5 << attempt));
            }
            Err(e) => return Err(e),
        }
    }
}

/// Atomically replace `path` with `data` (temp file + rename), creating
/// parent directories as needed.
pub(crate) fn write_atomic(path: &Path, data: &[u8]) -> Result<()> {
    let tmp = write_temp(path, data)?;
    if let Err(e) = rename_with_retry(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(e.into());
    }
    Ok(())
}

/// Serialize `value` as pretty JSON and write it atomically.
pub(crate) fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let mut data = serde_json::to_vec_pretty(value)?;
    data.push(b'\n');
    write_atomic(path, &data)
}

/// Read and parse a JSON file; `None` if it does not exist.
pub(crate) fn read_json_opt<T: DeserializeOwned>(path: &Path) -> Result<Option<T>> {
    match fs::read(path) {
        Ok(data) => Ok(Some(serde_json::from_slice(&data)?)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Read a UTF-8 text file; `None` if it does not exist.
pub(crate) fn read_string_opt(path: &Path) -> Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(s) => Ok(Some(s)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Remove a file, treating "already gone" as success.
pub(crate) fn remove_file_if_exists(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

/// Remove a directory tree, treating "already gone" as success.
pub(crate) fn remove_dir_all_if_exists(path: &Path) -> Result<()> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

/// Remove empty directories from `dir` upwards, stopping at (and never
/// removing) `stop`.
pub(crate) fn prune_empty_dirs(dir: &Path, stop: &Path) {
    let mut current = dir.to_path_buf();
    while current != stop && current.starts_with(stop) {
        // `remove_dir` only succeeds on empty directories.
        if fs::remove_dir(&current).is_err() {
            break;
        }
        if !current.pop() {
            break;
        }
    }
}

/// Modification time in nanoseconds since the Unix epoch (0 if earlier or
/// unavailable).
pub(crate) fn mtime_ns(meta: &fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

/// Current time in nanoseconds since the Unix epoch.
pub(crate) fn now_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

/// Whether `meta` describes an executable file. Always `false` on platforms
/// without an executable bit.
#[cfg(unix)]
pub(crate) fn is_executable(meta: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o111 != 0
}

/// Set or clear the executable bits of `path` (no-op off Unix).
#[cfg(unix)]
pub(crate) fn set_executable(path: &Path, executable: bool) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let meta = fs::metadata(path)?;
    let mode = meta.permissions().mode();
    let new_mode = if executable {
        mode | ((mode & 0o444) >> 2)
    } else {
        mode & !0o111
    };
    if new_mode != mode {
        fs::set_permissions(path, fs::Permissions::from_mode(new_mode))?;
    }
    Ok(())
}

/// Set or clear the executable bits of `path` (no-op off Unix).
#[cfg(not(unix))]
pub(crate) fn set_executable(_path: &Path, _executable: bool) -> Result<()> {
    Ok(())
}

// ---- symbolic links ----
//
// Links are data: their target text is read and written, never followed.

/// Metadata of whatever is at `path` itself (a symlink is not followed);
/// `None` if nothing is there.
pub(crate) fn symlink_meta(path: &Path) -> io::Result<Option<fs::Metadata>> {
    match fs::symlink_metadata(path) {
        Ok(meta) => Ok(Some(meta)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// The target of the symlink at `path` as the bytes a manifest stores: raw
/// bytes on Unix; elsewhere the target's text with `\` turned into `/`, so
/// a link created on Windows resolves on other platforms too.
pub(crate) fn read_link_target(path: &Path) -> io::Result<Vec<u8>> {
    let target = fs::read_link(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        Ok(target.into_os_string().into_vec())
    }
    #[cfg(not(unix))]
    {
        Ok(target.to_string_lossy().replace('\\', "/").into_bytes())
    }
}

/// Fail if a directory on the way from `base` to `base/rel` is a symbolic
/// link. `rel` is a validated, '/'-separated manifest path; `base` itself
/// is trusted. Writing or deleting `base/rel` would otherwise act on
/// whatever the link points at — possibly outside the working tree.
pub(crate) fn ensure_no_symlink_parents(base: &Path, rel: &str) -> Result<()> {
    let mut dir = base.to_path_buf();
    let mut components = rel.split('/').peekable();
    while let Some(component) = components.next() {
        if components.peek().is_none() {
            break; // the leaf itself is handled by the caller
        }
        dir.push(component);
        match symlink_meta(&dir)? {
            // Nothing below exists yet; directories are created fresh.
            None => break,
            Some(meta) if meta.file_type().is_symlink() => {
                return Err(crate::error::SdkError::InvalidArgument(format!(
                    "refusing to touch {rel}: {} is a symbolic link",
                    dir.strip_prefix(base).unwrap_or(&dir).display()
                )));
            }
            Some(_) => {}
        }
    }
    Ok(())
}

/// Remove the file or symlink at `path` itself (never what a link points
/// at), treating "already gone" as success.
pub(crate) fn remove_file_or_link(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => {
            // A directory symlink on Windows is removed like a directory.
            let is_link =
                symlink_meta(path).is_ok_and(|m| m.is_some_and(|m| m.file_type().is_symlink()));
            if is_link && fs::remove_dir(path).is_ok() {
                return Ok(());
            }
            Err(e.into())
        }
    }
}

/// Put a symlink with the given `target` bytes at `path`, replacing a file
/// or link already there and creating parent directories. Returns `false`
/// when this platform (or this user) cannot create the link; the caller
/// then stores the target text in a regular file instead.
#[cfg(unix)]
pub(crate) fn write_symlink(path: &Path, target: &[u8]) -> io::Result<bool> {
    use std::os::unix::ffi::OsStrExt;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    // Link under a temp name, then rename: `rename` replaces the entry at
    // `path` itself and never follows a link that is already there.
    let tmp = temp_path_for(path);
    std::os::unix::fs::symlink(std::ffi::OsStr::from_bytes(target), &tmp)?;
    if let Err(e) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(true)
}

/// Put a symlink with the given `target` bytes at `path` (see the Unix
/// variant). Windows needs Developer Mode or the "create symbolic links"
/// privilege, and distinguishes file links from directory links: the kind
/// is chosen by what the target is right now (a file link if it does not
/// exist yet). Any failure reports `false`.
#[cfg(windows)]
pub(crate) fn write_symlink(path: &Path, target: &[u8]) -> io::Result<bool> {
    let Ok(text) = std::str::from_utf8(target) else {
        return Ok(false);
    };
    let Some(parent) = path.parent() else {
        return Ok(false);
    };
    fs::create_dir_all(parent)?;
    let native = text.replace('/', "\\");
    let to_dir = parent.join(&native).is_dir();
    let tmp = temp_path_for(path);
    let made = if to_dir {
        std::os::windows::fs::symlink_dir(&native, &tmp)
    } else {
        std::os::windows::fs::symlink_file(&native, &tmp)
    };
    if made.is_err() {
        return Ok(false);
    }
    let remove_tmp = |tmp: &Path| {
        let _ = fs::remove_file(tmp).or_else(|_| fs::remove_dir(tmp));
    };
    if let Err(e) = remove_file_or_link(path) {
        remove_tmp(&tmp);
        return Err(io::Error::other(e.to_string()));
    }
    if let Err(e) = fs::rename(&tmp, path) {
        remove_tmp(&tmp);
        return Err(e);
    }
    Ok(true)
}

/// Symlinks cannot be created on this platform.
#[cfg(not(any(unix, windows)))]
pub(crate) fn write_symlink(_path: &Path, _target: &[u8]) -> io::Result<bool> {
    Ok(false)
}

/// Append one line to a file in a single write (creating it if needed).
pub(crate) fn append_line(path: &Path, line: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file: File = OpenOptions::new().create(true).append(true).open(path)?;
    let mut buf = String::with_capacity(line.len() + 1);
    buf.push_str(line);
    buf.push('\n');
    file.write_all(buf.as_bytes())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_atomic_replaces_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a/b/file.txt");
        write_atomic(&p, b"one").unwrap();
        write_atomic(&p, b"two").unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"two");
        // No temp files left behind.
        let names: Vec<_> = fs::read_dir(p.parent().unwrap()).unwrap().collect();
        assert_eq!(names.len(), 1);
    }

    #[test]
    fn json_roundtrip_and_missing() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("x.json");
        assert_eq!(read_json_opt::<Vec<u32>>(&p).unwrap(), None);
        write_json(&p, &vec![1u32, 2, 3]).unwrap();
        assert_eq!(read_json_opt::<Vec<u32>>(&p).unwrap(), Some(vec![1, 2, 3]));
    }

    #[test]
    fn prune_stops_at_boundary() {
        let dir = tempfile::tempdir().unwrap();
        let deep = dir.path().join("a/b/c");
        fs::create_dir_all(&deep).unwrap();
        fs::write(dir.path().join("a/keep.txt"), b"x").unwrap();
        prune_empty_dirs(&deep, dir.path());
        assert!(!dir.path().join("a/b").exists());
        assert!(dir.path().join("a").exists());
    }

    #[cfg(unix)]
    #[test]
    fn symlink_helpers_act_on_the_link_itself() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join("tree");
        let outside = dir.path().join("outside");
        fs::create_dir_all(base.join("real/sub")).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("victim"), b"keep").unwrap();
        symlink("../outside", base.join("out")).unwrap();

        // Parents: real directories and not-yet-existing ones pass; a
        // linked directory anywhere on the way fails. The leaf is not checked.
        ensure_no_symlink_parents(&base, "real/sub/file").unwrap();
        ensure_no_symlink_parents(&base, "new/dir/file").unwrap();
        ensure_no_symlink_parents(&base, "out").unwrap();
        assert!(ensure_no_symlink_parents(&base, "out/victim").is_err());
        assert!(ensure_no_symlink_parents(&base, "out/deeper/file").is_err());

        // write_symlink replaces a file or a link in place, verbatim.
        let link = base.join("real/link");
        fs::write(&link, b"was a file").unwrap();
        assert!(write_symlink(&link, b"../../outside/victim").unwrap());
        assert_eq!(read_link_target(&link).unwrap(), b"../../outside/victim");
        assert!(write_symlink(&link, b"/absolute/elsewhere").unwrap());
        assert_eq!(read_link_target(&link).unwrap(), b"/absolute/elsewhere");
        assert_eq!(fs::read(outside.join("victim")).unwrap(), b"keep");
        assert!(symlink_meta(&link)
            .unwrap()
            .unwrap()
            .file_type()
            .is_symlink());

        // Removing a link removes the link, not what it points at.
        remove_file_or_link(&base.join("out")).unwrap();
        assert!(symlink_meta(&base.join("out")).unwrap().is_none());
        assert_eq!(fs::read(outside.join("victim")).unwrap(), b"keep");
        remove_file_or_link(&base.join("out")).unwrap();
    }

    #[test]
    fn remove_helpers_tolerate_missing() {
        let dir = tempfile::tempdir().unwrap();
        remove_file_if_exists(&dir.path().join("nope")).unwrap();
        remove_dir_all_if_exists(&dir.path().join("nope")).unwrap();
    }
}
