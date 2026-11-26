//! Hook scripts for the hook-phase tests (Windows: `.cmd`; Unix: `sh`).
//!
//! The recording script writes everything under `.wt/` (never part of a
//! snapshot), so recording does not dirty the working directory:
//! `.wt/hook-log` (phases in run order), `.wt/<phase>.env` (`WT_*`
//! variables), `.wt/<phase>.stdin` and `.wt/<phase>.file` (the content of
//! `f.txt` in the tree when the hook ran).
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

#[cfg(windows)]
pub fn write_hook(root: &Path, phase: &str, body: &str) {
    let dir = root.join(".wt").join("hooks");
    fs::create_dir_all(&dir).unwrap();
    for ext in ["", ".cmd"] {
        let _ = fs::remove_file(dir.join(format!("{phase}{ext}")));
    }
    fs::write(
        dir.join(format!("{phase}.cmd")),
        format!("@echo off\r\n{body}\r\n"),
    )
    .unwrap();
}

#[cfg(unix)]
pub fn write_hook(root: &Path, phase: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let dir = root.join(".wt").join("hooks");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(phase);
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
}

pub fn remove_hook(root: &Path, phase: &str) {
    let dir = root.join(".wt").join("hooks");
    for ext in ["", ".cmd"] {
        let _ = fs::remove_file(dir.join(format!("{phase}{ext}")));
    }
}

#[cfg(windows)]
pub mod script {
    pub const RECORD: &str = "echo %WT_HOOK%>> \"%WT_ROOT%\\.wt\\hook-log\"\r\n\
        set WT_ > \"%WT_ROOT%\\.wt\\%WT_HOOK%.env\"\r\n\
        findstr \"^\" > \"%WT_ROOT%\\.wt\\%WT_HOOK%.stdin\"\r\n\
        type \"%WT_TREE_PATH%\\f.txt\" > \"%WT_ROOT%\\.wt\\%WT_HOOK%.file\" 2>nul\r\n\
        exit /b 0";
    pub const FAIL: &str = "echo %WT_HOOK%>> \"%WT_ROOT%\\.wt\\hook-log\"\r\n\
        echo blocked by hook 1>&2\r\nexit /b 7";
    pub const SLEEP: &str = "ping -n 30 127.0.0.1 >nul";
}

#[cfg(unix)]
pub mod script {
    pub const RECORD: &str = "echo \"$WT_HOOK\" >> \"$WT_ROOT/.wt/hook-log\"\n\
        env | grep '^WT_' > \"$WT_ROOT/.wt/$WT_HOOK.env\"\n\
        cat > \"$WT_ROOT/.wt/$WT_HOOK.stdin\"\n\
        cat \"$WT_TREE_PATH/f.txt\" > \"$WT_ROOT/.wt/$WT_HOOK.file\" 2>/dev/null\n\
        exit 0";
    pub const FAIL: &str = "echo \"$WT_HOOK\" >> \"$WT_ROOT/.wt/hook-log\"\n\
        echo blocked by hook >&2\nexit 7";
    pub const SLEEP: &str = "sleep 30";
}

/// The `WT_*` variables a recording hook of `phase` saw.
pub fn hook_env(root: &Path, phase: &str) -> BTreeMap<String, String> {
    let path = root.join(".wt").join(format!("{phase}.env"));
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{phase} hook did not run ({}): {e}", path.display()));
    text.lines()
        .filter_map(|l| l.trim_end_matches('\r').split_once('='))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// What a recording hook of `phase` read from stdin (line endings
/// normalized to `\n`).
pub fn hook_stdin(root: &Path, phase: &str) -> String {
    fs::read_to_string(root.join(".wt").join(format!("{phase}.stdin")))
        .unwrap_or_default()
        .replace("\r\n", "\n")
}

/// The content of `f.txt` when a recording hook of `phase` ran.
pub fn hook_file(root: &Path, phase: &str) -> String {
    fs::read_to_string(root.join(".wt").join(format!("{phase}.file")))
        .unwrap_or_default()
        .replace("\r\n", "\n")
}

/// Phases whose hooks ran, in order.
pub fn hook_log(root: &Path) -> Vec<String> {
    fs::read_to_string(root.join(".wt").join("hook-log"))
        .unwrap_or_default()
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

/// Forget what earlier hooks recorded.
pub fn clear_records(root: &Path) {
    let wt = root.join(".wt");
    let _ = fs::remove_file(wt.join("hook-log"));
    for entry in fs::read_dir(&wt).unwrap().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.ends_with(".env") || name.ends_with(".stdin") || name.ends_with(".file") {
            let _ = fs::remove_file(entry.path());
        }
    }
}
