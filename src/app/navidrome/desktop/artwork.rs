//! Rune owns the texture handles; the backend owns fetch, validation and decode.

use crate::navidrome::backend::Cover;
use rune::Pixels;
use std::{
    collections::HashMap,
    rc::{Rc, Weak},
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

pub(crate) struct View {
    pub key: String,
    pub pixels: Option<Pixels>,
    pub keep: Vec<Rc<()>>,
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
    held: HashMap<String, Weak<()>>,
    over_budget: bool,
}

struct Retry {
    attempts: u8,
    after: Option<Instant>,
}

impl Covers {
    /// The view owns this lease, including while waiting for its first pixels.
    /// Rune can remove a virtual row without firing an off-screen callback.
    pub fn hold(&mut self, key: &str) -> Rc<()> {
        if let Some(held) = self.held.get(key).and_then(Weak::upgrade) {
            return held;
        }
        let held = Rc::new(());
        self.held.insert(key.to_owned(), Rc::downgrade(&held));
        held
    }

    pub fn maintain(&mut self) {
        self.held.retain(|_, held| held.strong_count() > 0);
        if self.over_budget {
            self.trim(32 * 1024 * 1024, 64 * 1024 * 1024);
        }
    }

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
        self.held.retain(|_, held| held.strong_count() > 0);
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
            self.over_budget = false;
            return;
        }
        let mut keys: Vec<_> = self
            .items
            .iter()
            .filter(|(key, _)| !self.held.contains_key(*key))
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
        // A view's working set may exceed the budgets. Reclaim it after the
        // view releases its leases, rather than repeatedly decoding visible art.
        self.over_budget = full > full_budget || retained > retained_budget;
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

    pub fn get(&mut self, key: &str) -> Option<Pixels> {
        let e = self.items.get_mut(key)?;
        e.used = Instant::now();
        Some(e.full.as_ref().unwrap_or(&e.preview).clone())
    }

    pub fn bytes(&self, key: &str) -> Option<Arc<[u8]>> {
        Some(self.items.get(key)?.bytes.clone())
    }

    pub fn has_failed(&self, key: &str) -> bool {
        self.failures.contains_key(key)
    }

    pub fn retry(&mut self, key: &str) {
        self.failures.remove(key);
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
    fn shared_budget_demotes_old_textures_before_evicting_encoded_art() {
        let mut covers = Covers::default();
        covers.insert("old".into(), cover());
        covers.insert("new".into(), cover());
        covers.trim(64, 14);
        assert_eq!(covers.get("old").unwrap().size(), (1, 1));
        assert_eq!(covers.get("new").unwrap().size(), (4, 4));
        assert!(covers.bytes("old").is_some());
        covers.trim(64, 7);
        assert!(covers.get("old").is_none());
        assert!(covers.bytes("new").is_some());
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

    #[test]
    fn mounted_art_survives_pressure_until_every_view_releases_it() {
        let mut covers = Covers::default();
        // The player and a card can show the same cover. Hold before its reply
        // arrives, as the real placeholder does.
        let player = covers.hold("playing");
        let card = covers.hold("playing");
        covers.insert("playing".into(), cover());
        let shown = format!("{:?}", covers.get("playing").unwrap());
        let key = covers.view_key("playing");
        for index in 0..100 {
            covers.insert(format!("offscreen-{index}"), cover());
            covers.trim(0, 0);
            assert_eq!(format!("{:?}", covers.get("playing").unwrap()), shown);
            assert_eq!(covers.view_key("playing"), key);
            assert!(covers.visible("playing").is_none(), "no decode loop");
            assert_eq!(covers.items.len(), 1);
        }
        drop(card);
        covers.trim(0, 0);
        assert_eq!(covers.get("playing").unwrap().size(), (4, 4));
        drop(player);
        covers.trim(0, 0);
        assert!(covers.get("playing").is_none());
        assert!(covers.held.is_empty());
    }

    #[test]
    fn released_placeholders_leave_no_unbounded_lease_registry() {
        let mut covers = Covers::default();
        for index in 0..100 {
            drop(covers.hold(&format!("missing-{index}")));
        }
        covers.maintain();
        assert!(covers.held.is_empty());
    }
}
