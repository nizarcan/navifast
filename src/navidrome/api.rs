//! Navidrome's public OpenSubsonic interface. Credentials never enter models.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use md5::{Digest, Md5};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::Sha256;

pub const PAGE_SIZE: usize = 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoverSize {
    Sleeve,
    Portrait,
}

impl CoverSize {
    pub fn pixels(self) -> u32 {
        match self {
            Self::Sleeve => 300,
            Self::Portrait => 1200,
        }
    }
}

const MAX_JSON: usize = 16 * 1024 * 1024;

/// Older servers supply one artistId. OpenSubsonic can identify each credit.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ArtistCredits {
    pub artist_id: Option<String>,
    #[serde(default)]
    pub artists: Vec<ArtistCredit>,
}

impl ArtistCredits {
    fn artist_ids(&self) -> impl Iterator<Item = &str> {
        self.artists
            .iter()
            .filter_map(|artist| artist.id.as_deref())
            .filter(|id| !id.trim().is_empty())
    }

    pub fn contains_id(&self, id: &str) -> bool {
        if self.artist_ids().next().is_some() {
            self.artist_ids().any(|credit| credit == id)
        } else {
            self.artist_id.as_deref() == Some(id)
        }
    }

    pub fn has_ids(&self) -> bool {
        self.artist_ids().next().is_some()
            || self
                .artist_id
                .as_deref()
                .is_some_and(|id| !id.trim().is_empty())
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ArtistCredit {
    pub cover_art: Option<String>,
    pub id: Option<String>,
    #[serde(default)]
    pub name: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Song {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub artist: String,
    #[serde(flatten)]
    pub credits: ArtistCredits,
    #[serde(default)]
    pub album: String,
    #[serde(default)]
    pub cover_art: Option<String>,
    #[serde(default)]
    pub duration: f64,
    #[serde(default)]
    pub suffix: String,
    #[serde(default)]
    pub track: u32,
    #[serde(default)]
    pub disc_number: u32,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Album {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub artist: String,
    #[serde(flatten)]
    pub credits: ArtistCredits,
    #[serde(default)]
    pub cover_art: Option<String>,
    #[serde(default)]
    pub song_count: u32,
    #[serde(default)]
    pub release_types: Vec<String>,
    #[serde(default)]
    pub year: u32,
    #[serde(default)]
    pub song: Vec<Song>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct NamedItem {
    pub id: String,
    pub name: String,
}

#[derive(Default, Deserialize)]
pub struct Catalogue {
    #[serde(default)]
    pub song: Vec<Song>,
    #[serde(default)]
    pub album: Vec<Album>,
    #[serde(default)]
    pub artist: Vec<NamedItem>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FavoriteKind {
    Song,
    Album,
    Artist,
}

impl FavoriteKind {
    fn parameter(self) -> &'static str {
        match self {
            Self::Song => "id",
            Self::Album => "albumId",
            Self::Artist => "artistId",
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RequestError {
    #[error("{0}")]
    Rejected(String),
    #[error("{0}")]
    Unconfirmed(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchPart {
    Catalogue,
    Playlists,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CollectionKind {
    Artist,
    Playlist,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtistInfo {
    pub biography: Option<String>,
    #[serde(default)]
    pub similar_artist: Vec<ArtistCredit>,
}

pub enum Collection {
    Artist {
        name: String,
        cover_art: Option<String>,
        albums: Vec<Album>,
    },
    Playlist {
        name: String,
        songs: Vec<Song>,
    },
}

// Deliberately no Debug: both the secret and authenticated request URLs are sensitive.
#[derive(Clone)]
pub struct Client {
    base: reqwest::Url,
    username: String,
    password: Arc<String>,
    http: reqwest::Client,
    pub scope: String,
}

pub fn server_url(value: &str) -> Result<reqwest::Url, String> {
    let mut url = reqwest::Url::parse(value.trim()).map_err(|_| {
        "Enter a complete server URL, for example https://music.example.com.".to_owned()
    })?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err("The server URL must use http:// or https://.".into());
    }
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(
            "Use the server's base URL without credentials, query parameters, or a fragment."
                .into(),
        );
    }
    let path = format!("{}/", url.path().trim_end_matches('/'));
    url.set_path(&path);
    Ok(url)
}

impl Client {
    pub fn new(
        server: &str,
        username: &str,
        password: String,
        proxy: &crate::settings::ProxyConfig,
    ) -> Result<Self, String> {
        let base = server_url(server)?;
        if username.trim().is_empty() || password.is_empty() {
            return Err("Enter your Navidrome username and password.".into());
        }
        let username = username.trim().to_owned();
        let scope = format!(
            "{:x}",
            Sha256::digest(format!("{}\0{username}", base.as_str()))
        );
        Ok(Self {
            base,
            username,
            password: Arc::new(password),
            http: crate::http::client_builder(proxy)?
                .connect_timeout(Duration::from_secs(8))
                .timeout(Duration::from_secs(30))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(network_error)?,
            scope,
        })
    }

    pub fn base_url(&self) -> &str {
        self.base.as_str()
    }

    pub fn username(&self) -> &str {
        &self.username
    }

    pub(super) fn saved_login(&self) -> crate::credentials::NavidromePassword {
        crate::credentials::NavidromePassword {
            server: self.base_url().into(),
            username: self.username.clone(),
            password: self.password.as_ref().clone(),
        }
    }

    pub(super) fn url(&self, endpoint: &str, params: &[(&str, String)]) -> reqwest::Url {
        let mut url = self
            .base
            .join(&format!("rest/{endpoint}.view"))
            .expect("constant API path");
        let salt = format!("{:032x}", rand::random::<u128>());
        let token = format!("{:x}", Md5::digest(format!("{}{salt}", self.password)));
        url.query_pairs_mut()
            .append_pair("u", &self.username)
            .append_pair("t", &token)
            .append_pair("s", &salt)
            .append_pair("v", "1.16.1")
            .append_pair("c", "Navifast")
            .append_pair("f", "json")
            .extend_pairs(params.iter().map(|(k, v)| (*k, v.as_str())));
        url
    }

    async fn request(
        &self,
        endpoint: &str,
        params: &[(&str, String)],
    ) -> Result<serde_json::Value, String> {
        self.checked_request(endpoint, params)
            .await
            .map_err(|error| error.to_string())
    }

    async fn checked_request(
        &self,
        endpoint: &str,
        params: &[(&str, String)],
    ) -> Result<serde_json::Value, RequestError> {
        let mut response = self
            .http
            .get(self.url(endpoint, params))
            .send()
            .await
            .map_err(|error| RequestError::Unconfirmed(network_error(error)))?
            .error_for_status()
            .map_err(|error| {
                if error
                    .status()
                    .is_some_and(|status| status.is_client_error())
                {
                    RequestError::Rejected(network_error(error))
                } else {
                    RequestError::Unconfirmed(network_error(error))
                }
            })?;
        if response.status().is_redirection() {
            return Err(RequestError::Unconfirmed(
                "The request was redirected. Use your server's final URL.".into(),
            ));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|error| RequestError::Unconfirmed(network_error(error)))?
        {
            if bytes.len() + chunk.len() > MAX_JSON {
                return Err(RequestError::Unconfirmed(
                    "The server response is too large.".into(),
                ));
            }
            bytes.extend_from_slice(&chunk);
        }
        envelope(&bytes)
    }

    pub async fn ping(&self) -> Result<(), String> {
        self.request("ping", &[]).await.map(|_| ())
    }

    pub async fn albums(&self, offset: usize, newest: bool) -> Result<Vec<Album>, String> {
        let body = self
            .request(
                "getAlbumList2",
                &[
                    (
                        "type",
                        if newest {
                            "newest"
                        } else {
                            "alphabeticalByName"
                        }
                        .into(),
                    ),
                    ("size", PAGE_SIZE.to_string()),
                    ("offset", offset.to_string()),
                ],
            )
            .await?;
        decode(
            body.get("albumList2")
                .and_then(|v| v.get("album"))
                .filter(|v| !v.is_null())
                .cloned()
                .unwrap_or_else(|| serde_json::json!([])),
        )
    }

    pub async fn album(&self, id: &str) -> Result<Album, String> {
        let body = self.request("getAlbum", &[("id", id.into())]).await?;
        decode(
            body.get("album")
                .cloned()
                .ok_or("The server returned no album.")?,
        )
    }

    pub async fn favorites(&self) -> Result<Catalogue, String> {
        let body = self.request("getStarred2", &[]).await?;
        decode(
            body.get("starred2")
                .cloned()
                .ok_or("The server returned no favorites list.")?,
        )
    }

    pub async fn set_favorite(
        &self,
        kind: FavoriteKind,
        id: &str,
        starred: bool,
    ) -> Result<(), RequestError> {
        self.checked_request(
            if starred { "star" } else { "unstar" },
            &[(kind.parameter(), id.into())],
        )
        .await
        .map(|_| ())
    }

    pub async fn search(&self, query: &str) -> Result<Catalogue, String> {
        let body = self.request("search3", &[("query", query.into())]).await?;
        decode(
            body.get("searchResult3")
                .cloned()
                .ok_or("The server returned no search results object.")?,
        )
    }

    pub async fn matching_playlists(&self, query: &str) -> Result<Vec<NamedItem>, String> {
        #[derive(Deserialize)]
        struct Playlists {
            #[serde(default)]
            playlist: Vec<NamedItem>,
        }
        let body = self.request("getPlaylists", &[]).await?;
        let playlists: Playlists = decode(
            body.get("playlists")
                .cloned()
                .ok_or("The server returned no playlist list.")?,
        )?;
        let needle = query.to_lowercase();
        Ok(playlists
            .playlist
            .into_iter()
            .filter(|item| item.name.to_lowercase().contains(&needle))
            .collect())
    }

    pub async fn collection(&self, kind: CollectionKind, id: &str) -> Result<Collection, String> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Artist {
            name: String,
            cover_art: Option<String>,
            #[serde(default)]
            album: Vec<Album>,
        }
        #[derive(Deserialize)]
        struct Playlist {
            name: String,
            #[serde(default)]
            entry: Vec<Song>,
        }
        let (endpoint, field) = match kind {
            CollectionKind::Artist => ("getArtist", "artist"),
            CollectionKind::Playlist => ("getPlaylist", "playlist"),
        };
        let body = self.request(endpoint, &[("id", id.into())]).await?;
        let value = body
            .get(field)
            .cloned()
            .ok_or("The server returned no collection.")?;
        Ok(match kind {
            CollectionKind::Artist => {
                let Artist {
                    name,
                    cover_art,
                    album,
                } = decode(value)?;
                Collection::Artist {
                    name,
                    albums: album,
                    cover_art,
                }
            }
            CollectionKind::Playlist => {
                let Playlist { name, entry } = decode(value)?;
                Collection::Playlist { name, songs: entry }
            }
        })
    }

    /// Look up guest performances when getArtist returns only album-artist releases.
    /// A search hit supplies candidates. Only a matching performer ID establishes a credit.
    pub async fn appearances(
        &self,
        id: &str,
        name: &str,
        mut known: HashSet<String>,
    ) -> Result<Vec<Album>, String> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Track {
            id: String,
            #[serde(flatten)]
            credits: ArtistCredits,
            album_id: Option<String>,
            #[serde(default)]
            album: String,
            #[serde(default)]
            album_artists: Vec<ArtistCredit>,
            cover_art: Option<String>,
        }
        #[derive(Deserialize)]
        struct Results {
            #[serde(default)]
            song: Vec<Track>,
        }
        let mut releases = Vec::new();
        let mut offset = 0;
        let mut page_starts = HashSet::new();
        loop {
            let body = self
                .request(
                    "search3",
                    &[
                        ("query", name.into()),
                        ("songCount", PAGE_SIZE.to_string()),
                        ("songOffset", offset.to_string()),
                        ("artistCount", "0".into()),
                        ("albumCount", "0".into()),
                    ],
                )
                .await?;
            let results: Results = decode(
                body.get("searchResult3")
                    .cloned()
                    .ok_or("The server returned no search results object.")?,
            )?;
            let count = results.song.len();
            let Some(first) = results.song.first() else {
                break;
            };
            if !page_starts.insert(first.id.clone()) {
                break;
            }
            for track in results.song {
                let Some(album_id) = track.album_id else {
                    continue;
                };
                if album_id.trim().is_empty()
                    || track.album.trim().is_empty()
                    || !track.credits.contains_id(id)
                    || track
                        .album_artists
                        .iter()
                        .any(|a| a.id.as_deref() == Some(id))
                    || !known.insert(album_id.clone())
                {
                    continue;
                }
                let artist = track
                    .album_artists
                    .iter()
                    .map(|a| a.name.as_str())
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join(", ");
                releases.push(Album {
                    id: album_id,
                    name: track.album,
                    artist,
                    cover_art: track.cover_art,
                    ..Default::default()
                });
            }
            if count < PAGE_SIZE {
                break;
            }
            offset += count;
        }
        Ok(releases)
    }

    pub async fn artist_info(&self, id: &str) -> Result<ArtistInfo, String> {
        let body = self
            .request(
                "getArtistInfo2",
                &[("id", id.into()), ("includeNotPresent", "false".into())],
            )
            .await?;
        let mut info: ArtistInfo = decode(
            body.get("artistInfo2")
                .cloned()
                .ok_or("The server returned no artist information.")?,
        )?;
        info.biography = info
            .biography
            .map(|bio| crate::util::strip_html(&bio))
            .filter(|bio| !bio.trim().is_empty());
        // Some servers include unmatched Last.fm recommendations with the
        // Subsonic sentinel -1 even when only library artists were requested.
        let mut seen = HashSet::new();
        info.similar_artist.retain(|artist| {
            !artist.name.trim().is_empty()
                && artist.id.as_deref().is_some_and(|other| {
                    !other.trim().is_empty()
                        && other != "-1"
                        && other != id
                        && seen.insert(other.to_owned())
                })
        });
        Ok(info)
    }

    pub async fn cover(&self, id: &str, size: CoverSize) -> Result<Vec<u8>, String> {
        let mut response = self
            .http
            .get(self.url(
                "getCoverArt",
                &[("id", id.into()), ("size", size.pixels().to_string())],
            ))
            .send()
            .await
            .map_err(network_error)?
            .error_for_status()
            .map_err(network_error)?;
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(network_error)? {
            if bytes.len() + chunk.len() > 8 * 1024 * 1024 {
                return Err("Artwork is too large.".into());
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    }

    pub(super) fn stream_url(&self, id: &str) -> reqwest::Url {
        self.url("stream", &[("id", id.into()), ("format", "raw".into())])
    }

    pub(super) fn stream_client(
        proxy: &crate::settings::ProxyConfig,
    ) -> Result<reqwest::blocking::Client, String> {
        crate::http::blocking_builder(proxy)?
            .connect_timeout(Duration::from_secs(8))
            .timeout(Duration::from_secs(15))
            // A redirected stream must not forward the credential-bearing URL.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(network_error)
    }
}

fn decode<T: DeserializeOwned>(value: serde_json::Value) -> Result<T, String> {
    serde_json::from_value(value)
        .map_err(|_| "The server returned unexpected music metadata.".into())
}

pub(super) fn network_error(error: reqwest::Error) -> String {
    if error.is_timeout() {
        "The server did not respond in time. Check the connection and try again.".into()
    } else if error
        .status()
        .is_some_and(|status| matches!(status.as_u16(), 401 | 403))
    {
        "Access was denied. Check your server address and credentials.".into()
    } else {
        format!("Could not reach the server: {}", error.without_url())
    }
}

fn envelope(bytes: &[u8]) -> Result<serde_json::Value, RequestError> {
    let json: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|_| RequestError::Unconfirmed("The address did not return a Subsonic response. Check the server URL and reverse proxy.".into()))?;
    let body = json.get("subsonic-response").ok_or_else(|| {
        RequestError::Unconfirmed("This server did not return a Subsonic response.".into())
    })?;
    match body.get("status").and_then(|v| v.as_str()) {
        Some("ok") => Ok(body.clone()),
        Some("failed") => Err(RequestError::Rejected(
            match body.pointer("/error/code").and_then(|v| v.as_u64()) {
                Some(40..=44) => "Sign-in failed. Check your Navidrome username and password.",
                Some(50) => "Your account does not have permission for this operation.",
                Some(70) => "This item is no longer available on the server.",
                _ => "The server could not complete this request.",
            }
            .into(),
        )),
        _ => Err(RequestError::Unconfirmed(
            "The server returned an invalid response status.".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn favorites_decode_all_groups_and_write_only_the_entity_parameter() {
        use crate::navidrome::test_server::{Server, response};
        let server = Server::spawn(|request| {
            let url = reqwest::Url::parse(&format!(
                "http://localhost{}",
                request.split_whitespace().nth(1).unwrap()
            ))
            .unwrap();
            let body = match url.path() {
                "/rest/getStarred2.view" => serde_json::json!({"status":"ok","starred2":{
                    "song":[{"id":"s:/α","title":"Song","userRating":1}],
                    "album":[{"id":"a:/α","name":"Album"}], "artist":[{"id":"r:/α","name":"Artist"}]
                }}),
                "/rest/star.view" | "/rest/unstar.view" => {
                    let params: Vec<_> = url
                        .query_pairs()
                        .filter(|(key, _)| {
                            !matches!(key.as_ref(), "u" | "s" | "t" | "c" | "v" | "f")
                        })
                        .collect();
                    assert_eq!(params.len(), 1);
                    let (key, id) = &params[0];
                    assert!(matches!(
                        (key.as_ref(), id.as_ref()),
                        ("id", "s:/α") | ("albumId", "a:/α") | ("artistId", "r:/α")
                    ));
                    serde_json::json!({"status":"ok"})
                }
                other => panic!("Unexpected endpoint {other}"),
            };
            response(
                "200 OK",
                "",
                &serde_json::to_vec(&serde_json::json!({"subsonic-response":body})).unwrap(),
            )
        });
        let client = Client::new(
            &server.url,
            "test",
            "dummy".into(),
            &crate::settings::ProxyConfig::Off,
        )
        .unwrap();
        let list = client.favorites().await.unwrap();
        assert_eq!(list.song[0].title, "Song");
        assert_eq!(list.album[0].name, "Album");
        assert_eq!(list.artist[0].name, "Artist");
        for (kind, id) in [
            (FavoriteKind::Song, &list.song[0].id),
            (FavoriteKind::Album, &list.album[0].id),
            (FavoriteKind::Artist, &list.artist[0].id),
        ] {
            for starred in [true, false] {
                client.set_favorite(kind, id, starred).await.unwrap();
            }
        }
    }

    #[test]
    fn base_paths_and_opaque_ids_survive_url_encoding() {
        let client = Client::new(
            "https://example.com/music",
            "alice",
            "secret".into(),
            &crate::settings::ProxyConfig::Off,
        )
        .unwrap();
        let url = client.url("getAlbum", &[("id", "a/b&c".into())]);
        assert_eq!(url.path(), "/music/rest/getAlbum.view");
        let query: std::collections::HashMap<_, _> = url.query_pairs().collect();
        assert_eq!(query["id"], "a/b&c");
        assert!(!url.as_str().contains("secret"));
        assert!(!query.contains_key("p"));
        assert_eq!(
            query["t"],
            format!("{:x}", Md5::digest(format!("secret{}", query["s"])))
        );
        assert_ne!(url, client.url("getAlbum", &[("id", "a/b&c".into())]));
    }

    #[test]
    fn credentials_and_query_strings_are_rejected_in_base_urls() {
        for url in [
            "file:///music",
            "https://user:secret@example.com",
            "https://example.com?token=secret",
            "https://example.com/#/album",
        ] {
            assert!(server_url(url).is_err());
        }
    }

    #[test]
    fn protocol_failures_are_errors_even_with_http_success_and_never_echo_server_secrets() {
        let result = envelope(
            br#"{"subsonic-response":{"status":"failed","error":{"code":40,"message":"secret"}}}"#,
        );
        let error = result.unwrap_err().to_string();
        assert!(error.contains("Sign-in failed"));
        assert!(!error.contains("secret"));
        assert!(envelope(b"<html>login</html>").is_err());
    }

    #[test]
    fn identity_is_scoped_to_server_and_account() {
        let make = |url, user| {
            Client::new(
                url,
                user,
                "secret".into(),
                &crate::settings::ProxyConfig::Off,
            )
            .unwrap()
        };
        assert_eq!(
            make("https://example.com/music", "a").scope,
            make("https://example.com/music/", "a").scope
        );
        assert_ne!(
            make("https://example.com", "a").scope,
            make("https://example.com", "b").scope
        );
        assert_ne!(
            make("https://example.com", "a").scope,
            make("https://other.com", "a").scope
        );
    }
    #[test]
    fn real_http_auth_pagination_and_failure_responses() {
        use crate::navidrome::test_server::{Server, response};
        let server = Server::spawn(|request| {
            let target = request.split_whitespace().nth(1).unwrap();
            let url = reqwest::Url::parse(&format!("http://localhost{target}")).unwrap();
            let q: std::collections::HashMap<_, _> = url.query_pairs().collect();
            assert_eq!(q["u"], "alice");
            assert_eq!(
                q["t"],
                format!("{:x}", Md5::digest(format!("secret{}", q["s"])))
            );
            assert!(!q.contains_key("p"));
            let body = if url.path().ends_with("ping.view") {
                serde_json::json!({"subsonic-response":{"status":"ok"}})
            } else if url.path().ends_with("getAlbumList2.view") {
                assert_eq!(q["offset"], "60");
                assert_eq!(q["size"], "60");
                assert_eq!(q["type"], "alphabeticalByName");
                serde_json::json!({"subsonic-response":{"status":"ok", "albumList2":{"album":[{"id":"a/b", "name":"Album"}]}}})
            } else {
                serde_json::json!({"subsonic-response":{"status":"failed", "error":{"code":40,"message":"secret must not be echoed"}}})
            };
            response(
                "200 OK",
                "Content-Type: application/json\r\n",
                body.to_string().as_bytes(),
            )
        });
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let client = Client::new(
                &format!("{}/music", server.url),
                "alice",
                "secret".into(),
                &crate::settings::ProxyConfig::Off,
            )
            .unwrap();
            client.ping().await.unwrap();
            assert_eq!(client.albums(60, false).await.unwrap()[0].id, "a/b");
            let error = client.album("missing").await.unwrap_err();
            assert!(error.contains("Sign-in failed"));
            assert!(!error.contains("secret"));
        });
    }
    #[tokio::test]
    async fn appearances_page_past_name_matches_and_deduplicate_album_ids() {
        use crate::navidrome::test_server::{Server, response};
        let server = Server::spawn(|request| {
            assert!(
                request.starts_with("GET /rest/search3.view?")
                    && request.contains("artistCount=0")
                    && request.contains("albumCount=0")
            );
            let songs = if request.contains("songOffset=0&") {
                (0..PAGE_SIZE).map(|i| serde_json::json!({"id":i.to_string(),"artistId":"namesake","artist":"Twin","album":"Wrong album","albumId":"wrong"})).collect()
            } else {
                assert!(request.contains(&format!("songOffset={PAGE_SIZE}")));
                vec![
                    serde_json::json!({"id":"a","artists":[{"id":"guest"}],"albumId":"own","album":"Own"}),
                    serde_json::json!({"id":"b","artists":[{"id":"guest"}],"albumArtists":[{"id":"guest"}],"albumId":"co","album":"Joint"}),
                    serde_json::json!({"id":"c","artists":[{"id":"first"},{"id":"guest"}],"albumArtists":[{"id":"first","name":"Performer"}],"albumId":"other:/α","album":"Other","coverArt":"art"}),
                    serde_json::json!({"id":"d","artistId":"guest","albumId":"other:/α","album":"Other"}),
                    serde_json::json!({"id":"e","title":"A song feat. Twin","albumId":"wrong","album":"Wrong"}),
                    serde_json::json!({"id":"f","artistId":"guest","artists":[{"id":"namesake"}],"albumId":"legacy-mismatch","album":"Wrong"}),
                ]
            };
            let data = serde_json::json!({"subsonic-response":{"status":"ok","searchResult3":{"song":songs}}});
            response("200 OK", "", data.to_string().as_bytes())
        });
        let client = Client::new(
            &server.url,
            "listener",
            "dummy".into(),
            &crate::settings::ProxyConfig::Off,
        )
        .unwrap();
        let albums = client
            .appearances("guest", "Twin", HashSet::from(["own".into()]))
            .await
            .unwrap();
        assert_eq!(albums.len(), 1);
        assert_eq!(albums[0].id, "other:/α");
        assert_eq!(albums[0].artist, "Performer");
        assert_eq!(albums[0].cover_art.as_deref(), Some("art"));
    }
}
