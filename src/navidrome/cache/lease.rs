use super::*;
use std::io::Read;

struct Handle {
    cache: Cache,
    id: u64,
}
#[derive(Clone)]
pub struct Lease(Arc<Handle>);
impl Lease {
    pub(crate) fn invalidate(&self) {
        let mut state = self.0.cache.0.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(entry) = state.entries.get_mut(&self.0.id) {
            entry.retiring = true;
            let key = entry.record.key.clone();
            if state.current.get(&key) == Some(&self.0.id) {
                state.current.remove(&key);
            }
        }
    }
    pub(super) fn new(cache: Cache, id: u64) -> Self {
        Self(Arc::new(Handle { cache, id }))
    }
    pub(crate) fn length(&self) -> Option<u64> {
        let state = self.0.cache.0.lock().unwrap_or_else(|p| p.into_inner());
        state.entries.get(&self.0.id)?.record.length
    }
    pub(crate) fn complete_length(&self) -> Option<u64> {
        let state = self.0.cache.0.lock().unwrap_or_else(|p| p.into_inner());
        let record = &state.entries.get(&self.0.id)?.record;
        record.complete().then_some(record.length).flatten()
    }
    pub(crate) fn read(&self, position: u64, maximum: usize) -> Option<Vec<u8>> {
        let mut state = self.0.cache.0.lock().unwrap_or_else(|p| p.into_inner());
        let entry = state.entries.get_mut(&self.0.id)?;
        let (&start, block) = entry
            .record
            .blocks
            .range(position.saturating_sub(BLOCK as u64)..=position)
            .rev()
            .find(|(start, block)| **start + u64::from(block.bytes) > position)?;
        let path = storage::block_path(&entry.path, start);
        let bytes = std::fs::File::open(&path).ok().and_then(|file| {
            let mut bytes = Vec::with_capacity(block.bytes as usize);
            file.take(BLOCK as u64 + 1).read_to_end(&mut bytes).ok()?;
            (bytes.len() == block.bytes as usize
                && format!("{:x}", Sha256::digest(&bytes)) == block.sha256)
                .then_some(bytes)
        });
        match bytes {
            Some(mut bytes) => {
                let offset = (position - start) as usize;
                if offset > 0 {
                    bytes.drain(..offset);
                }
                bytes.truncate(maximum);
                Some(bytes)
            }
            None => {
                match std::fs::remove_file(path) {
                    Ok(()) => {
                        let removed = entry.record.blocks.remove(&start).unwrap();
                        entry.dirty = BLOCK;
                        state.bytes = state.bytes.saturating_sub(u64::from(removed.bytes));
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        let removed = entry.record.blocks.remove(&start).unwrap();
                        entry.dirty = BLOCK;
                        state.bytes = state.bytes.saturating_sub(u64::from(removed.bytes));
                    }
                    Err(_) => {
                        entry.retiring = true;
                        let key = entry.record.key.clone();
                        if state.current.get(&key) == Some(&self.0.id) {
                            state.current.remove(&key);
                        }
                    }
                }
                state.emit(true);
                None
            }
        }
    }
    /// Cache misses and cache-write failures never become streaming failures.
    pub(crate) fn store(&self, start: u64, bytes: &[u8], length: Option<u64>) {
        if bytes.is_empty()
            || bytes.len() > BLOCK
            || start.checked_add(bytes.len() as u64).is_none()
        {
            return;
        }
        let mut state = self.0.cache.0.lock().unwrap_or_else(|p| p.into_inner());
        let Some(entry) = state.entries.get(&self.0.id) else {
            return;
        };
        if !state.config.enabled
            || entry.retiring
            || entry.failed
            || (entry.record.blocks.len() >= 32768 && !entry.record.blocks.contains_key(&start))
        {
            return;
        }
        let previous = entry
            .record
            .blocks
            .get(&start)
            .map_or(0, |b| u64::from(b.bytes));
        let added = (bytes.len() as u64).saturating_sub(previous);
        state.evict(added);
        if state.bytes.saturating_add(added) > state.config.bytes() {
            return;
        }
        let entry = state.entries.get_mut(&self.0.id).unwrap();
        if storage::write(&entry.path, start, bytes).is_err() {
            entry.failed = true;
            state.error = Some("Audio could not be cached. Streaming continues. Free disk space or check folder permissions, then retry.".into());
            state.emit(true);
            return;
        }
        entry.record.blocks.insert(
            start,
            Block {
                bytes: bytes.len() as u32,
                sha256: format!("{:x}", Sha256::digest(bytes)),
            },
        );
        if length.is_some() {
            entry.record.length = length;
        }
        entry.dirty = entry.dirty.saturating_add(bytes.len());
        // Check coverage only when this block reaches EOF, not once per buffered block.
        let checkpoint = entry.dirty >= 1024 * 1024
            || (entry
                .record
                .length
                .is_some_and(|length| start + bytes.len() as u64 >= length)
                && entry.record.complete());
        state.bytes = state.bytes.saturating_sub(previous) + bytes.len() as u64;
        if checkpoint {
            state.checkpoint(self.0.id);
        }
        state.emit(false);
    }
    pub(crate) fn finish(&self, length: u64) {
        let mut state = self.0.cache.0.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(entry) = state.entries.get_mut(&self.0.id)
            && !entry.retiring
            && !entry.failed
        {
            entry.record.length = Some(length);
            state.checkpoint(self.0.id);
            state.emit(true);
        }
    }
}
impl State {
    fn checkpoint(&mut self, id: u64) {
        let Some(entry) = self.entries.get_mut(&id) else {
            return;
        };
        if entry.retiring {
            return;
        }
        if storage::manifest(&entry.path, &entry.record).is_err() {
            entry.failed = true;
            self.error = Some("The audio cache index could not be saved. Streaming continues without saving new bytes.".into());
        } else {
            entry.dirty = 0;
        }
    }
}
impl Drop for Handle {
    fn drop(&mut self) {
        let mut state = self.cache.0.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(entry) = state.entries.get_mut(&self.id) {
            entry.pins = entry.pins.saturating_sub(1);
            if entry.record.blocks.is_empty() {
                entry.retiring = true;
            }
            state.checkpoint(self.id);
        }
        state.evict(0);
        state.emit(true);
    }
}
