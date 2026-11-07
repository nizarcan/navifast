use super::{Journal, MAX_ENTRIES};
use std::{
    io::Read,
    path::{Path, PathBuf},
};
const MAX_BYTES: u64 = 16 * 1024 * 1024;
pub(super) fn path(root: &Path, scope: &str) -> Result<PathBuf, String> {
    if scope.len() != 64 || !scope.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Invalid sync account.".into());
    }
    Ok(root.join(format!("{scope}.json")))
}
pub(super) fn load(path: &Path, scope: &str) -> Result<Journal, String> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Journal::new(scope.into()));
        }
        Err(_) => return Err("Could not read pending changes. Check folder permissions.".into()),
    };
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Could not read pending changes.")?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("The pending changes file is too large.".into());
    }
    let mut journal: Journal = serde_json::from_slice(&bytes)
        .map_err(|_| "The pending changes file is damaged. It has been kept for recovery.")?;
    if journal.version != 1
        || journal.scope != scope
        || journal.pending.len() > MAX_ENTRIES
        || journal.ratings.len() > MAX_ENTRIES
        || journal.favorite_guards.len() > MAX_ENTRIES
        || journal.ratings.iter().any(|(key, work)| {
            !matches!(work, super::Work::Rating { .. }) || key != &work.key() || !work.valid()
        })
        || journal
            .pending
            .iter()
            .any(|(key, entry)| key != &entry.work.key() || !entry.work.valid())
    {
        return Err(
            "The pending changes file is incompatible. It has been kept for recovery.".into(),
        );
    }
    journal.recover();
    Ok(journal)
}
pub(super) fn save(path: &Path, journal: &Journal) -> Result<(), String> {
    std::fs::create_dir_all(path.parent().ok_or("Invalid sync folder.")?)
        .and_then(|_| crate::util::write_json_atomic(path, journal, MAX_BYTES as usize))
        .map_err(|_| "Changes could not be saved. Keep the app open, free disk space or fix folder permissions, then retry.".into())
}
