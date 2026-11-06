//! Rune owns the texture handles; the backend owns fetch, validation and decode.

use crate::navidrome::backend::Cover;
use rune::Pixels;
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

struct Entry {
    full: Option<Pixels>,
    preview: Pixels,
    bytes: Arc<[u8]>,
    color: Option<[u8; 3]>,
    used: Instant,
}

fn memory(p: &Pixels) -> usize {
    let (w, h) = p.size();
    w as usize * h as usize * 4
}

fn pixels(image: image::RgbaImage) -> Pixels {
    let p = Pixels::new();
    p.set(image.width(), image.height(), image.into_raw());
    p
}

#[derive(Default)]
pub(super) struct Covers {
    items: HashMap<String, Entry>,
    failures: HashMap<String, Retry>,
}

struct Retry {
    attempts: u8,
    after: Option<Instant>,
}

impl Covers {
    pub fn insert(&mut self, key: String, cover: Cover) {
        self.failures.remove(&key);
        self.items.insert(
            key,
            Entry {
                full: Some(pixels(cover.image)),
                preview: pixels(cover.preview),
                bytes: cover.bytes,
                color: cover.color,
                used: Instant::now(),
            },
        );
        self.trim(32 * 1024 * 1024, 64 * 1024 * 1024);
    }

    fn trim(&mut self, full_budget: usize, retained_budget: usize) {
        let mut full: usize = self
            .items
            .values()
            .filter_map(|e| e.full.as_ref())
            .map(memory)
            .sum();
        let mut retained: usize = self
            .items
            .values()
            .map(|e| e.bytes.len() + memory(&e.preview))
            .sum();
        if full <= full_budget && retained <= retained_budget {
            return;
        }
        let mut keys: Vec<_> = self
            .items
            .iter()
            .map(|(k, e)| (e.used, k.clone()))
            .collect();
        keys.sort_by_key(|(used, _)| *used);
        for (_, key) in keys {
            if retained > retained_budget {
                let e = self.items.remove(&key).unwrap();
                full -= e.full.as_ref().map_or(0, memory);
                retained -= memory(&e.preview) + e.bytes.len();
            } else if full > full_budget {
                if let Some(p) = self.items.get_mut(&key).and_then(|e| e.full.take()) {
                    full -= memory(&p);
                }
            } else {
                break;
            }
        }
    }

    /// A changed key re-arms Rune's visibility callback. An on-screen demoted
    /// cover paints its preview immediately and schedules its own local decode.
    pub fn view_key(&self, key: &str) -> String {
        let tier = self.items.get(key).map_or("missing", |e| {
            if e.full.is_some() { "full" } else { "preview" }
        });
        let attempt = self
            .failures
            .get(key)
            .filter(|r| r.after.is_none())
            .map_or(0, |r| r.attempts);
        format!("{key}/{tier}/{attempt}")
    }

    pub fn failed(&mut self, key: &str, now: Instant) {
        let retry = self.failures.entry(key.to_owned()).or_insert(Retry {
            attempts: 0,
            after: None,
        });
        retry.attempts = retry.attempts.saturating_add(1);
        retry.after = Some(now + Duration::from_secs(1 << retry.attempts.min(6)));
    }

    pub fn poll_retries(&mut self, now: Instant) -> (bool, Option<Duration>) {
        let mut ready = false;
        let mut next = None;
        for retry in self.failures.values_mut() {
            if let Some(deadline) = retry.after {
                if now >= deadline {
                    retry.after = None;
                    ready = true;
                } else {
                    let delay = deadline - now;
                    next = Some(next.map_or(delay, |other: Duration| other.min(delay)));
                }
            }
        }
        (ready, next)
    }

    pub fn color(&self, key: &str) -> Option<[u8; 3]> {
        self.items.get(key)?.color
    }

    pub fn get(&self, key: &str) -> Option<Pixels> {
        let e = self.items.get(key)?;
        Some(e.full.as_ref().unwrap_or(&e.preview).clone())
    }

    /// A miss needs the server; a demoted image has enough data for a local decode.
    pub fn visible(&mut self, key: &str) -> Option<Option<Arc<[u8]>>> {
        if self
            .failures
            .get(key)
            .is_some_and(|retry| retry.after.is_some())
        {
            return None;
        }
        if let Some(e) = self.items.get_mut(key) {
            e.used = Instant::now();
            e.full.is_none().then(|| Some(e.bytes.clone()))
        } else {
            Some(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cover() -> Cover {
        Cover {
            image: image::RgbaImage::new(4, 4),
            preview: image::RgbaImage::new(1, 1),
            bytes: Arc::from([1, 2, 3]),
            color: None,
        }
    }

    #[test]
    fn demotion_rearms_visible_decode_and_keeps_preview_until_replacement() {
        let mut covers = Covers::default();
        covers.insert("art".into(), cover());
        let full_key = covers.view_key("art");
        covers.trim(0, 1024);
        assert_ne!(full_key, covers.view_key("art"));
        assert_eq!(covers.get("art").unwrap().size(), (1, 1));
        assert_eq!(covers.visible("art").unwrap().unwrap().as_ref(), &[1, 2, 3]);
        covers.insert("art".into(), cover());
        assert_eq!(full_key, covers.view_key("art"));
        assert_eq!(covers.get("art").unwrap().size(), (4, 4));
        assert!(covers.visible("art").is_none());
    }

    #[test]
    fn failed_cover_waits_before_rearming_a_visible_tile() {
        let mut covers = Covers::default();
        let now = Instant::now();
        let initial = covers.view_key("art");
        covers.failed("art", now);
        assert!(covers.visible("art").is_none());
        assert_eq!(
            covers.poll_retries(now),
            (false, Some(Duration::from_secs(2)))
        );
        assert_eq!(
            covers.poll_retries(now + Duration::from_secs(2)),
            (true, None)
        );
        assert_ne!(initial, covers.view_key("art"));
        assert!(matches!(covers.visible("art"), Some(None)));
    }
}
