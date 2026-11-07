//! Server-issued public links. Authenticated media URLs never enter share state.
use super::api::{Client, RequestError};
use jiff::Timestamp;
use serde::Deserialize;

#[derive(Clone)]
pub struct Share {
    pub id: String,
    pub name: String,
    pub url: Option<String>,
    pub expires: Option<Timestamp>,
    pub expiration_known: bool,
}

#[derive(Clone)]
pub enum Task {
    List,
    Create {
        id: String,
        name: String,
        expires: i64,
    },
    Revoke(String),
}

pub enum Outcome {
    Listed { allowed: bool, shares: Vec<Share> },
    Created(Share),
    Revoked,
}

/// Keep the exact server string for the clipboard, but accept only public URLs.
fn public_link(raw: &str) -> Option<String> {
    let url = reqwest::Url::parse(raw).ok()?;
    if raw.len() > 8192
        || raw.chars().any(char::is_control)
        || !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url
            .path()
            .to_ascii_lowercase()
            .split('/')
            .any(|p| p == "rest" || p.ends_with(".view"))
        || url.query_pairs().any(|(key, _)| {
            matches!(
                key.to_ascii_lowercase().as_str(),
                "u" | "p" | "t" | "s" | "api_key" | "apikey" | "password" | "authorization"
            )
        })
    {
        return None;
    }
    Some(raw.into())
}

fn parse_shares(body: &serde_json::Value) -> Result<Vec<Share>, RequestError> {
    #[derive(Deserialize)]
    struct Raw {
        id: String,
        url: String,
        #[serde(default)]
        description: String,
        expires: Option<String>,
    }
    let bad = || RequestError::Unconfirmed("The server returned invalid music links.".into());
    let group = body
        .get("shares")
        .filter(|v| v.is_object())
        .ok_or_else(bad)?;
    let raw: Vec<Raw> = serde_json::from_value(
        group
            .get("share")
            .filter(|v| !v.is_null())
            .cloned()
            .unwrap_or_else(|| serde_json::json!([])),
    )
    .map_err(|_| bad())?;
    if raw.len() > 10_000 || raw.iter().any(|s| s.id.is_empty()) {
        return Err(bad());
    }
    let mut seen = std::collections::HashSet::new();
    Ok(raw
        .into_iter()
        .filter(|s| seen.insert(s.id.clone()))
        .map(|s| {
            let expires = s
                .expires
                .as_deref()
                .and_then(|date| date.parse::<Timestamp>().ok());
            Share {
                id: s.id,
                name: s.description,
                url: public_link(&s.url),
                expires,
                expiration_known: s.expires.is_none() || expires.is_some(),
            }
        })
        .collect())
}

impl Client {
    pub async fn share_task(&self, task: &Task) -> Result<Outcome, RequestError> {
        match task {
            Task::List => {
                let user = self
                    .checked_request("getUser", &[("username", self.username().into())])
                    .await?;
                let user = user
                    .get("user")
                    .filter(|u| u.get("username").and_then(|v| v.as_str()) == Some(self.username()))
                    .ok_or_else(|| {
                        RequestError::Unconfirmed(
                            "The server did not return your sharing permission.".into(),
                        )
                    })?;
                let allowed = match user.get("shareRole") {
                    Some(serde_json::Value::Bool(value)) => *value,
                    Some(serde_json::Value::String(value))
                        if value == "true" || value == "false" =>
                    {
                        value == "true"
                    }
                    _ => {
                        return Err(RequestError::Unconfirmed(
                            "The server did not return your sharing permission.".into(),
                        ));
                    }
                };
                let shares = if allowed {
                    parse_shares(&self.checked_request("getShares", &[]).await?)?
                } else {
                    Vec::new()
                };
                Ok(Outcome::Listed { allowed, shares })
            }
            Task::Create { id, name, expires } => {
                if id.is_empty() || *expires <= Timestamp::now().as_millisecond() {
                    return Err(RequestError::Rejected(
                        "Choose an item and a future expiration.".into(),
                    ));
                }
                let body = self
                    .checked_request(
                        "createShare",
                        &[
                            ("id", id.clone()),
                            ("description", name.clone()),
                            ("expires", expires.to_string()),
                        ],
                    )
                    .await?;
                let mut shares = parse_shares(&body)?;
                if shares.len() != 1 {
                    return Err(RequestError::Unconfirmed("The server did not confirm the new link. Refresh Music links before trying again.".into()));
                }
                let share = shares.remove(0);
                if !share.expires.is_some_and(|time| {
                    time.as_millisecond() <= expires.saturating_add(1000) && time > Timestamp::now()
                }) {
                    // Roll back this just-created link if the server ignored its expiry.
                    let revoked = self
                        .checked_request("deleteShare", &[("id", share.id)])
                        .await;
                    return Err(
                        if revoked.is_ok() || matches!(revoked, Err(RequestError::Missing)) {
                            RequestError::Rejected("The server did not honor the expiration. The new link was revoked.".into())
                        } else {
                            RequestError::Unconfirmed("The server did not honor the expiration. Check Music links and revoke the new link.".into())
                        },
                    );
                }
                Ok(Outcome::Created(share))
            }
            Task::Revoke(id) => {
                match self
                    .checked_request("deleteShare", &[("id", id.clone())])
                    .await
                {
                    Ok(_) | Err(RequestError::Missing) => Ok(Outcome::Revoked),
                    Err(error) => Err(error),
                }
            }
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
    use serde_json::json;

    #[test]
    fn public_links_keep_exact_server_text_and_reject_authenticated_urls() {
        let public = "https://music.example/share.php?id=abc&secret=a%2Bb";
        assert_eq!(public_link(public).as_deref(), Some(public));
        for bad in [
            "file:///secret",
            "javascript:alert(1)",
            "https://user:password@music.example/share/a",
            "https://music.example/rest/stream.view?id=a",
            "https://music.example/base/rest/getCoverArt?id=a",
            "https://music.example/share/a?u=user&t=secret&s=salt",
            "https://music.example/share/a?%70=password",
            "https://music.example/share/a?apiKey=secret",
        ] {
            assert!(public_link(bad).is_none());
        }
        let list = parse_shares(&json!({"shares":{"share":[{"id":"one","url":"https://music.example/rest/stream.view?u=user","expires":"bad"}]}})).unwrap();
        assert!(list[0].url.is_none());
        assert!(!list[0].expiration_known);
    }

    #[tokio::test]
    async fn expiring_creation_uses_ids_and_server_public_link_then_revokes_missing_share() {
        let expires = Timestamp::now().as_millisecond() + 86_400_000;
        let date = Timestamp::from_millisecond(expires).unwrap().to_string();
        let expected = "https://public.example/music/share/listen?secret=a%2Bb";
        let server = Server::spawn(move |request| {
            let url = reqwest::Url::parse(&format!(
                "http://localhost{}",
                request.split_whitespace().nth(1).unwrap()
            ))
            .unwrap();
            let pairs: std::collections::HashMap<_, _> = url.query_pairs().collect();
            let body = match url.path() {
                "/rest/getUser.view" => {
                    assert_eq!(pairs["username"], "listener");
                    json!({"status":"ok","user":{"username":"listener","shareRole":true}})
                }
                "/rest/getShares.view" => json!({"status":"ok","shares":{}}),
                "/rest/createShare.view" => {
                    assert_eq!(pairs["id"], "album:/α & 夜");
                    assert_eq!(pairs["expires"], expires.to_string());
                    assert_eq!(pairs["description"], "Album");
                    json!({"status":"ok","shares":{"share":[{"id":"new-share","url":expected,"description":"Album","expires":date}]}})
                }
                "/rest/deleteShare.view" => {
                    assert_eq!(pairs["id"], "new-share");
                    json!({"status":"failed","error":{"code":70}})
                }
                _ => panic!("unexpected endpoint"),
            };
            response(
                "200 OK",
                "",
                json!({"subsonic-response":body}).to_string().as_bytes(),
            )
        });
        let client =
            Client::new(&server.url, "listener", "dummy".into(), &ProxyConfig::Off).unwrap();
        assert!(matches!(
            client.share_task(&Task::List).await.unwrap(),
            Outcome::Listed { allowed: true, .. }
        ));
        let Outcome::Created(share) = client
            .share_task(&Task::Create {
                id: "album:/α & 夜".into(),
                name: "Album".into(),
                expires,
            })
            .await
            .unwrap()
        else {
            panic!("missing share")
        };
        assert_eq!(share.url.as_deref(), Some(expected));
        assert_eq!(share.expires.unwrap().as_millisecond(), expires);
        assert!(matches!(
            client.share_task(&Task::Revoke(share.id)).await.unwrap(),
            Outcome::Revoked
        ));
    }

    #[tokio::test]
    async fn disabled_or_read_only_accounts_do_not_list_and_permission_denial_is_safe() {
        for role in [json!(false), json!("false")] {
            let server = Server::spawn(move |request| {
                assert!(request.contains("/rest/getUser.view?"));
                response("200 OK","",json!({"subsonic-response":{"status":"ok","user":{"username":"listener","shareRole":role}}}).to_string().as_bytes())
            });
            let client =
                Client::new(&server.url, "listener", "dummy".into(), &ProxyConfig::Off).unwrap();
            assert!(matches!(
                client.share_task(&Task::List).await.unwrap(),
                Outcome::Listed { allowed: false, .. }
            ));
        }
        let server = Server::spawn(|_| {
            response("200 OK","",br#"{"subsonic-response":{"status":"failed","error":{"code":50,"message":"secret authenticated URL"}}}"#)
        });
        let client =
            Client::new(&server.url, "listener", "dummy".into(), &ProxyConfig::Off).unwrap();
        let Err(error) = client.share_task(&Task::Revoke("share".into())).await else {
            panic!("expected denial")
        };
        assert!(matches!(error, RequestError::Rejected(_)));
        assert!(!error.to_string().contains("secret"));
        assert!(error.to_string().contains("permission"));
    }

    #[tokio::test]
    async fn ignored_expiration_revokes_only_the_just_created_link() {
        let (send, receive) = std::sync::mpsc::channel();
        let server = Server::spawn(move |request| {
            let body = if request.contains("/rest/createShare.view?") {
                json!({"status":"ok","shares":{"share":[{"id":"new-link","url":"https://music.example/share/new-link"}]}})
            } else {
                assert!(request.contains("/rest/deleteShare.view?"));
                assert!(request.contains("id=new-link"));
                send.send(()).unwrap();
                json!({"status":"ok"})
            };
            response(
                "200 OK",
                "",
                json!({"subsonic-response":body}).to_string().as_bytes(),
            )
        });
        let client =
            Client::new(&server.url, "listener", "dummy".into(), &ProxyConfig::Off).unwrap();
        let result = client
            .share_task(&Task::Create {
                id: "song".into(),
                name: "Song".into(),
                expires: Timestamp::now().as_millisecond() + 86_400_000,
            })
            .await;
        assert!(matches!(result, Err(RequestError::Rejected(_))));
        receive
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap();
    }
}
