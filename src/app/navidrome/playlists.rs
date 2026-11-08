//! Account-scoped playlist browsing, independent of search requests.
use super::App;
use crate::model::{Dialog, Loadable, Page};
use crate::navidrome::api::{Collection, NamedItem, Playlist, Song};
use crate::navidrome::backend::{Event, Request};
use crate::navidrome::playlists::{Edit, Outcome, Task, same_songs};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub enum Action {
    Open(Vec<Song>),
    LoadDestinations,
    Toggle(String),
    Add,
    Create(String),
    Retry(usize),
    Dismiss(usize),
    Remove(Vec<usize>),
    ConfirmDelete,
    Delete,
}

pub struct Job {
    pub name: String,
    pub task: Task,
    pub pending: bool,
    pub error: Option<String>,
    pub dismissed: bool,
    rollback: Option<Playlist>,
    temporary: Option<String>,
}

#[derive(Default)]
pub struct State {
    pub list: Loadable<Vec<NamedItem>>,
    pub refreshing: bool,
    pub error: Option<String>,
    request: u64,
    pub destinations: Loadable<Vec<Playlist>>,
    pub selected: HashSet<String>,
    pub songs: Vec<Song>,
    pub jobs: Vec<Job>,
    pub deleting: Option<Playlist>,
    // Keep local intent visible until a read actually observes it.
    overlays: HashMap<String, Option<Playlist>>,
    readback: Option<(Page, Instant)>,
    backoff: u32,
    creation: u64,
}

impl App {
    pub(super) fn playlist_library_visible(&self) -> bool {
        self.connected
            && (matches!(self.presentation.page(), Page::Home | Page::Playlists)
                || self.presentation.settings.sidebar_visible
                    && self.presentation.settings.library_content
                        != crate::settings::LibraryContent::Albums)
    }

    pub fn can_create_playlist(&self) -> bool {
        !self.presentation.navidrome_playlists.jobs.iter().any(|j| {
            !j.dismissed
                && (j.pending || j.error.is_some())
                && matches!(j.task.edit, Edit::Create { .. })
        })
    }

    pub fn playlist_destination_writable(&self, playlist: &Playlist) -> bool {
        playlist.writable(self.client.as_ref().map_or("Demo", |c| c.username()))
    }

    pub fn playlist_writable(&self) -> bool {
        matches!(
            self.presentation.navidrome_search.collection.get(),
            Some(Collection::Playlist { writable: true, .. })
        )
    }

    pub(super) fn pending_playlist(&self, id: &str) -> bool {
        self.presentation.navidrome_playlists.jobs.iter().any(|j| {
            j.temporary.as_deref() == Some(id) || (j.pending && j.task.edit.id() == Some(id))
        })
    }

    fn current_playlist(&self) -> Option<Playlist> {
        let Page::Playlist(id) = self.presentation.page() else {
            return None;
        };
        let Collection::Playlist {
            name,
            songs,
            cover_art,
            owner,
            writable,
        } = self.presentation.navidrome_search.collection.get()?
        else {
            return None;
        };
        Some(Playlist {
            id: super::decode(id)?,
            name: name.clone(),
            songs: songs.clone(),
            cover_art: cover_art.clone(),
            owner: owner.clone(),
            readonly: Some(!writable),
            changed: None,
        })
    }

    pub(super) fn playlist_action(&mut self, action: Action) {
        match action {
            Action::Open(songs) => {
                let state = &mut self.presentation.navidrome_playlists;
                if state
                    .jobs
                    .iter()
                    .all(|j| j.dismissed || (!j.pending && j.error.is_none()))
                {
                    state.jobs.clear();
                }
                state.songs = songs;
                state.selected.clear();
                self.presentation.dialog = Some(Dialog::Playlists);
                self.playlist_action(Action::LoadDestinations);
            }
            Action::LoadDestinations => {
                let state = &mut self.presentation.navidrome_playlists;
                if state.destinations.is_loading() {
                    return;
                }
                state.destinations = Loadable::Loading;
                if let Some(client) = &self.client {
                    self.backend.send(Request::PlaylistDestinations {
                        session: self.session,
                        client: client.clone(),
                    });
                }
                #[cfg(feature = "navidrome-demo")]
                if self.demo && self.client.is_none() {
                    self.populate_demo_playlists(&[]);
                    self.presentation.navidrome_playlists.destinations = Loadable::Loaded(
                        self.presentation
                            .navidrome_playlists
                            .list
                            .get()
                            .unwrap()
                            .iter()
                            .map(|p| Playlist {
                                id: p.id.clone(),
                                name: p.name.clone(),
                                readonly: Some(p.id == "list2"),
                                owner: Some("Demo".into()),
                                ..Default::default()
                            })
                            .collect(),
                    );
                }
            }
            Action::Toggle(id) => {
                let selected = &mut self.presentation.navidrome_playlists.selected;
                if !selected.remove(&id) {
                    selected.insert(id);
                }
            }
            Action::Add => {
                let state = &mut self.presentation.navidrome_playlists;
                let destinations: Vec<_> = state
                    .destinations
                    .get()
                    .into_iter()
                    .flatten()
                    .filter(|p| state.selected.contains(&p.id))
                    .map(|p| (p.id.clone(), p.name.clone()))
                    .collect();
                let songs: std::sync::Arc<[Song]> = state.songs.clone().into();
                state.selected.clear();
                for (id, name) in destinations {
                    self.start_playlist_job(
                        name,
                        Edit::Append {
                            id,
                            songs: songs.clone(),
                        },
                    );
                }
            }
            Action::Create(name) if !name.trim().is_empty() => {
                let songs = self.presentation.navidrome_playlists.songs.clone().into();
                self.start_playlist_job(
                    name.trim().into(),
                    Edit::Create {
                        name: name.trim().into(),
                        songs,
                    },
                );
            }
            Action::Retry(job) => self.send_playlist_job(job),
            Action::Dismiss(job) => {
                if let Some(j) = self.presentation.navidrome_playlists.jobs.get_mut(job)
                    && !j.pending
                    && !j.task.sent
                {
                    j.dismissed = true;
                }
            }
            Action::Remove(positions) => {
                if let Some(p) = self
                    .current_playlist()
                    .filter(|p| p.readonly == Some(false))
                {
                    self.start_playlist_job(
                        p.name.clone(),
                        Edit::Remove {
                            id: p.id,
                            before: p.songs.iter().map(|s| s.id.clone()).collect(),
                            positions,
                        },
                    );
                }
            }
            Action::ConfirmDelete => {
                if let Some(p) = self
                    .current_playlist()
                    .filter(|p| p.readonly == Some(false))
                {
                    self.presentation.navidrome_playlists.deleting = Some(p);
                    self.presentation.dialog = Some(Dialog::DeletePlaylist);
                }
            }
            Action::Delete => {
                if let Some(p) = self.presentation.navidrome_playlists.deleting.take() {
                    self.presentation.dialog = None;
                    self.start_playlist_job(p.name, Edit::Delete { id: p.id });
                    self.presentation.open(Page::Playlists);
                }
            }
            Action::Create(_) => {}
        }
    }

    fn start_playlist_job(&mut self, name: String, edit: Edit) {
        if edit.id().is_some_and(|id| self.pending_playlist(id)) {
            return;
        }
        let state = &self.presentation.navidrome_playlists;
        if state.jobs.iter().any(|j| {
            !j.dismissed && (j.pending || j.error.is_some()) && j.task.edit.id() == edit.id()
        }) {
            return;
        }
        let rollback = self
            .current_playlist()
            .filter(|p| Some(p.id.as_str()) == edit.id());
        let state = &mut self.presentation.navidrome_playlists;
        let job = state.jobs.len();
        let temporary = matches!(edit, Edit::Create { .. }).then(|| {
            state.creation = state.creation.wrapping_add(1);
            format!("navifast-pending:{}:{}", self.session, state.creation)
        });
        if let Some((id, preview)) = preview(&edit, rollback.as_ref(), temporary.as_deref()) {
            if temporary.is_some() && state.list.get().is_none() {
                state.list = Loadable::Loaded(Vec::new());
            }
            state.overlays.insert(id, preview);
        }
        state.jobs.push(Job {
            name,
            task: Task::new(edit),
            pending: false,
            error: None,
            dismissed: false,
            rollback,
            temporary,
        });
        self.apply_playlist_overlays();
        self.send_playlist_job(job);
    }

    fn send_playlist_job(&mut self, job: usize) {
        let current = self.current_playlist();
        let Some(state) = self.presentation.navidrome_playlists.jobs.get_mut(job) else {
            return;
        };
        if state.pending || state.dismissed || (state.task.sent && state.error.is_none()) {
            return;
        }
        let retrying = state.error.is_some() && !state.task.sent;
        if retrying
            && matches!(state.task.edit, Edit::Append { .. })
            && let Some(current) = current.filter(|p| state.task.edit.id() == Some(p.id.as_str()))
        {
            state.rollback = Some(current);
        }
        state.pending = true;
        state.error = None;
        let task = state.task.clone();
        let rollback = state.rollback.clone();
        let temporary = state.temporary.clone();
        if retrying {
            let state = &mut self.presentation.navidrome_playlists;
            if let Some((id, preview)) =
                preview(&task.edit, rollback.as_ref(), temporary.as_deref())
            {
                state.overlays.insert(id, preview);
            }
            self.apply_playlist_overlays();
        }
        if let Some(client) = &self.client {
            self.backend.send(Request::EditPlaylist {
                session: self.session,
                job,
                client: client.clone(),
                task,
            });
        }
    }

    pub(super) fn reconcile_playlist_view(&mut self) {
        if let Some(p) = self.current_playlist() {
            let state = &mut self.presentation.navidrome_playlists;
            if state
                .overlays
                .get(&p.id)
                .and_then(Option::as_ref)
                .is_some_and(|expected| same_songs(&expected.songs, &p.songs))
            {
                state.overlays.remove(&p.id);
            }
        }
        self.apply_playlist_overlays();
        self.schedule_playlist_readback();
    }

    fn schedule_playlist_readback(&mut self) {
        let page = self.presentation.page().clone();
        let state = &mut self.presentation.navidrome_playlists;
        if let Page::Playlist(id) = &page
            && let Some(id) = super::decode(id)
            && state.overlays.contains_key(&id)
            && !state
                .jobs
                .iter()
                .any(|j| j.temporary.as_deref() == Some(&id))
        {
            let delay = Duration::from_millis(500 * (1_u64 << state.backoff.min(6)));
            state.readback = Some((page, Instant::now() + delay));
            state.backoff = state.backoff.saturating_add(1);
        } else {
            state.readback = None;
            state.backoff = 0;
        }
    }

    pub(super) fn poll_playlists(&mut self, host: &impl super::Host) {
        let state = &mut self.presentation.navidrome_playlists;
        let Some((page, when)) = state.readback.clone() else {
            return;
        };
        if self.client.is_none() || self.presentation.page() != &page {
            self.presentation.navidrome_playlists.readback = None;
            return;
        }
        if Instant::now() >= when {
            self.presentation.navidrome_playlists.readback = None;
            self.fetch_collection(&page);
        } else {
            host.repaint_after(when.saturating_duration_since(Instant::now()));
        }
    }

    pub(super) fn apply_playlist_overlays(&mut self) {
        let state = &mut self.presentation.navidrome_playlists;
        if let Some(list) = state.list.get_mut() {
            for (id, p) in &state.overlays {
                if let Some(p) = p {
                    if !list.iter().any(|v| &v.id == id) {
                        list.push(p.named());
                    }
                } else {
                    list.retain(|v| &v.id != id);
                }
            }
        }
        if let Some(destinations) = state.destinations.get_mut() {
            for (id, p) in &state.overlays {
                if let Some(p) = p {
                    if !id.starts_with("navifast-pending:")
                        && !destinations.iter().any(|v| &v.id == id)
                    {
                        destinations.push(p.metadata());
                    }
                } else {
                    destinations.retain(|v| &v.id != id);
                }
            }
        }
        if let Page::Playlist(id) = self.presentation.page()
            && let Some(raw) = super::decode(id)
            && let Some(p) = self.presentation.navidrome_playlists.overlays.get(&raw)
        {
            self.presentation.navidrome_search.collection = match p {
                Some(p) => Loadable::Loaded(
                    p.clone()
                        .collection(self.client.as_ref().map_or("Demo", |c| c.username())),
                ),
                None => Loadable::Failed("This playlist has been deleted.".into()),
            };
        }
    }
    pub(super) fn cancel_playlists(&mut self) {
        self.backend.send(Request::CancelPlaylistLibrary);
        let state = &mut self.presentation.navidrome_playlists;
        state.request = state.request.wrapping_add(1);
        state.refreshing = false;
        if state.list.is_loading() {
            state.list = Loadable::NotLoaded;
        }
    }

    pub(super) fn fetch_playlists(&mut self) {
        self.cancel_playlists();
        let state = &mut self.presentation.navidrome_playlists;
        state.error = None;
        state.refreshing = true;
        if state.list.get().is_none() {
            state.list = Loadable::Loading;
        }
        #[cfg(feature = "navidrome-demo")]
        if self.demo && self.client.is_none() {
            self.populate_demo_playlists(&[]);
            return;
        }
        if let Some(client) = &self.client {
            self.backend.send(Request::PlaylistLibrary {
                session: self.session,
                request: state.request,
                client: client.clone(),
            });
        }
    }

    pub(super) fn playlists_event(&mut self, event: Event) {
        let browsing = self.playlist_library_visible();
        let state = &mut self.presentation.navidrome_playlists;
        match event {
            Event::PlaylistLibrary {
                session,
                request,
                result,
            } if session == self.session && request == state.request && browsing => {
                let state = &mut self.presentation.navidrome_playlists;
                state.refreshing = false;
                state.error = result.as_ref().err().cloned();
                if let Ok(list) = &result {
                    state.overlays.retain(|id, p| {
                        p.is_some()
                            || list.iter().any(|p| &p.id == id)
                            || state
                                .jobs
                                .iter()
                                .any(|j| j.pending && j.task.edit.id() == Some(id))
                    });
                }
                state.list.refresh(result);
            }
            Event::PlaylistDestinations { session, result } if session == self.session => {
                state.destinations = Loadable::from_result(result);
            }
            Event::PlaylistEdited {
                session,
                job,
                task,
                result,
            } if session == self.session => {
                let Some(j) = state.jobs.get_mut(job) else {
                    return;
                };
                if !j.pending {
                    return;
                }
                j.pending = false;
                j.task = *task;
                j.error = result.as_ref().err().cloned();
                if result.is_ok() {
                    j.task.release_data();
                }
                match result {
                    Ok(Outcome::Saved(p)) => {
                        if let Some(id) = j.temporary.take() {
                            state.overlays.remove(&id);
                            if let Some(list) = state.list.get_mut() {
                                list.retain(|p| p.id != id);
                            }
                            for page in &mut self.presentation.history {
                                if page == &Page::Playlist(super::encode(&id)) {
                                    *page = Page::Playlist(super::encode(&p.id));
                                }
                            }
                            if self.presentation.navidrome_search.collection_page
                                == Some(Page::Playlist(super::encode(&id)))
                            {
                                self.presentation.navidrome_search.collection_page =
                                    Some(Page::Playlist(super::encode(&p.id)));
                            }
                        }
                        state.overlays.insert(p.id.clone(), Some(p.clone()));
                        if let Some(destinations) = state.destinations.get_mut() {
                            if let Some(old) = destinations.iter_mut().find(|old| old.id == p.id) {
                                *old = p.metadata();
                            } else {
                                destinations.push(p.metadata());
                            }
                        }
                        j.rollback = None;
                    }
                    Ok(Outcome::Deleted(id)) => {
                        self.preferences.home_history.collections.retain(|entry| {
                            entry.kind != super::home::Kind::Playlist || entry.id != id
                        });
                        self.dirty.get_or_insert_with(Instant::now);
                        state.overlays.insert(id.clone(), None);
                        if let Some(destinations) = state.destinations.get_mut() {
                            destinations.retain(|p| p.id != id);
                        }
                        j.rollback = None;
                    }
                    Err(_) if !j.task.sent => {
                        if let Some(p) = j.rollback.clone() {
                            state.overlays.remove(&p.id);
                            if let Some(list) = state.list.get_mut()
                                && !list.iter().any(|item| item.id == p.id)
                            {
                                list.push(p.named());
                            }
                            if let Some(destinations) = state.destinations.get_mut()
                                && !destinations.iter().any(|item| item.id == p.id)
                            {
                                destinations.push(p.metadata());
                            }
                            if self.presentation.navidrome_search.collection_page
                                == Some(Page::Playlist(super::encode(&p.id)))
                            {
                                self.presentation.navidrome_search.collection =
                                    Loadable::Loaded(p.collection(
                                        self.client.as_ref().map_or("Demo", |c| c.username()),
                                    ));
                            }
                        }
                        if let Some(id) = &j.temporary {
                            state.overlays.remove(id);
                            if let Some(list) = state.list.get_mut() {
                                list.retain(|p| &p.id != id);
                            }
                            if self.presentation.navidrome_search.collection_page
                                == Some(Page::Playlist(super::encode(id)))
                            {
                                self.presentation.navidrome_search.collection =
                                    Loadable::Failed(j.error.clone().expect("rejected create"));
                            }
                        }
                    }
                    Err(_) => {
                        if let Some(p) = &j.task.expected {
                            state.overlays.insert(p.id.clone(), Some(p.clone()));
                        }
                    }
                }
            }
            _ => return,
        }
        self.apply_playlist_overlays();
        self.play_requested_playlist();
        let page = self.presentation.page().clone();
        if self.presentation.navidrome_search.collection.is_loading()
            && let Page::Playlist(id) = &page
            && super::decode(id).is_some_and(|id| !self.pending_playlist(&id))
        {
            self.fetch_collection(&page);
        }
        self.schedule_playlist_readback();
    }

    #[cfg(feature = "navidrome-demo")]
    pub(super) fn populate_demo_playlists(&mut self, flags: &[&str]) {
        let state = &mut self.presentation.navidrome_playlists;
        state.refreshing = flags.contains(&"loading");
        state.error = flags
            .contains(&"error")
            .then(|| "The server did not respond in time.".into());
        state.list = if state.refreshing {
            Loadable::Loading
        } else if let Some(error) = &state.error {
            Loadable::Failed(error.clone())
        } else {
            Loadable::Loaded(if flags.contains(&"empty") {
                Vec::new()
            } else {
                ["Evening drive", "Bonobo favorites", "Evening drive"]
                    .into_iter()
                    .enumerate()
                    .map(|(i, name)| NamedItem {
                        id: format!("list{i}"),
                        name: name.into(),
                        cover_art: if i < 2 {
                            self.albums.get(i).and_then(|a| a.cover_art.clone())
                        } else {
                            None
                        },
                    })
                    .collect()
            })
        };
    }
}

fn preview(
    edit: &Edit,
    before: Option<&Playlist>,
    temporary: Option<&str>,
) -> Option<(String, Option<Playlist>)> {
    match edit {
        Edit::Create { name, songs } => {
            let id = temporary?.to_owned();
            Some((
                id.clone(),
                Some(Playlist {
                    id,
                    name: name.clone(),
                    songs: songs.to_vec(),
                    readonly: Some(false),
                    ..Default::default()
                }),
            ))
        }
        Edit::Delete { id } => Some((id.clone(), None)),
        _ => {
            let mut p = before?.clone();
            match edit {
                Edit::Append { songs, .. } => p.songs.extend(songs.iter().cloned()),
                Edit::Remove { positions, .. } => {
                    let positions: HashSet<_> = positions.iter().copied().collect();
                    let mut i = 0;
                    p.songs.retain(|_| {
                        let keep = !positions.contains(&i);
                        i += 1;
                        keep
                    });
                }
                _ => unreachable!(),
            }
            Some((p.id.clone(), Some(p)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::navidrome::{Action, tests::with_app};
    use crate::model::Action as UiAction;
    use crate::navidrome::{
        api::{Client, Collection},
        test_server::{Server, response},
    };
    use crate::settings::ProxyConfig;

    #[test]
    fn optimistic_rows_survive_lagging_reads_and_known_failure_restores_the_original() {
        with_app(|app, _| {
            app.demo = false;
            let original = Playlist {
                id: "list".into(),
                name: "Mix".into(),
                readonly: Some(false),
                songs: app.album.as_ref().unwrap().song.clone(),
                ..Default::default()
            };
            let page = Page::Playlist(super::super::encode(&original.id));
            app.presentation.open(page.clone());
            app.presentation.navidrome_search.collection_page = Some(page);
            app.presentation.navidrome_search.collection =
                Loadable::Loaded(original.clone().collection("listener"));
            app.playlist_action(super::Action::Remove(vec![2]));
            assert_eq!(app.current_playlist().unwrap().songs.len(), 2);
            app.presentation.navidrome_search.collection =
                Loadable::Loaded(original.clone().collection("listener"));
            app.reconcile_playlist_view();
            assert_eq!(
                app.current_playlist().unwrap().songs.len(),
                2,
                "lagging reads cannot undo the removal"
            );
            let task = app.presentation.navidrome_playlists.jobs[0].task.clone();
            app.playlists_event(Event::PlaylistEdited {
                session: app.session,
                job: 0,
                task: Box::new(task),
                result: Err("Permission changed".into()),
            });
            assert_eq!(app.current_playlist().unwrap().songs.len(), 3);
            assert!(app.presentation.navidrome_playlists.overlays.is_empty());
            app.playlist_action(super::Action::Retry(0));
            assert_eq!(app.current_playlist().unwrap().songs.len(), 2);
            let mut task = app.presentation.navidrome_playlists.jobs[0].task.clone();
            task.sent = true;
            let confirmed = app.current_playlist().unwrap();
            app.playlists_event(Event::PlaylistEdited {
                session: app.session,
                job: 0,
                task: Box::new(task),
                result: Ok(Outcome::Saved(confirmed.clone())),
            });
            assert_eq!(app.current_playlist().unwrap().songs.len(), 2);
            app.presentation.navidrome_search.collection =
                Loadable::Loaded(original.collection("listener"));
            app.reconcile_playlist_view();
            assert_eq!(app.current_playlist().unwrap().songs.len(), 2);
            app.presentation.navidrome_search.collection =
                Loadable::Loaded(confirmed.collection("listener"));
            app.reconcile_playlist_view();
            assert!(
                app.presentation.navidrome_playlists.overlays.is_empty(),
                "release the checkpoint once observed"
            );
        });
    }

    #[test]
    fn created_rows_reconcile_identity_and_deleted_rows_do_not_reappear_during_refresh() {
        with_app(|app, host| {
            app.demo = false;
            app.presentation.open(Page::Playlists);
            let old = Playlist {
                id: "old".into(),
                name: "Mix".into(),
                readonly: Some(false),
                ..Default::default()
            };
            app.presentation.navidrome_playlists.list = Loadable::Loaded(vec![old.named()]);
            app.playlist_action(super::Action::Create("Mix".into()));
            let temporary = app.presentation.navidrome_playlists.jobs[0]
                .temporary
                .clone()
                .unwrap();
            assert_eq!(
                app.presentation
                    .navidrome_playlists
                    .list
                    .get()
                    .unwrap()
                    .len(),
                2
            );
            app.ui_action(
                UiAction::Open(Page::Playlist(super::super::encode(&temporary))),
                host,
            );
            assert_eq!(app.current_playlist().unwrap().name, "Mix");
            let mut task = app.presentation.navidrome_playlists.jobs[0].task.clone();
            task.sent = true;
            let new = Playlist {
                id: "new".into(),
                ..old.clone()
            };
            app.playlists_event(Event::PlaylistEdited {
                session: app.session,
                job: 0,
                task: Box::new(task),
                result: Ok(Outcome::Saved(new.clone())),
            });
            assert_eq!(app.current_playlist().unwrap().id, "new");
            assert!(
                !app.presentation
                    .navidrome_playlists
                    .list
                    .get()
                    .unwrap()
                    .iter()
                    .any(|p| p.id == temporary)
            );
            app.playlist_action(super::Action::ConfirmDelete);
            assert_eq!(
                app.presentation
                    .navidrome_playlists
                    .deleting
                    .as_ref()
                    .unwrap()
                    .id,
                "new"
            );
            app.playlist_action(super::Action::Delete);
            assert_eq!(app.presentation.page(), &Page::Playlists);
            assert_eq!(
                app.presentation
                    .navidrome_playlists
                    .list
                    .get()
                    .unwrap()
                    .len(),
                1
            );
            let request = app.presentation.navidrome_playlists.request;
            app.playlists_event(Event::PlaylistLibrary {
                session: app.session,
                request,
                result: Ok(vec![old.named(), new.named()]),
            });
            assert_eq!(
                app.presentation.navidrome_playlists.list.get().unwrap()[0].id,
                "old"
            );
            assert_eq!(
                app.presentation
                    .navidrome_playlists
                    .list
                    .get()
                    .unwrap()
                    .len(),
                1
            );
            let task = app.presentation.navidrome_playlists.jobs[1].task.clone();
            app.playlists_event(Event::PlaylistEdited {
                session: app.session,
                job: 1,
                task: Box::new(task),
                result: Err("Deletion rejected".into()),
            });
            assert_eq!(
                app.presentation
                    .navidrome_playlists
                    .list
                    .get()
                    .unwrap()
                    .len(),
                2,
                "a rejected deletion restores the browser row"
            );
            app.playlist_action(super::Action::Retry(1));
            assert_eq!(
                app.presentation
                    .navidrome_playlists
                    .list
                    .get()
                    .unwrap()
                    .len(),
                1
            );
            let session = app.session;
            let task = app.presentation.navidrome_playlists.jobs[1].task.clone();
            app.apply(Action::SignOut);
            app.playlists_event(Event::PlaylistEdited {
                session,
                job: 1,
                task: Box::new(task),
                result: Ok(Outcome::Deleted("new".into())),
            });
            assert!(app.presentation.navidrome_playlists.jobs.is_empty());
            assert!(app.presentation.navidrome_playlists.overlays.is_empty());
        });
    }

    #[test]
    fn accessible_playlists_preserve_ids_order_and_duplicates_and_retry_without_playing() {
        with_app(|app, host| {
            let mut transient = true;
            let server = Server::spawn(move |request| {
                let body = if request.starts_with("GET /rest/getOpenSubsonicExtensions.view?") {
                    r#"{"status":"ok","openSubsonicExtensions":[]}"#
                } else if request.starts_with("GET /rest/getPlaylists.view?") {
                    r#"{"status":"ok","playlists":{"playlist":[{"id":"shared:/1","name":"Mix","owner":"other","public":true},{"id":"smart:/2","name":"Mix","smart":true}]}}"#
                } else if request.contains("id=shared%3A%2F1") {
                    r#"{"status":"ok","playlist":{"name":"Mix","entry":[{"id":"a","title":"First"},{"id":"b","title":"Middle"},{"id":"a","title":"First"},{"id":"c","title":"Successor"}]}}"#
                } else if request.contains("id=smart%3A%2F2") {
                    r#"{"status":"ok","playlist":{"name":"Mix"}}"#
                } else if request.contains("id=removed") {
                    r#"{"status":"failed","error":{"code":70}}"#
                } else if request.contains("id=retry") {
                    if std::mem::take(&mut transient) {
                        return response("503 Service Unavailable", "", b"");
                    }
                    r#"{"status":"ok","playlist":{"name":"Recovered"}}"#
                } else {
                    panic!("unexpected playlist fixture endpoint");
                };
                response(
                    "200 OK",
                    "",
                    format!(r#"{{"subsonic-response":{body}}}"#).as_bytes(),
                )
            });
            app.set_client(
                Client::new(&server.url, "listener", "dummy".into(), &ProxyConfig::Off).unwrap(),
            );
            let receive = |app: &mut App| loop {
                let event = app
                    .backend
                    .events
                    .recv_timeout(std::time::Duration::from_secs(5))
                    .unwrap();
                if let Event::StreamingCapabilities { result, .. } = &event {
                    assert!(result.is_ok());
                    continue;
                }
                if matches!(event, Event::PlaylistLibrary { .. }) {
                    app.playlists_event(event);
                } else {
                    app.search_event(event);
                }
                break;
            };
            app.ui_action(UiAction::Open(Page::Playlists), host);
            receive(app);
            let list = app.presentation.navidrome_playlists.list.get().unwrap();
            assert_eq!(
                list.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
                ["shared:/1", "smart:/2"]
            );
            assert_eq!(list[0].name, list[1].name);
            app.ui_action(
                UiAction::Open(Page::Playlist(super::super::encode("shared:/1"))),
                host,
            );
            receive(app);
            let client = app.client.take(); // Do not open a real audio device in this fixture.
            app.search_action(super::super::search::Action::PlayEntry(2), host);
            assert_eq!(
                app.transport
                    .queue
                    .context()
                    .iter()
                    .map(|s| s.id.as_str())
                    .collect::<Vec<_>>(),
                ["a", "b", "a", "c"]
            );
            assert_eq!(app.transport.queue.context_index(), Some(2));
            app.apply(Action::Next);
            assert_eq!(app.transport.song().unwrap().id, "c");
            app.client = client;
            let generation = app.transport.generation;
            for id in ["smart:/2", "removed", "retry"] {
                app.ui_action(
                    UiAction::Open(Page::Playlist(super::super::encode(id))),
                    host,
                );
                receive(app);
                match &app.presentation.navidrome_search.collection {
                    Loadable::Loaded(Collection::Playlist { songs, .. }) if id == "smart:/2" => {
                        assert!(songs.is_empty())
                    }
                    Loadable::Failed(error) if id == "removed" => {
                        assert!(error.contains("no longer available"))
                    }
                    Loadable::Failed(error) if id == "retry" => {
                        assert!(!error.contains("no longer available"))
                    }
                    _ => panic!("incorrect playlist load state"),
                }
                assert_eq!(app.transport.generation, generation);
            }
            app.ui_action(UiAction::Reload(app.presentation.page().clone()), host);
            receive(app);
            assert!(
                matches!(app.presentation.navidrome_search.collection.get(), Some(Collection::Playlist { name, .. }) if name == "Recovered")
            );
            assert_eq!(app.transport.generation, generation);
        });
    }

    #[test]
    fn sidebar_consumes_playlist_reads_across_pages_and_rejects_cancelled_or_old_account_results() {
        use crate::settings::LibraryContent;
        with_app(|app, host| {
            app.demo = false;
            app.ui_action(UiAction::Open(Page::Albums), host);
            let session = app.session;
            let event = |request, name: &str| Event::PlaylistLibrary {
                session,
                request,
                result: Ok(vec![NamedItem {
                    id: "mix".into(),
                    name: name.into(),
                    cover_art: None,
                }]),
            };
            app.ui_action(UiAction::CycleLibraryContent, host);
            assert_eq!(
                app.presentation.settings.library_content,
                LibraryContent::Playlists
            );
            let first = app.presentation.navidrome_playlists.request;
            app.ui_action(UiAction::Open(Page::Settings), host);
            assert_eq!(app.presentation.navidrome_playlists.request, first);
            app.playlists_event(event(first, "Current"));
            assert_eq!(
                app.presentation.navidrome_playlists.list.get().unwrap()[0].name,
                "Current"
            );
            app.ui_action(UiAction::CycleLibraryContent, host);
            assert_eq!(
                app.presentation.navidrome_playlists.request, first,
                "All keeps the same request and rows"
            );
            app.ui_action(UiAction::Reload(Page::Playlists), host);
            let second = app.presentation.navidrome_playlists.request;
            app.ui_action(UiAction::CycleLibraryContent, host);
            assert_eq!(
                app.presentation.settings.library_content,
                LibraryContent::Albums
            );
            app.playlists_event(event(second, "Cancelled"));
            assert_eq!(
                app.presentation.navidrome_playlists.list.get().unwrap()[0].name,
                "Current"
            );
            app.ui_action(UiAction::CycleLibraryContent, host);
            let third = app.presentation.navidrome_playlists.request;
            assert!(app.presentation.navidrome_playlists.refreshing);
            assert_eq!(
                app.presentation.navidrome_playlists.list.get().unwrap()[0].name,
                "Current"
            );
            app.ui_action(UiAction::ToggleSidebar, host);
            app.playlists_event(event(third, "Hidden"));
            assert_eq!(
                app.presentation.navidrome_playlists.list.get().unwrap()[0].name,
                "Current"
            );
            app.ui_action(UiAction::ToggleSidebar, host);
            let fourth = app.presentation.navidrome_playlists.request;
            app.apply(Action::SignOut);
            app.playlists_event(event(fourth, "Other account"));
            assert!(app.presentation.navidrome_playlists.list.get().is_none());
        });
    }

    #[test]
    fn refresh_keeps_rows_on_failure_and_rejects_stale_navigation_and_account_results() {
        with_app(|app, host| {
            app.demo = false;
            app.ui_action(UiAction::Open(Page::Playlists), host);
            let session = app.session;
            let first = app.presentation.navidrome_playlists.request;
            let event = |request, name: &str| Event::PlaylistLibrary {
                session,
                request,
                result: Ok(vec![NamedItem {
                    id: name.into(),
                    name: name.into(),
                    cover_art: None,
                }]),
            };
            app.playlists_event(event(first, "Current"));
            app.ui_action(UiAction::Open(Page::Home), host);
            assert_eq!(
                app.presentation.navidrome_playlists.request, first,
                "Home shares the same active library request"
            );
            app.ui_action(UiAction::Reload(Page::Playlists), host);
            let second = app.presentation.navidrome_playlists.request;
            app.playlists_event(event(first, "Stale"));
            app.playlists_event(Event::PlaylistLibrary {
                session,
                request: second,
                result: Err("Try again".into()),
            });
            let state = &app.presentation.navidrome_playlists;
            assert_eq!(state.list.get().unwrap()[0].name, "Current");
            assert_eq!(state.error.as_deref(), Some("Try again"));
            assert!(!state.refreshing);
            app.ui_action(UiAction::Reload(Page::Playlists), host);
            let third = app.presentation.navidrome_playlists.request;
            app.ui_action(UiAction::Open(Page::Albums), host);
            app.playlists_event(event(third, "Left behind"));
            assert_eq!(
                app.presentation.navidrome_playlists.list.get().unwrap()[0].name,
                "Current"
            );
            app.ui_action(UiAction::Open(Page::Playlists), host);
            let fourth = app.presentation.navidrome_playlists.request;
            app.apply(Action::SignOut);
            app.playlists_event(event(fourth, "Other account"));
            assert!(app.presentation.navidrome_playlists.list.get().is_none());
        });
    }
}
