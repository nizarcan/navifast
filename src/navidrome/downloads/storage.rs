//! Selection intent and small completion records are atomic; partial files are never playable.
use super::*;
use std::io::Read;
const MAX_MANIFEST: u64 = 16 * 1024 * 1024;

pub(crate) fn account_root(root: &Path, scope: &str) -> Result<PathBuf, String> {
    if scope.len() != 64 || !scope.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Invalid download account.".into());
    }
    Ok(root.join(scope))
}
pub(crate) fn partial_path(root: &Path, id: &str) -> PathBuf {
    root.join(format!("{}.partial", track_stem(id)))
}
fn record_path(root: &Path, id: &str) -> PathBuf {
    root.join(format!("{}.json", track_stem(id)))
}

pub(crate) fn write_json(path: &Path, value: &impl Serialize) -> std::io::Result<()> {
    crate::util::write_json_atomic(path, value, MAX_MANIFEST as usize)
}
pub(crate) fn persist(root: &Path, library: &Library) -> Result<(), String> {
    std::fs::create_dir_all(root)
        .and_then(|_| write_json(&root.join("selections.json"), library))
        .map_err(|_| "Could not save downloads. Check free space and folder permissions.".into())
}
pub(crate) fn load(root: &Path, scope: &str) -> Result<Library, String> {
    let file = match std::fs::File::open(root.join("selections.json")) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Library::new(scope.into()));
        }
        Err(_) => return Err("Could not read downloaded selections.".into()),
    };
    let mut bytes = Vec::new();
    file.take(MAX_MANIFEST + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Could not read downloaded selections.")?;
    if bytes.len() as u64 > MAX_MANIFEST {
        return Err("The download list is too large.".into());
    }
    let mut library: Library = serde_json::from_slice(&bytes)
        .map_err(|_| "The download list is damaged. Your audio files were kept.")?;
    if !library.valid(scope) {
        return Err("This download list is incompatible. Your audio files were kept.".into());
    }
    for (id, track) in &mut library.tracks {
        let record = std::fs::File::open(record_path(root, id))
            .ok()
            .and_then(|f| serde_json::from_reader::<_, Status>(f.take(16 * 1024)).ok());
        track.status = match record {
            Some(Status::Ready { bytes, codec })
                if bytes > 0
                    && std::fs::metadata(audio_path(root, id))
                        .is_ok_and(|m| m.is_file() && m.len() == bytes) =>
            {
                Status::Ready { bytes, codec }
            }
            Some(status @ (Status::Failed { .. } | Status::Cancelled)) => status,
            _ => Status::Queued,
        };
        // An interrupted transfer starts fresh; a partial is never mistaken for a local song.
        let _ = std::fs::remove_file(partial_path(root, id));
    }
    library.reindex();
    let paused: Vec<_> = library
        .selections
        .iter()
        .filter(|(_, s)| s.paused)
        .map(|(k, _)| k.clone())
        .collect();
    for key in paused {
        library.pause(&key);
    }
    // A crash after saving removal intent must not leave unowned audio forever.
    // Never inspect or remove unrelated names, and never collect a damaged manifest.
    let stems: BTreeSet<_> = library.tracks.keys().map(|id| track_stem(id)).collect();
    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            let Some((stem, extension)) = name.split_once('.') else {
                continue;
            };
            let managed = stem.len() == 64 && stem.bytes().all(|b| b.is_ascii_hexdigit());
            let unowned = managed
                && matches!(extension, "audio" | "json" | "partial")
                && !stems.contains(stem);
            let temporary = (managed || stem == "selections") && extension.ends_with(".tmp");
            if unowned || temporary {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
    Ok(library)
}
pub(crate) fn publish(root: &Path, id: &str, bytes: u64, codec: String) -> Result<Status, String> {
    let status = Status::Ready { bytes, codec };
    // Publish the completion record before the file. On restart both must exist and match.
    write_json(&record_path(root, id), &status)
        .and_then(|_| crate::util::replace_file(&partial_path(root, id), &audio_path(root, id)))
        .map_err(|_| "Could not finish the download. Check free space and try again.")?;
    Ok(status)
}
pub(crate) fn record_failure(root: &Path, id: &str, status: &Status) -> Result<(), String> {
    write_json(&record_path(root, id), status)
        .map_err(|_| "Could not save download status. Check free space.".into())
}
pub(crate) fn clear_record(root: &Path, id: &str) -> Result<(), String> {
    remove(&record_path(root, id))
        .map_err(|_| "Could not retry the download. Check folder permissions.".into())
}
fn remove(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}
pub(crate) fn remove_track(root: &Path, id: &str) -> Result<(), String> {
    for path in [
        audio_path(root, id),
        partial_path(root, id),
        record_path(root, id),
    ] {
        remove(&path).map_err(|_| "Could not remove a downloaded file. Try again.")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::tests::{selection, song};
    use super::*;
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir()
                .join(format!("navifast-download-{:016x}", rand::random::<u64>()));
            std::fs::create_dir(&root).unwrap();
            Self(root)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn interrupted_partial_and_unpublished_record_never_become_ready() {
        let f = Fixture::new();
        let mut l = Library::new("scope".into());
        l.keep(selection("one"), Some(vec![song("a")])).unwrap();
        persist(&f.0, &l).unwrap();
        std::fs::write(partial_path(&f.0, "a"), b"partial").unwrap();
        write_json(
            &record_path(&f.0, "a"),
            &Status::Ready {
                bytes: 7,
                codec: "pcm".into(),
            },
        )
        .unwrap();
        let loaded = load(&f.0, "scope").unwrap();
        assert_eq!(loaded.tracks["a"].status, Status::Queued);
        assert!(!partial_path(&f.0, "a").exists());
        std::fs::write(partial_path(&f.0, "a"), b"complete").unwrap();
        publish(&f.0, "a", 8, "pcm".into()).unwrap();
        assert!(load(&f.0, "scope").unwrap().tracks["a"].status.ready());
        std::fs::write(audio_path(&f.0, "a"), b"short").unwrap();
        assert!(!load(&f.0, "scope").unwrap().tracks["a"].status.ready());
    }
    #[test]
    fn failed_publish_keeps_previous_files_and_removal_recovery_collects_only_owned_names() {
        let f = Fixture::new();
        let mut library = Library::new("scope".into());
        library
            .keep(selection("one"), Some(vec![song("a"), song("b")]))
            .unwrap();
        persist(&f.0, &library).unwrap();
        std::fs::write(partial_path(&f.0, "a"), b"complete").unwrap();
        publish(&f.0, "a", 8, "pcm".into()).unwrap();
        std::fs::write(partial_path(&f.0, "b"), b"complete").unwrap();
        // Same atomic publication failure path used by ENOSPC: no prior ready file is touched.
        std::fs::create_dir(record_path(&f.0, "b")).unwrap();
        assert!(publish(&f.0, "b", 8, "pcm".into()).is_err());
        assert!(load(&f.0, "scope").unwrap().tracks["a"].status.ready());
        std::fs::write(f.0.join("personal.audio"), b"unrelated").unwrap();
        library.remove(&Kind::Playlist.key("one"));
        persist(&f.0, &library).unwrap();
        load(&f.0, "scope").unwrap();
        assert!(!audio_path(&f.0, "a").exists());
        assert!(f.0.join("personal.audio").exists());
    }
    #[test]
    fn damaged_manifest_keeps_completed_files_and_scope_is_checked() {
        let f = Fixture::new();
        std::fs::write(audio_path(&f.0, "a"), b"kept").unwrap();
        std::fs::write(f.0.join("selections.json"), b"{").unwrap();
        assert!(load(&f.0, "scope").is_err());
        assert_eq!(std::fs::read(audio_path(&f.0, "a")).unwrap(), b"kept");
        assert!(account_root(&f.0, "../../elsewhere").is_err());
        assert!(account_root(&f.0, &"a".repeat(64)).is_ok());
    }
}
