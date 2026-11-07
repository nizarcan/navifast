use super::{BLOCK, MAX_ENTRIES, Record};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
};
const MAX_MANIFEST: usize = 4 * 1024 * 1024;
pub(super) fn hash(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}
pub(super) fn block_path(root: &Path, start: u64) -> PathBuf {
    root.join(format!("{start:016x}.block"))
}
pub(super) fn manifest(root: &Path, record: &Record) -> std::io::Result<()> {
    crate::util::write_json_atomic(&root.join("manifest.json"), record, MAX_MANIFEST)
}
pub(super) fn write(root: &Path, start: u64, bytes: &[u8]) -> std::io::Result<()> {
    write_with(root, start, |file| file.write_all(bytes))
}
pub(super) fn write_with(
    root: &Path,
    start: u64,
    write: impl FnOnce(&mut std::fs::File) -> std::io::Result<()>,
) -> std::io::Result<()> {
    let temporary = root.join(format!("{:016x}.tmp", rand::random::<u64>()));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    // Disposable bytes need atomic publication, not a synchronous drive flush per block.
    // Manifest checkpoints and read-time digests reject interrupted/corrupted cache data.
    let written = write(&mut file);
    drop(file);
    let result =
        written.and_then(|()| crate::util::replace_file(&temporary, &block_path(root, start)));
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}
pub(super) fn load(root: &Path) -> Result<Vec<(PathBuf, Record)>, String> {
    let folders = match std::fs::read_dir(root) {
        Ok(folders) => folders,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err("Could not read the audio cache. Check folder permissions.".into()),
    };
    let mut records = Vec::new();
    for folder in folders.flatten() {
        if !folder.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let name = folder.file_name();
        let Some(name) = name.to_str() else { continue };
        let Some((key, nonce)) = name.split_once('-') else {
            continue;
        };
        if !hash(key) || nonce.len() != 16 || !nonce.bytes().all(|b| b.is_ascii_hexdigit()) {
            continue;
        }
        let path = folder.path();
        let loaded = std::fs::File::open(path.join("manifest.json"))
            .ok()
            .and_then(|file| {
                let mut bytes = Vec::new();
                file.take(MAX_MANIFEST as u64 + 1)
                    .read_to_end(&mut bytes)
                    .ok()?;
                (bytes.len() <= MAX_MANIFEST)
                    .then(|| serde_json::from_slice::<Record>(&bytes).ok())
                    .flatten()
            });
        let Some(mut record) =
            loaded.filter(|r| r.version == 1 && r.key == key && r.blocks.len() <= 32768)
        else {
            std::fs::remove_dir_all(&path)
                .map_err(|_| "Could not remove invalid cached audio. Check folder permissions.")?;
            continue;
        };
        record.blocks.retain(|start, block| {
            block.bytes > 0
                && block.bytes as usize <= BLOCK
                && hash(&block.sha256)
                && start.checked_add(u64::from(block.bytes)).is_some()
                && std::fs::metadata(block_path(&path, *start))
                    .is_ok_and(|m| m.is_file() && m.len() == u64::from(block.bytes))
        });
        // Remove only cache-owned orphan blocks and abandoned temporaries.
        if let Ok(files) = std::fs::read_dir(&path) {
            for file in files.flatten() {
                let name = file.file_name();
                let Some(name) = name.to_str() else { continue };
                if let Some((stem, extension)) = name.split_once('.')
                    && stem.len() == 16
                    && stem.bytes().all(|b| b.is_ascii_hexdigit())
                    && (extension == "tmp"
                        || (extension == "block"
                            && u64::from_str_radix(stem, 16)
                                .is_ok_and(|start| !record.blocks.contains_key(&start))))
                {
                    std::fs::remove_file(file.path()).map_err(
                        |_| "Could not remove incomplete cached audio. Check folder permissions.",
                    )?;
                }
            }
        }
        if records.len() >= MAX_ENTRIES {
            std::fs::remove_dir_all(&path)
                .map_err(|_| "Could not remove invalid cached audio. Check folder permissions.")?;
        } else {
            records.push((path, record));
        }
    }
    records.sort_by_key(|(_, r)| r.touched);
    Ok(records)
}
