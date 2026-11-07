//! Account ratings use the existing authenticated OpenSubsonic transport.
use super::api::{Client, RequestError};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Kind {
    Song,
    Album,
}

impl Client {
    pub async fn set_rating(&self, id: &str, value: u8) -> Result<(), RequestError> {
        if value > 5 || id.is_empty() {
            return Err(RequestError::Rejected(
                "Choose a rating from 1 to 5, or clear it.".into(),
            ));
        }
        self.checked_request(
            "setRating",
            &[("id", id.into()), ("rating", value.to_string())],
        )
        .await
        .map(|_| ())
    }

    pub async fn rating(&self, kind: Kind, id: &str) -> Result<u8, String> {
        self.checked_rating(kind, id)
            .await
            .map_err(|e| e.to_string())
    }

    pub(crate) async fn checked_rating(&self, kind: Kind, id: &str) -> Result<u8, RequestError> {
        let (endpoint, field) = match kind {
            Kind::Song => ("getSong", "song"),
            Kind::Album => ("getAlbum", "album"),
        };
        let body = self.checked_request(endpoint, &[("id", id.into())]).await?;
        let item = body
            .get(field)
            .filter(|item| item.get("id").and_then(|id| id.as_str()) == Some(id))
            .ok_or_else(|| {
                RequestError::Unconfirmed("The server did not return this item's rating.".into())
            })?;
        match item.get("userRating") {
            None | Some(serde_json::Value::Null) => Ok(0),
            Some(value) => value
                .as_u64()
                .filter(|v| *v <= 5)
                .map(|v| v as u8)
                .ok_or_else(|| {
                    RequestError::Unconfirmed("The server returned an invalid rating.".into())
                }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        navidrome::test_server::{Server, response},
        settings::ProxyConfig,
    };

    #[tokio::test]
    async fn songs_and_albums_set_change_and_clear_using_opaque_ids() {
        for kind in [Kind::Song, Kind::Album] {
            let mut saved = 0;
            let server = Server::spawn(move |request| {
                let url = reqwest::Url::parse(&format!(
                    "http://localhost{}",
                    request.split_whitespace().nth(1).unwrap()
                ))
                .unwrap();
                let params: std::collections::HashMap<_, _> = url.query_pairs().collect();
                assert_eq!(params.get("id").unwrap(), "music:/α & 夜");
                let mut body = serde_json::json!({"status":"ok"});
                if url.path().ends_with("setRating.view") {
                    saved = params.get("rating").unwrap().parse::<u8>().unwrap();
                } else {
                    let field = if kind == Kind::Song { "song" } else { "album" };
                    assert!(url.path().ends_with(if kind == Kind::Song {
                        "getSong.view"
                    } else {
                        "getAlbum.view"
                    }));
                    body[field] = serde_json::json!({"id":"music:/α & 夜", "userRating":saved});
                }
                response(
                    "200 OK",
                    "",
                    serde_json::json!({"subsonic-response":body})
                        .to_string()
                        .as_bytes(),
                )
            });
            let client =
                Client::new(&server.url, "listener", "dummy".into(), &ProxyConfig::Off).unwrap();
            assert_eq!(client.rating(kind, "music:/α & 夜").await.unwrap(), 0);
            for value in [1, 5, 3, 0] {
                client.set_rating("music:/α & 夜", value).await.unwrap();
                assert_eq!(client.rating(kind, "music:/α & 夜").await.unwrap(), value);
            }
            assert!(client.set_rating("music:/α & 夜", 6).await.is_err());
        }
    }
    #[tokio::test]
    async fn absent_ratings_are_unrated_and_malformed_or_wrong_items_are_rejected() {
        for (body, expected) in [
            (serde_json::json!({"id":"song"}), Some(0)),
            (serde_json::json!({"id":"song","userRating":null}), Some(0)),
            (serde_json::json!({"id":"song","userRating":6}), None),
            (serde_json::json!({"id":"song","userRating":"4"}), None),
            (serde_json::json!({"id":"another","userRating":4}), None),
        ] {
            let server = Server::spawn(move |_| {
                response(
                    "200 OK",
                    "",
                    serde_json::json!({"subsonic-response":{"status":"ok","song":body}})
                        .to_string()
                        .as_bytes(),
                )
            });
            let client =
                Client::new(&server.url, "listener", "dummy".into(), &ProxyConfig::Off).unwrap();
            assert_eq!(client.rating(Kind::Song, "song").await.ok(), expected);
        }
    }
}
