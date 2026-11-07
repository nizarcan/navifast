//! Account-scoped playlist browsing, independent of search requests.
use super::App;
use crate::model::{Loadable, Page};
use crate::navidrome::api::NamedItem;
use crate::navidrome::backend::{Event, Request};

#[derive(Default)]
pub struct State {
    pub list: Loadable<Vec<NamedItem>>,
    pub refreshing: bool,
    pub error: Option<String>,
    request: u64,
}

impl App {
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
        let state = &mut self.presentation.navidrome_playlists;
        if let Event::PlaylistLibrary {
            session,
            request,
            result,
        } = event
            && session == self.session
            && request == state.request
            && self.presentation.page() == &Page::Playlists
        {
            let state = &mut self.presentation.navidrome_playlists;
            state.refreshing = false;
            state.error = result.as_ref().err().cloned();
            state.list.refresh(result);
        }
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
    fn accessible_playlists_preserve_ids_order_and_duplicates_and_retry_without_playing() {
        with_app(|app, host| {
            let mut transient = true;
            let server = Server::spawn(move |request| {
                let body = if request.starts_with("GET /rest/getPlaylists.view?") {
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
            let receive = |app: &mut App| {
                let event = app
                    .backend
                    .events
                    .recv_timeout(std::time::Duration::from_secs(5))
                    .unwrap();
                if matches!(event, Event::PlaylistLibrary { .. }) {
                    app.playlists_event(event);
                } else {
                    app.search_event(event);
                }
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
                    .songs
                    .iter()
                    .map(|s| s.id.as_str())
                    .collect::<Vec<_>>(),
                ["a", "b", "a", "c"]
            );
            assert_eq!(app.transport.index, Some(2));
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
