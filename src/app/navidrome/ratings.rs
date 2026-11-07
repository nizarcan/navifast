//! One optimistic value and one serialized write per account entity.
use std::collections::{BTreeMap, HashMap};
use std::time::{Duration, Instant};

use super::{App, favorites::Item};
use crate::navidrome::{
    api::RequestError,
    backend::{Event, Request},
    ratings::Kind,
};

#[derive(Clone, Debug)]
pub enum Action {
    Set { uri: String, value: u8 },
    Retry(String),
}

struct Entry {
    kind: Kind,
    id: String,
    name: String,
    confirmed: u8,
    shown: u8,
    desired: Option<u8>,
    flight: Option<Flight>,
    attempts: u8,
    durable: bool,
}

#[derive(Clone, Copy)]
enum Flight {
    Write { token: u64, value: u8 },
    Read(u64),
}

pub struct Failure {
    pub name: String,
    pub message: String,
    value: u8,
}

#[derive(Default)]
pub struct State {
    entries: HashMap<String, Entry>,
    pub failures: BTreeMap<String, Failure>,
    // Only unsettled entries schedule work. Idle menus and playback do no scans.
    due: HashMap<String, Instant>,
    serial: u64,
}

struct Operation {
    kind: Kind,
    id: String,
    flight: Flight,
}

impl State {
    pub(super) fn release(&mut self, uri: &str) {
        self.entries.remove(uri);
        self.due.remove(uri);
        self.failures.remove(uri);
    }
    pub(super) fn hold(&mut self, kind: Kind, id: String, name: String, value: u8, pending: bool) {
        let uri = kind.uri(&id);
        self.entries.insert(
            uri.clone(),
            Entry {
                kind,
                id,
                name,
                confirmed: value,
                shown: value,
                desired: pending.then_some(value),
                flight: None,
                attempts: 0,
                durable: pending,
            },
        );
        self.due.remove(&uri);
        self.failures.remove(&uri);
    }

    pub fn value(&self, uri: &str) -> Option<u8> {
        self.entries.get(uri).map(|e| e.shown)
    }

    fn begin(&mut self, uri: &str, value: Option<u8>) -> Option<Operation> {
        let entry = self.entries.get_mut(uri)?;
        self.serial = self.serial.wrapping_add(1);
        let flight = value.map_or(Flight::Read(self.serial), |value| Flight::Write {
            token: self.serial,
            value,
        });
        entry.flight = Some(flight);
        self.due.remove(uri);
        Some(Operation {
            kind: entry.kind,
            id: entry.id.clone(),
            flight,
        })
    }

    fn set(&mut self, uri: &str, value: u8) -> Option<Operation> {
        if value > 5 {
            return None;
        }
        let entry = self.entries.get_mut(uri)?;
        entry.shown = value;
        entry.desired = Some(value);
        entry.attempts = 0;
        self.failures.remove(uri);
        if matches!(entry.flight, Some(Flight::Write { .. })) {
            return None;
        }
        // A pre-edit read may finish later, but its token no longer owns the value.
        self.begin(uri, Some(value))
    }

    fn written(
        &mut self,
        uri: &str,
        token: u64,
        result: Result<(), RequestError>,
    ) -> Option<Operation> {
        let entry = self.entries.get_mut(uri)?;
        let Some(Flight::Write {
            token: active,
            value,
        }) = entry.flight
        else {
            return None;
        };
        if active != token {
            return None;
        }
        entry.flight = None;
        if entry.desired != Some(value) {
            return self.begin(uri, self.entries[uri].desired);
        }
        if let Err(error) = result {
            if !matches!(error, RequestError::Unconfirmed(_)) {
                entry.desired = None;
                entry.shown = entry.confirmed;
            }
            self.failures.insert(
                uri.into(),
                Failure {
                    name: entry.name.clone(),
                    message: error.to_string(),
                    value,
                },
            );
        }
        self.begin(uri, None)
    }

    fn received(&mut self, uri: &str, token: u64, result: Result<u8, String>) {
        let Some(entry) = self.entries.get_mut(uri) else {
            return;
        };
        if !matches!(entry.flight, Some(Flight::Read(active)) if active == token) {
            return;
        }
        entry.flight = None;
        if let Ok(value) = result {
            if entry.desired.is_none_or(|desired| desired == value) {
                entry.confirmed = value;
                entry.shown = value;
                if entry.desired.take().is_some() {
                    self.failures.remove(uri);
                }
                entry.attempts = 0;
                return;
            }
        } else if entry.desired.is_none() {
            // A failed refresh cannot erase the last known rating.
            return;
        }
        entry.attempts = entry.attempts.saturating_add(1).min(4);
        if entry.attempts < 4 {
            self.due.insert(
                uri.into(),
                Instant::now() + Duration::from_millis(250 << entry.attempts),
            );
        } else {
            self.failures.entry(uri.into()).or_insert_with(|| Failure {
                name: entry.name.clone(),
                message: "The server has not confirmed this rating.".into(),
                value: entry.shown,
            });
        }
    }
}

impl App {
    /// Resolve metadata once when opening a menu, never clone a collection per frame.
    pub(crate) fn prepare_rating(&mut self, uri: &str) {
        if !self.ratings.entries.contains_key(uri) {
            let Some(item) = self.favorite_item(uri) else {
                return;
            };
            let (kind, id, name, value) = match item {
                Item::Song(s) => (Kind::Song, s.id, s.title, s.user_rating),
                Item::Album(a) => (Kind::Album, a.id, a.name, a.user_rating),
                Item::Artist(_) => return,
            };
            let value = value.filter(|v| *v <= 5).unwrap_or(0);
            self.ratings.entries.insert(
                uri.into(),
                Entry {
                    kind,
                    id,
                    name,
                    confirmed: value,
                    shown: value,
                    desired: None,
                    flight: None,
                    attempts: 0,
                    durable: false,
                },
            );
        }
        if self.ratings.entries[uri].flight.is_none()
            && !self.ratings.entries[uri].durable
            && !self.offline()
        {
            let op = self.ratings.begin(uri, None);
            self.send_rating(op);
        }
    }

    pub(super) fn rating_action(&mut self, action: Action) {
        if !self.demo && self.client.is_some() {
            let (uri, value) = match action {
                Action::Set { uri, value } => (uri, value),
                Action::Retry(uri) => {
                    if let Some(entry) = self.ratings.entries.get(&uri) {
                        self.retry_sync(Some(format!("rating:{:?}:{}", entry.kind, entry.id)));
                    }
                    return;
                }
            };
            if value > 5 {
                return;
            }
            if !self.ratings.entries.contains_key(&uri) {
                self.prepare_rating(&uri);
            }
            let Some(e) = self.ratings.entries.get(&uri) else {
                return;
            };
            let (kind, id, name) = (e.kind, e.id.clone(), e.name.clone());
            if self.enqueue_sync(crate::navidrome::sync::Work::Rating {
                kind,
                id: id.clone(),
                name: name.clone(),
                value,
            }) {
                self.ratings.hold(kind, id, name, value, true);
            }
            return;
        }

        let op = match action {
            Action::Set { uri, value } => {
                if !self.ratings.entries.contains_key(&uri) {
                    self.prepare_rating(&uri);
                }
                self.ratings.set(&uri, value)
            }
            Action::Retry(uri) => {
                let Some(value) = self.ratings.failures.get(&uri).map(|f| f.value) else {
                    return;
                };
                self.ratings.set(&uri, value)
            }
        };
        self.send_rating(op);
    }

    fn send_rating(&mut self, operation: Option<Operation>) {
        let Some(Operation { kind, id, flight }) = operation else {
            return;
        };
        if let Some(client) = &self.client {
            let session = self.session;
            let client = client.clone();
            self.backend.send(match flight {
                Flight::Write {
                    token: request,
                    value,
                } => Request::RatingWrite {
                    session,
                    request,
                    client,
                    kind,
                    id,
                    value,
                },
                Flight::Read(request) => Request::RatingRead {
                    session,
                    request,
                    client,
                    kind,
                    id,
                },
            });
        } else if self.demo {
            let uri = kind.uri(&id);
            match flight {
                Flight::Write { token, .. } => {
                    let op = self.ratings.written(&uri, token, Ok(()));
                    self.send_rating(op);
                }
                Flight::Read(token) => {
                    let value = self.ratings.entries[&uri].shown;
                    self.ratings.received(&uri, token, Ok(value));
                }
            }
        }
    }

    pub(super) fn rating_event(&mut self, event: Event) {
        match event {
            Event::RatingWritten {
                session,
                request,
                kind,
                id,
                result,
            } if session == self.session => {
                let op = self.ratings.written(&kind.uri(&id), request, result);
                self.send_rating(op);
            }
            Event::RatingRead {
                session,
                request,
                kind,
                id,
                result,
            } if session == self.session => {
                self.ratings.received(&kind.uri(&id), request, result);
            }
            _ => {}
        }
    }

    pub(super) fn poll_ratings(&mut self, host: &impl super::Host) {
        let now = Instant::now();
        let due: Vec<_> = self
            .ratings
            .due
            .iter()
            .filter(|(_, at)| **at <= now)
            .map(|(uri, _)| uri.clone())
            .collect();
        for uri in due {
            let op = self.ratings.begin(&uri, None);
            self.send_rating(op);
        }
        if let Some(at) = self.ratings.due.values().min() {
            host.repaint_after(at.saturating_duration_since(now));
        }
    }
}

impl Kind {
    pub fn uri(self, id: &str) -> String {
        use crate::navidrome::api::FavoriteKind;
        match self {
            Self::Song => FavoriteKind::Song,
            Self::Album => FavoriteKind::Album,
        }
        .uri(id)
    }
}

#[cfg(test)]
mod tests;

#[cfg(feature = "navidrome-demo")]
impl App {
    pub(super) fn demo_rating(&mut self, uri: &str, error: bool) {
        let Some(op) = self.ratings.set(uri, 4) else {
            return;
        };
        let Flight::Write { token, .. } = op.flight else {
            return;
        };
        if error {
            let read = self.ratings.written(
                uri,
                token,
                Err(RequestError::Rejected(
                    "Your account could not save this rating.".into(),
                )),
            );
            if let Some(Operation {
                flight: Flight::Read(token),
                ..
            }) = read
            {
                self.ratings.received(uri, token, Ok(0));
            }
        } else {
            self.send_rating(Some(op));
        }
    }
}
