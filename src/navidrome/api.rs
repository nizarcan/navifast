//! Navidrome's public OpenSubsonic interface. Credentials never enter models.

use std::sync::Arc;
use std::time::Duration;

use md5::{Digest, Md5};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::Sha256;

pub const PAGE_SIZE: usize = 60;
const MAX_JSON: usize = 16 * 1024 * 1024;

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Song {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub artist: String,
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
    #[serde(default)]
    pub cover_art: Option<String>,
    #[serde(default)]
    pub song_count: u32,
    #[serde(default)]
    pub year: u32,
    #[serde(default)]
    pub song: Vec<Song>,
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
        let mut response = self
            .http
            .get(self.url(endpoint, params))
            .send()
            .await
            .map_err(network_error)?
            .error_for_status()
            .map_err(network_error)?;
        if response.status().is_redirection() {
            return Err("The request was redirected. Use your server's final URL.".into());
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(network_error)? {
            if bytes.len() + chunk.len() > MAX_JSON {
                return Err("The server response is too large.".into());
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

    pub async fn cover(&self, id: &str) -> Result<Vec<u8>, String> {
        let mut response = self
            .http
            .get(self.url("getCoverArt", &[("id", id.into()), ("size", "300".into())]))
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

fn envelope(bytes: &[u8]) -> Result<serde_json::Value, String> {
    let json: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|_| "The address did not return a Subsonic response. Check the server URL and reverse proxy.".to_owned())?;
    let body = json
        .get("subsonic-response")
        .ok_or("This server did not return a Subsonic response.")?;
    match body.get("status").and_then(|v| v.as_str()) {
        Some("ok") => Ok(body.clone()),
        Some("failed") => Err(match body.pointer("/error/code").and_then(|v| v.as_u64()) {
            Some(40..=44) => "Sign-in failed. Check your Navidrome username and password.",
            Some(50) => "Your account does not have permission for this operation.",
            Some(70) => "This item is no longer available on the server.",
            _ => "The server could not complete this request.",
        }
        .into()),
        _ => Err("The server returned an invalid response status.".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let error = result.unwrap_err();
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
}
