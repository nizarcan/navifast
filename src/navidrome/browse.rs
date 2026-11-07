//! Paged OpenSubsonic enumeration. Genre strings are opaque server values.
use super::api::{Album, Client, PAGE_SIZE, Song, decode};
use serde::Deserialize;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Query {
    Albums(super::albums::Order),
    Songs,
    Genres,
    GenreSongs(String),
    GenreAlbums(String),
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Genre {
    pub value: String,
    #[serde(default)]
    pub song_count: u32,
    #[serde(default)]
    pub album_count: u32,
}

pub enum Items {
    Songs(Vec<Song>),
    Albums(Vec<Album>),
    Genres(Vec<Genre>),
}

impl Items {
    pub fn len(&self) -> usize {
        match self {
            Self::Songs(items) => items.len(),
            Self::Albums(items) => items.len(),
            Self::Genres(items) => items.len(),
        }
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Client {
    pub async fn browse(&self, query: &Query, offset: usize) -> Result<Items, String> {
        let mut params = vec![("offset", offset.to_string())];
        let (endpoint, container, field) = match query {
            Query::Albums(order) => {
                return self.album_list(offset, *order).await.map(Items::Albums);
            }
            Query::Songs => {
                params = vec![
                    ("query", String::new()),
                    ("artistCount", "0".into()),
                    ("albumCount", "0".into()),
                    ("songCount", PAGE_SIZE.to_string()),
                    ("songOffset", offset.to_string()),
                ];
                ("search3", "searchResult3", "song")
            }
            Query::Genres => {
                params.clear();
                ("getGenres", "genres", "genre")
            }
            Query::GenreSongs(genre) => {
                params.extend([("genre", genre.clone()), ("count", PAGE_SIZE.to_string())]);
                ("getSongsByGenre", "songsByGenre", "song")
            }
            Query::GenreAlbums(genre) => {
                params.extend([
                    ("genre", genre.clone()),
                    ("type", "byGenre".into()),
                    ("size", PAGE_SIZE.to_string()),
                ]);
                ("getAlbumList2", "albumList2", "album")
            }
        };
        let body = self
            .checked_request(endpoint, &params)
            .await
            .map_err(|e| e.to_string())?;
        if *query == Query::Songs
            && body.get("openSubsonic").and_then(|v| v.as_bool()) != Some(true)
        {
            return Err("This server does not advertise OpenSubsonic all-song browsing. You can still browse albums and genres.".into());
        }
        let group = body
            .get(container)
            .filter(|v| v.is_object())
            .ok_or("The server returned no library results object.")?;
        let values = group
            .get(field)
            .filter(|v| !v.is_null())
            .cloned()
            .unwrap_or_else(|| serde_json::json!([]));
        let items = match query {
            Query::Songs | Query::GenreSongs(_) => Items::Songs(decode(values)?),
            Query::GenreAlbums(_) | Query::Albums(_) => Items::Albums(decode(values)?),
            Query::Genres => Items::Genres(decode(values)?),
        };
        if *query != Query::Genres && items.len() > PAGE_SIZE {
            return Err("The server returned more items than the requested page size.".into());
        }
        let invalid = match &items {
            Items::Songs(items) => items.iter().any(|item| item.id.is_empty()),
            Items::Albums(items) => items.iter().any(|item| item.id.is_empty()),
            Items::Genres(items) => items.iter().any(|item| item.value.trim().is_empty()),
        };
        if invalid {
            return Err("The server returned a library item without an identity.".into());
        }
        Ok(items)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::navidrome::test_server::{Server, response};
    use crate::settings::ProxyConfig;

    #[tokio::test]
    async fn genre_requests_preserve_exact_names_and_independent_offsets() {
        let genre = "  R&B / موسيقى + 夜  ";
        let server = Server::spawn(move |request| {
            let path = request.split_whitespace().nth(1).unwrap();
            let url = reqwest::Url::parse(&format!("http://localhost{path}")).unwrap();
            let pairs: std::collections::HashMap<_, _> = url.query_pairs().collect();
            assert_eq!(pairs.get("genre").unwrap(), genre);
            assert_eq!(pairs.get("offset").unwrap(), "60");
            let body = if url.path().ends_with("getSongsByGenre.view") {
                assert_eq!(pairs.get("count").unwrap(), "60");
                br#"{"subsonic-response":{"status":"ok","songsByGenre":{"song":[{"id":"compilation-track","title":"Only this track has the genre"}]}}}"#.as_slice()
            } else {
                assert_eq!(pairs.get("type").unwrap(), "byGenre");
                br#"{"subsonic-response":{"status":"ok","albumList2":{}}}"#.as_slice()
            };
            response("200 OK", "", body)
        });
        let client =
            Client::new(&server.url, "listener", "dummy".into(), &ProxyConfig::Off).unwrap();
        assert_eq!(
            client
                .browse(&Query::GenreSongs(genre.into()), 60)
                .await
                .unwrap()
                .len(),
            1
        );
        assert!(
            client
                .browse(&Query::GenreAlbums(genre.into()), 60)
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn all_songs_uses_empty_query_and_requires_the_server_declaration() {
        for supported in [true, false] {
            let server = Server::spawn(move |request| {
                let url = reqwest::Url::parse(&format!(
                    "http://localhost{}",
                    request.split_whitespace().nth(1).unwrap()
                ))
                .unwrap();
                let pairs: std::collections::HashMap<_, _> = url.query_pairs().collect();
                assert_eq!(pairs.get("query").unwrap(), "");
                assert_eq!(pairs.get("songOffset").unwrap(), "60");
                assert_eq!(pairs.get("artistCount").unwrap(), "0");
                assert_eq!(pairs.get("albumCount").unwrap(), "0");
                response("200 OK", "", serde_json::json!({"subsonic-response":{"status":"ok","openSubsonic":supported,"searchResult3":{}}}).to_string().as_bytes())
            });
            let client =
                Client::new(&server.url, "listener", "dummy".into(), &ProxyConfig::Off).unwrap();
            assert_eq!(client.browse(&Query::Songs, 60).await.is_ok(), supported);
        }
    }
}
