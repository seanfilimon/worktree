//! Small filesystem helpers with panicking, path-annotated errors.

use std::collections::BTreeMap;
use std::path::Path;

/// Write `content`, creating parent directories.
#[track_caller]
pub fn write(path: &Path, content: impl AsRef<[u8]>) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .unwrap_or_else(|e| panic!("create_dir_all {}: {e}", parent.display()));
    }
    std::fs::write(path, content).unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
}

#[track_caller]
pub fn read(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

#[track_caller]
pub fn read_string(path: &Path) -> String {
    String::from_utf8_lossy(&read(path)).into_owned()
}

#[track_caller]
pub fn remove(path: &Path) {
    let r = if path.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    };
    r.unwrap_or_else(|e| panic!("remove {}: {e}", path.display()));
}

/// Directory names never compared as user content.
pub const METADATA_DIRS: &[&str] = &[".wt", ".wt-tree", ".git"];

/// All regular files below `root` ('/'-separated relative path -> bytes),
/// skipping `.wt/`, `.wt-tree/` and `.git/`.
#[track_caller]
pub fn dir_files(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut out = BTreeMap::new();
    collect(root, root, &mut out);
    out
}

fn collect(root: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
    let entries =
        std::fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()));
    for entry in entries {
        let entry = entry.unwrap_or_else(|e| panic!("read_dir entry in {}: {e}", dir.display()));
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let ft = entry
            .file_type()
            .unwrap_or_else(|e| panic!("file_type {}: {e}", path.display()));
        if ft.is_dir() {
            if METADATA_DIRS.contains(&name.as_str()) {
                continue;
            }
            collect(root, &path, out);
        } else if ft.is_file() {
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            out.insert(rel, read(&path));
        }
    }
}

/// Render a file map for assertion messages.
pub fn describe_files(files: &BTreeMap<String, Vec<u8>>) -> String {
    files
        .iter()
        .map(|(k, v)| {
            format!(
                "  {k} ({} bytes): {:?}",
                v.len(),
                String::from_utf8_lossy(&v[..v.len().min(80)])
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Assert two directory trees hold identical user files.
#[track_caller]
pub fn assert_same_files(a: &Path, b: &Path) {
    let fa = dir_files(a);
    let fb = dir_files(b);
    assert!(
        fa == fb,
        "working directories differ\n{}:\n{}\n{}:\n{}",
        a.display(),
        describe_files(&fa),
        b.display(),
        describe_files(&fb)
    );
}

/// File names inside a .tar.gz archive (directories excluded, '/'-separated,
/// leading "./" stripped).
#[track_caller]
pub fn tar_gz_entries(path: &Path) -> BTreeMap<String, Vec<u8>> {
    use std::io::Read;
    let file = std::fs::File::open(path).unwrap_or_else(|e| panic!("open {}: {e}", path.display()));
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(file));
    let mut out = BTreeMap::new();
    let entries = archive
        .entries()
        .unwrap_or_else(|e| panic!("read tar {}: {e}", path.display()));
    for entry in entries {
        let mut entry = entry.unwrap_or_else(|e| panic!("tar entry in {}: {e}", path.display()));
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let name = entry
            .path()
            .unwrap_or_else(|e| panic!("tar entry path in {}: {e}", path.display()))
            .to_string_lossy()
            .replace('\\', "/");
        let name = name.trim_start_matches("./").to_string();
        let mut data = Vec::new();
        entry
            .read_to_end(&mut data)
            .unwrap_or_else(|e| panic!("tar entry {name}: {e}"));
        out.insert(name, data);
    }
    out
}
