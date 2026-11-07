//! Playlist mutations keep a checkpoint across lost acknowledgements. A retry
//! checks the server first; it never repeats a possibly accepted append.
use super::api::{Client, Playlist, RequestError, Song};
use std::collections::HashSet;
use std::sync::Arc;

#[derive(Clone, Debug)]
// A multi-destination edit shares the selected metadata instead of copying it per request.
pub enum Edit {
    Create {
        name: String,
        songs: Arc<[Song]>,
    },
    Append {
        id: String,
        songs: Arc<[Song]>,
    },
    Remove {
        id: String,
        before: Vec<String>,
        positions: Vec<usize>,
    },
    Delete {
        id: String,
    },
}

impl Edit {
    pub fn id(&self) -> Option<&str> {
        match self {
            Self::Create { .. } => None,
            Self::Append { id, .. } | Self::Remove { id, .. } | Self::Delete { id } => Some(id),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Task {
    pub edit: Edit,
    pub sent: bool,
    pub expected: Option<Playlist>,
    existing: HashSet<String>,
}

impl Task {
    pub fn new(edit: Edit) -> Self {
        Self {
            edit,
            sent: false,
            expected: None,
            existing: HashSet::new(),
        }
    }

    /// Completed jobs retain only their identity and status for the dialog.
    pub fn release_data(&mut self) {
        self.expected = None;
        self.existing = HashSet::new();
        match &mut self.edit {
            Edit::Create { songs, .. } | Edit::Append { songs, .. } => *songs = Arc::from([]),
            Edit::Remove {
                before, positions, ..
            } => {
                *before = Vec::new();
                *positions = Vec::new();
            }
            Edit::Delete { .. } => {}
        }
    }

    pub async fn run(&mut self, client: &Client) -> Result<Outcome, String> {
        if self.sent {
            return self.reconcile(client).await;
        }
        let mut params = Vec::new();
        let endpoint;
        match &self.edit {
            Edit::Create { name, songs } => {
                if name.trim().is_empty() {
                    return Err("Enter a playlist name.".into());
                }
                self.existing = client
                    .playlists()
                    .await?
                    .into_iter()
                    .map(|p| p.id)
                    .collect();
                endpoint = "createPlaylist";
                params.push(("name", name.trim().into()));
                params.extend(songs.iter().map(|s| ("songId", s.id.clone())));
            }
            edit => {
                let id = edit.id().expect("existing playlist");
                let mut current = client.playlist(id).await?;
                if !current.writable(client.username()) {
                    return Err("This playlist is read-only for your account.".into());
                }
                match edit {
                    Edit::Append { songs, .. } => {
                        endpoint = "updatePlaylist";
                        params.push(("playlistId", id.into()));
                        params.extend(songs.iter().map(|s| ("songIdToAdd", s.id.clone())));
                        current.songs.extend(songs.iter().cloned());
                    }
                    Edit::Remove {
                        before, positions, ..
                    } => {
                        if !ids_equal(&current.songs, before) {
                            return Err("The playlist changed on the server. Refresh it and select the tracks again.".into());
                        }
                        let mut positions = positions.clone();
                        positions.sort_unstable();
                        positions.dedup();
                        if positions.iter().any(|&i| i >= current.songs.len()) {
                            return Err("The selected track is no longer available.".into());
                        }
                        endpoint = "updatePlaylist";
                        params.push(("playlistId", id.into()));
                        params.extend(
                            positions
                                .iter()
                                .map(|i| ("songIndexToRemove", i.to_string())),
                        );
                        let mut removals = positions.into_iter().peekable();
                        let mut index = 0;
                        current.songs.retain(|_| {
                            let remove = removals.peek() == Some(&index);
                            if remove {
                                removals.next();
                            }
                            index += 1;
                            !remove
                        });
                    }
                    Edit::Delete { .. } => {
                        endpoint = "deletePlaylist";
                        params.push(("id", id.into()));
                    }
                    Edit::Create { .. } => unreachable!(),
                }
                self.expected = Some(current);
            }
        }
        self.sent = true;
        match client.checked_request(endpoint, &params).await {
            Ok(body) => {
                if matches!(self.edit, Edit::Create { .. }) {
                    let playlist: Playlist = body
                        .get("playlist")
                        .cloned()
                        .and_then(|p| serde_json::from_value(p).ok())
                        .filter(|p: &Playlist| !p.id.is_empty())
                        .ok_or("The playlist may have been created. Retry to check its status.")?;
                    self.expected = Some(playlist.clone());
                    Ok(Outcome::Saved(playlist))
                } else if let Edit::Delete { id } = &self.edit {
                    Ok(Outcome::Deleted(id.clone()))
                } else {
                    Ok(Outcome::Saved(
                        self.expected.clone().expect("prepared playlist"),
                    ))
                }
            }
            Err(error @ (RequestError::Rejected(_) | RequestError::Missing)) => {
                let error = error.to_string();
                self.sent = false;
                self.expected = None;
                Err(error)
            }
            Err(RequestError::Unconfirmed(_)) => self.reconcile(client).await,
        }
    }

    async fn reconcile(&mut self, client: &Client) -> Result<Outcome, String> {
        match &self.edit {
            Edit::Create { name, songs } => {
                let candidates: Vec<_> = client
                    .playlist_destinations()
                    .await?
                    .into_iter()
                    .filter(|p| {
                        p.name == name.trim()
                            && !self.existing.contains(&p.id)
                            && p.writable(client.username())
                    })
                    .collect();
                // A name is not an identity. Ambiguous matches need review, never another create.
                if candidates.len() == 1 {
                    let current = client.playlist(&candidates[0].id).await?;
                    if same_songs(&current.songs, songs) {
                        self.expected = Some(current.clone());
                        return Ok(Outcome::Saved(current));
                    }
                }
            }
            Edit::Delete { id } => {
                if !client.playlists().await?.iter().any(|p| &p.id == id) {
                    return Ok(Outcome::Deleted(id.clone()));
                }
            }
            Edit::Append { id, .. } | Edit::Remove { id, .. } => {
                let current = client.playlist(id).await?;
                if self
                    .expected
                    .as_ref()
                    .is_some_and(|p| same_songs(&p.songs, &current.songs))
                {
                    return Ok(Outcome::Saved(current));
                }
            }
        }
        Err("Confirmation is still pending. Retry checks the server without sending the change twice.".into())
    }
}

#[derive(Debug)]
pub enum Outcome {
    Saved(Playlist),
    Deleted(String),
}

pub fn same_songs(a: &[Song], b: &[Song]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(a, b)| a.id == b.id)
}

fn ids_equal(songs: &[Song], ids: &[String]) -> bool {
    songs.len() == ids.len() && songs.iter().zip(ids).all(|(song, id)| &song.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::navidrome::test_server::{Server, response};
    use crate::settings::ProxyConfig;
    use std::sync::{Arc, Mutex};

    fn songs(ids: &[&str]) -> Vec<Song> {
        ids.iter()
            .map(|id| Song {
                id: (*id).into(),
                ..Default::default()
            })
            .collect()
    }

    fn ok(body: serde_json::Value) -> Vec<u8> {
        response(
            "200 OK",
            "",
            serde_json::to_vec(&serde_json::json!({"subsonic-response": body}))
                .unwrap()
                .as_slice(),
        )
    }

    fn client(server: &Server) -> Client {
        Client::new(&server.url, "listener", "dummy".into(), &ProxyConfig::Off).unwrap()
    }

    #[tokio::test]
    async fn lost_append_acknowledgement_checks_until_visible_without_sending_twice() {
        let writes = Arc::new(Mutex::new(0));
        let count = writes.clone();
        let mut reads = 0;
        let server = Server::spawn(move |request| {
            if request.starts_with("GET /rest/updatePlaylist.view?") {
                *count.lock().unwrap() += 1;
                let query = reqwest::Url::parse(&format!(
                    "http://fixture{}",
                    request.split_whitespace().nth(1).unwrap()
                ))
                .unwrap();
                assert_eq!(
                    query
                        .query_pairs()
                        .filter(|(k, _)| k == "songIdToAdd")
                        .map(|(_, v)| v.into_owned())
                        .collect::<Vec<_>>(),
                    ["dup", "middle", "dup"]
                );
                return Vec::new(); // Applied on the server, connection lost before acknowledgement.
            }
            assert!(request.starts_with("GET /rest/getPlaylist.view?"));
            reads += 1;
            let ids = if reads < 3 {
                vec!["base"]
            } else {
                vec!["base", "dup", "middle", "dup"]
            };
            ok(
                serde_json::json!({"status":"ok","playlist":{"id":"p","name":"Mix","owner":"listener","entry":ids.into_iter().map(|id| serde_json::json!({"id":id})).collect::<Vec<_>>()}}),
            )
        });
        let mut task = Task::new(Edit::Append {
            id: "p".into(),
            songs: songs(&["dup", "middle", "dup"]).into(),
        });
        assert!(task.run(&client(&server)).await.is_err());
        assert!(task.sent);
        assert!(matches!(
            task.run(&client(&server)).await,
            Ok(Outcome::Saved(_))
        ));
        assert_eq!(*writes.lock().unwrap(), 1);
    }

    #[tokio::test]
    async fn removal_uses_the_selected_server_occurrence_and_refuses_changed_order() {
        for changed in [false, true] {
            let writes = Arc::new(Mutex::new(0));
            let count = writes.clone();
            let server = Server::spawn(move |request| {
                if request.starts_with("GET /rest/updatePlaylist.view?") {
                    *count.lock().unwrap() += 1;
                    assert!(request.contains("songIndexToRemove=2"));
                    assert!(!request.contains("songIdToAdd"));
                    return ok(serde_json::json!({"status":"ok"}));
                }
                let ids = if changed {
                    ["dup", "dup", "middle"]
                } else {
                    ["dup", "middle", "dup"]
                };
                ok(
                    serde_json::json!({"status":"ok","playlist":{"id":"p","name":"Mix","readonly":false,"entry":ids.into_iter().map(|id| serde_json::json!({"id":id})).collect::<Vec<_>>()}}),
                )
            });
            let mut task = Task::new(Edit::Remove {
                id: "p".into(),
                before: ["dup", "middle", "dup"].map(String::from).to_vec(),
                positions: vec![2],
            });
            let result = task.run(&client(&server)).await;
            if changed {
                assert!(result.unwrap_err().contains("changed on the server"));
                assert!(!task.sent);
                assert_eq!(*writes.lock().unwrap(), 0);
            } else {
                let Outcome::Saved(p) = result.unwrap() else {
                    panic!("not saved")
                };
                assert!(same_songs(&p.songs, &songs(&["dup", "middle"])));
                assert_eq!(*writes.lock().unwrap(), 1);
            }
        }
    }

    #[tokio::test]
    async fn partial_success_retries_only_rejected_destination_and_rechecks_permissions() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let recorded = calls.clone();
        let mut denied = false;
        let server = Server::spawn(move |request| {
            let query = reqwest::Url::parse(&format!(
                "http://fixture{}",
                request.split_whitespace().nth(1).unwrap()
            ))
            .unwrap();
            let id = query
                .query_pairs()
                .find(|(k, _)| k == "id" || k == "playlistId")
                .unwrap()
                .1
                .into_owned();
            if request.starts_with("GET /rest/updatePlaylist.view?") {
                recorded.lock().unwrap().push(id.clone());
                if id == "b" {
                    denied = true;
                    return ok(serde_json::json!({"status":"failed","error":{"code":50}}));
                }
                return ok(serde_json::json!({"status":"ok"}));
            }
            ok(
                serde_json::json!({"status":"ok","playlist":{"id":id,"name":"Mix","owner":"listener","readonly":denied && id == "b"}}),
            )
        });
        let mut a = Task::new(Edit::Append {
            id: "a".into(),
            songs: songs(&["dup", "dup"]).into(),
        });
        let mut b = Task::new(Edit::Append {
            id: "b".into(),
            songs: songs(&["dup", "dup"]).into(),
        });
        assert!(a.run(&client(&server)).await.is_ok());
        assert!(b.run(&client(&server)).await.is_err());
        assert!(!b.sent);
        assert!(
            b.run(&client(&server))
                .await
                .unwrap_err()
                .contains("read-only")
        );
        assert_eq!(*calls.lock().unwrap(), ["a", "b"]);
    }

    #[tokio::test]
    async fn lost_create_response_resolves_new_id_without_confusing_same_named_playlists() {
        let mut created = false;
        let server = Server::spawn(move |request| {
            if request.starts_with("GET /rest/createPlaylist.view?") {
                assert!(!created);
                assert!(request.contains("name=Mix"));
                created = true;
                return Vec::new();
            }
            if request.starts_with("GET /rest/getPlaylists.view?") {
                let ids = if created {
                    vec!["old", "new"]
                } else {
                    vec!["old"]
                };
                return ok(
                    serde_json::json!({"status":"ok","playlists":{"playlist":ids.into_iter().map(|id| serde_json::json!({"id":id,"name":"Mix","owner":"listener"})).collect::<Vec<_>>()}}),
                );
            }
            assert!(request.contains("id=new"));
            ok(
                serde_json::json!({"status":"ok","playlist":{"id":"new","name":"Mix","owner":"listener","entry":[{"id":"dup"},{"id":"dup"}]}}),
            )
        });
        let mut task = Task::new(Edit::Create {
            name: "Mix".into(),
            songs: songs(&["dup", "dup"]).into(),
        });
        let Outcome::Saved(p) = task.run(&client(&server)).await.unwrap() else {
            panic!("not saved")
        };
        assert_eq!(p.id, "new");
        assert_eq!(p.songs.len(), 2);
    }

    #[tokio::test]
    async fn deletion_targets_only_the_selected_id_and_checks_readonly_before_writing() {
        for readonly in [false, true] {
            let server = Server::spawn(move |request| {
                if request.starts_with("GET /rest/deletePlaylist.view?") {
                    assert!(!readonly);
                    assert!(request.contains("id=selected"));
                    return ok(serde_json::json!({"status":"ok"}));
                }
                ok(
                    serde_json::json!({"status":"ok","playlist":{"id":"selected","name":"Same name","owner":"listener","readonly":readonly}}),
                )
            });
            let mut task = Task::new(Edit::Delete {
                id: "selected".into(),
            });
            assert_eq!(task.run(&client(&server)).await.is_ok(), !readonly);
        }
    }
}
