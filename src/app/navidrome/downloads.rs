//! Optimistic download controls; only the worker touches files or transfer state.
use super::{App, Request};
use crate::{
    model::Page,
    navidrome::{
        api::{Client, Collection, Song},
        downloads::{
            Kind, Library, Selection, Status,
            worker::{Action, Command, Update},
        },
    },
};

#[derive(Clone, Default)]
pub struct Summary {
    pub total: usize,
    pub ready: usize,
    pub failed: usize,
    pub active: usize,
    pub bytes: u64,
}
impl Summary {
    fn replace(&mut self, old: &Status, new: &Status, occurrences: usize) {
        let ready = |s: &Status| usize::from(s.ready()) * occurrences;
        let failed = |s: &Status| usize::from(matches!(s, Status::Failed { .. })) * occurrences;
        let active = |s: &Status| {
            usize::from(matches!(s, Status::Queued | Status::Downloading { .. })) * occurrences
        };
        self.ready = self.ready - ready(old) + ready(new);
        self.failed = self.failed - failed(old) + failed(new);
        self.active = self.active - active(old) + active(new);
        self.bytes = self.bytes - old.bytes() + new.bytes();
    }
}
#[derive(Default)]
pub struct State {
    pub library: Option<Library>,
    pub loading: bool,
    pub error: Option<String>,
    pub songs: Vec<Song>,
    pub order: Vec<String>,
    pub summaries: std::collections::BTreeMap<String, Summary>,
    owners: std::collections::BTreeMap<String, Vec<(String, usize)>>,
    revision: u64,
    pub(super) content_revision: u64,
    pub bytes: u64,
    view: Option<(String, u64)>,
}
impl App {
    pub(crate) fn offline(&self) -> bool {
        self.client.as_ref().is_some_and(Client::is_offline)
    }
    pub(crate) fn download_ready(&self, id: &str) -> bool {
        self.downloads
            .library
            .as_ref()
            .and_then(|l| l.tracks.get(id))
            .is_some_and(|t| t.status.ready())
    }
    pub(super) fn open_downloads(&mut self) {
        self.downloads = Default::default();
        let Some(client) = self.client.clone() else {
            return;
        };
        self.downloads.loading = true;
        if self.demo {
            self.downloads.library = Some(Library::new(client.scope.clone()));
            self.downloads.loading = false;
            return;
        }
        self.download_command(Action::Open {
            root: self.dirs.state.join("downloads"),
            online: !client.is_offline(),
            client,
            proxy: self.proxy.clone(),
        });
    }
    fn download_command(&mut self, action: Action) {
        self.summarize_downloads();
        self.downloads.revision = self.downloads.revision.wrapping_add(1);
        if !self.demo {
            self.backend.send(Request::Downloads(Command {
                session: self.session,
                revision: self.downloads.revision,
                action,
            }));
        }
    }
    pub(super) fn download_update(&mut self, session: u64, revision: u64, update: Update) {
        if session != self.session || revision < self.downloads.revision {
            return;
        }
        match update {
            Update::Library(library) => {
                self.downloads.loading = false;
                self.downloads.library = Some(*library);
                self.downloads.content_revision = self.downloads.content_revision.wrapping_add(1);
            }
            Update::Track { id, status } => {
                if let Some(track) = self
                    .downloads
                    .library
                    .as_mut()
                    .and_then(|l| l.tracks.get_mut(&id))
                {
                    let old = std::mem::replace(&mut track.status, status);
                    let new = &track.status;
                    self.downloads.bytes = self.downloads.bytes - old.bytes() + new.bytes();
                    for (key, occurrences) in self.downloads.owners.get(&id).into_iter().flatten() {
                        if let Some(summary) = self.downloads.summaries.get_mut(key) {
                            summary.replace(&old, new, *occurrences);
                        }
                    }
                }
                return;
            }
            Update::Error(error) => {
                self.downloads.loading = false;
                self.downloads.error = Some(error);
            }
        }
        self.summarize_downloads();
    }
    pub(crate) fn summarize_downloads(&mut self) {
        let Some(library) = &self.downloads.library else {
            return;
        };
        self.downloads.bytes = library.bytes();
        self.downloads.order = library.selections.keys().cloned().collect();
        self.downloads
            .order
            .sort_by_cached_key(|key| library.selections[key].title.to_lowercase());
        self.downloads.owners.clear();
        self.downloads.summaries = library
            .selections
            .iter()
            .map(|(key, selection)| {
                let mut summary = Summary {
                    total: selection.songs.len(),
                    ..Default::default()
                };
                let mut counts = std::collections::BTreeMap::<&String, usize>::new();
                for id in &selection.songs {
                    if let Some(t) = library.tracks.get(id) {
                        summary.ready += usize::from(t.status.ready());
                        summary.failed += usize::from(matches!(t.status, Status::Failed { .. }));
                        summary.active += usize::from(matches!(
                            t.status,
                            Status::Queued | Status::Downloading { .. }
                        ));
                        let occurrences = counts.entry(id).or_default();
                        if *occurrences == 0 {
                            summary.bytes += t.status.bytes();
                        }
                        *occurrences += 1;
                    }
                }
                for (id, count) in counts {
                    self.downloads
                        .owners
                        .entry(id.clone())
                        .or_default()
                        .push((key.clone(), count));
                }
                (key.clone(), summary)
            })
            .collect();
    }
    pub(crate) fn sync_download_page(&mut self) {
        let Page::Download(key) = self.presentation.page() else {
            return;
        };
        if self.downloads.view.as_ref() == Some(&(key.clone(), self.downloads.content_revision)) {
            return;
        }
        self.downloads.songs = self
            .downloads
            .library
            .as_ref()
            .into_iter()
            .flat_map(|l| {
                l.selections
                    .get(key)
                    .into_iter()
                    .flat_map(|s| s.songs.iter())
                    .filter_map(|id| l.tracks.get(id))
            })
            .map(|t| t.song.clone())
            .collect();
        self.downloads.view = Some((key.clone(), self.downloads.content_revision));
    }
    pub(crate) fn keep_download(
        &mut self,
        kind: Kind,
        id: String,
        title: String,
        cover: Option<String>,
        songs: Option<Vec<Song>>,
    ) {
        if self.offline() {
            self.downloads.error = Some("Reconnect to download music.".into());
            return;
        }
        let Some(library) = &mut self.downloads.library else {
            return;
        };
        let selection = Selection {
            kind,
            id,
            title,
            cover,
            songs: Vec::new(),
            paused: false,
            pending: songs.is_none(),
            error: None,
        };
        if let Err(error) = library.keep(selection.clone(), songs.clone()) {
            self.downloads.error = Some(error);
            return;
        }
        self.downloads.error = None;
        self.downloads.content_revision = self.downloads.content_revision.wrapping_add(1);
        self.download_command(Action::Keep { selection, songs });
    }
    pub(crate) fn download_item(&mut self, uri: &str) {
        match self.favorite_item(uri) {
            Some(super::favorites::Item::Song(song)) => self.keep_download(
                Kind::Song,
                song.id.clone(),
                song.title.clone(),
                song.cover_art.clone(),
                Some(vec![song]),
            ),
            Some(super::favorites::Item::Album(album)) => {
                let songs = self
                    .album
                    .as_ref()
                    .filter(|a| a.id == album.id && !self.album_loading)
                    .map(|a| a.song.clone());
                self.keep_download(Kind::Album, album.id, album.name, album.cover_art, songs);
            }
            _ => {}
        }
    }
    pub(crate) fn download_playlist(&mut self) {
        let Page::Playlist(id) = self.presentation.page() else {
            return;
        };
        let Some(id) = super::decode(id) else { return };
        let Some(Collection::Playlist {
            name,
            cover_art,
            songs,
            ..
        }) = self.presentation.navidrome_search.collection.get()
        else {
            return;
        };
        self.keep_download(
            Kind::Playlist,
            id,
            name.clone(),
            cover_art.clone(),
            Some(songs.clone()),
        );
    }
    pub(crate) fn download_song_control(&mut self, id: String, retry: bool) {
        if let Some(library) = &mut self.downloads.library {
            if retry && !library.wanted(&id) {
                return;
            }
            if let Some(track) = library.tracks.get_mut(&id)
                && !track.status.ready()
            {
                track.status = if retry {
                    Status::Queued
                } else {
                    Status::Cancelled
                };
            }
        }
        self.downloads.error = None;
        self.download_command(if retry {
            Action::RetrySong(id)
        } else {
            Action::PauseSong(id)
        });
    }
    pub(crate) fn pause_download(&mut self, key: String) {
        if let Some(library) = &mut self.downloads.library {
            library.pause(&key);
        }
        self.downloads.error = None;
        self.download_command(Action::Pause(key));
    }
    pub(crate) fn retry_download_storage(&mut self) {
        self.downloads.error = None;
        if self.downloads.library.is_none() {
            self.open_downloads();
        } else {
            self.download_command(Action::Online(!self.offline()));
        }
    }
    pub(crate) fn retry_download(&mut self, key: String) {
        if let Some(library) = &mut self.downloads.library {
            library.retry(&key);
        }
        self.downloads.error = None;
        self.download_command(Action::Retry(key));
    }
    pub(crate) fn remove_download(&mut self, key: String) {
        if let Some(library) = &mut self.downloads.library {
            library.remove(&key);
        }
        self.downloads.content_revision = self.downloads.content_revision.wrapping_add(1);
        self.downloads.error = None;
        self.download_command(Action::Remove(key));
    }
    pub(crate) fn open_offline(&mut self, server: &str, username: &str) {
        match Client::offline(server, username, &self.proxy) {
            Ok(client) => {
                self.apply(super::Action::Pause(true));
                self.session = self.session.wrapping_add(1);
                self.backend.send(Request::Cancel);
                self.connecting = false;
                self.restoring_login = false;
                self.error = None;
                self.set_client(client);
                self.connected = true;
                self.presentation.open(Page::Home);
            }
            Err(error) => self.error = Some(error),
        }
    }
    pub(crate) fn reconnect(&mut self) {
        self.apply(super::Action::Pause(true));
        self.checkpoint_session(true);
        self.session = self.session.wrapping_add(1);
        self.backend.send(Request::Cancel);
        self.connected = false;
        self.connecting = false;
        self.restoring_login = true;
        self.error = None;
        self.backend.send(Request::Restore {
            session: self.session,
        });
    }
    pub(crate) fn download_status(&self, id: &str) -> Option<&Status> {
        self.downloads
            .library
            .as_ref()?
            .tracks
            .get(id)
            .map(|t| &t.status)
    }
}

#[cfg(feature = "navidrome-demo")]
impl App {
    pub(super) fn demo_downloads(&mut self, page: Option<&str>, flags: &[&str]) {
        if !flags.iter().any(|f| f.starts_with("downloads")) {
            return;
        }
        let mut library = Library::new("demo".into());
        if !flags.contains(&"downloads-empty") && !flags.contains(&"downloads-loading") {
            for index in 0..3 {
                let album = self.albums[index].clone();
                let mut songs = album.song;
                if index > 0 {
                    for song in &mut songs {
                        song.id = format!("{index}-{}", song.id);
                    }
                }
                library
                    .keep(
                        Selection {
                            kind: Kind::Album,
                            id: album.id,
                            title: album.name,
                            cover: album.cover_art,
                            songs: Vec::new(),
                            paused: false,
                            pending: false,
                            error: None,
                        },
                        Some(songs),
                    )
                    .unwrap();
            }
            for (index, track) in library.tracks.values_mut().enumerate() {
                track.status = if flags.contains(&"downloads-progress") && index % 4 == 0 {
                    Status::Downloading {
                        bytes: 4 * 1024 * 1024,
                        total: Some(24 * 1024 * 1024),
                    }
                } else if flags.contains(&"downloads-error") && index % 4 == 1 {
                    Status::Failed {
                        message: "The server did not respond. Try again.".into(),
                    }
                } else {
                    Status::Ready {
                        bytes: 24 * 1024 * 1024,
                        codec: "flac".into(),
                    }
                };
            }
        }
        self.downloads.library = Some(library);
        self.downloads.loading = flags.contains(&"downloads-loading");
        self.downloads.error = flags
            .contains(&"downloads-storage")
            .then(|| "Could not save downloads. Check free space and folder permissions.".into());
        if flags.contains(&"downloads-offline") {
            self.client = Some(
                Client::offline(
                    "https://music.example.com",
                    "listener",
                    &crate::settings::ProxyConfig::Off,
                )
                .unwrap(),
            );
        }
        self.summarize_downloads();
        if page == Some("downloads") {
            self.presentation.history = vec![Page::Home, Page::Downloads];
            self.presentation.history_index = 1;
        }
        if flags.contains(&"downloads-detail") {
            self.presentation.history = vec![
                Page::Home,
                Page::Downloads,
                Page::Download(Kind::Album.key("alb0")),
            ];
            self.presentation.history_index = 2;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::navidrome::tests::with_app;
    #[test]
    fn progress_updates_unique_bytes_and_duplicate_counts_without_rebuilding_collections() {
        with_app(|app, _| {
            let song = app.albums[0].song[0].clone();
            app.downloads.library = Some(Library::new("account".into()));
            app.keep_download(
                Kind::Playlist,
                "first".into(),
                "First".into(),
                None,
                Some(vec![song.clone(), song.clone()]),
            );
            app.keep_download(
                Kind::Song,
                song.id.clone(),
                song.title.clone(),
                None,
                Some(vec![song.clone()]),
            );
            let revision = app.downloads.revision;
            for status in [
                Status::Downloading {
                    bytes: 100,
                    total: Some(200),
                },
                Status::Ready {
                    bytes: 200,
                    codec: "flac".into(),
                },
            ] {
                app.download_update(
                    app.session,
                    revision,
                    Update::Track {
                        id: song.id.clone(),
                        status,
                    },
                );
            }
            assert_eq!(app.downloads.bytes, 200);
            let first = &app.downloads.summaries[&Kind::Playlist.key("first")];
            assert_eq!(
                (first.ready, first.total, first.bytes, first.active),
                (2, 2, 200, 0)
            );
            assert_eq!(app.downloads.summaries[&Kind::Song.key(&song.id)].ready, 1);
        });
    }
    #[test]
    fn stale_progress_cannot_restore_removed_downloads_or_an_old_account() {
        with_app(|app, _| {
            let song = app.albums[0].song[0].clone();
            app.downloads.library = Some(Library::new("account".into()));
            app.keep_download(
                Kind::Song,
                song.id.clone(),
                song.title.clone(),
                None,
                Some(vec![song.clone()]),
            );
            let stale = app.downloads.library.clone().unwrap();
            let revision = app.downloads.revision;
            let key = Kind::Song.key(&song.id);
            app.remove_download(key);
            app.download_update(
                app.session,
                revision,
                Update::Library(Box::new(stale.clone())),
            );
            app.download_update(
                app.session.wrapping_sub(1),
                app.downloads.revision,
                Update::Library(Box::new(stale)),
            );
            assert!(app.downloads.library.as_ref().unwrap().tracks.is_empty());
            assert!(app.downloads.order.is_empty());
            assert_eq!(app.downloads.bytes, 0);
        });
    }
}
