//! Server-provided lyrics. Logical text is never reordered or normalized.
use super::api::{Client, RequestError, Song};
use serde::Deserialize;
use serde_json::Value;
pub mod lrclib;
mod lyricsfile;
mod words;
pub use words::Word;

const MAX_LINES: usize = 10_000;
const MAX_TEXT: usize = 1024 * 1024;

#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub text: String,
    pub at_ms: Option<u64>,
    pub words: Vec<Word>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Lyrics {
    pub lines: Vec<Line>,
    pub timed: bool,
    pub invalid_timing: bool,
}
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    Found(Lyrics),
    Missing,
    Instrumental,
    Ambiguous,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Source {
    #[default]
    Server,
    Lrclib,
}
impl Lyrics {
    pub fn active_line(&self, position_ms: u64) -> Option<usize> {
        if !self.timed {
            return None;
        }
        self.lines
            .partition_point(|line| line.at_ms.is_some_and(|at| at <= position_ms))
            .checked_sub(1)
    }
}

#[derive(Deserialize)]
struct Structured {
    #[serde(default)]
    kind: Option<String>,
    #[serde(default, rename = "cueLine")]
    cues: Vec<Value>,
    #[serde(default)]
    synced: bool,
    #[serde(default)]
    offset: Value,
    #[serde(default)]
    instrumental: bool,
    #[serde(default)]
    line: Vec<RawLine>,
}
#[derive(Deserialize)]
struct RawLine {
    value: String,
    #[serde(default)]
    start: Value,
}

fn parse(lyrics: Structured) -> Result<Outcome, String> {
    if lyrics.line.len() > MAX_LINES
        || lyrics.line.iter().map(|l| l.value.len()).sum::<usize>() > MAX_TEXT
    {
        return Err("The server's lyrics are too large.".into());
    }
    if lyrics.instrumental {
        return Ok(Outcome::Instrumental);
    }
    if lyrics.line.iter().all(|line| line.value.trim().is_empty()) {
        return Ok(Outcome::Missing);
    }
    // Musical-note-only text explicitly marks an instrumental passage. An empty
    // response alone does not tell us that the recording is instrumental.
    if lyrics.line.iter().all(|line| {
        line.value
            .chars()
            .all(|c| c.is_whitespace() || matches!(c, '♪' | '♫' | '♬'))
    }) {
        return Ok(Outcome::Instrumental);
    }
    let offset = if lyrics.offset.is_null() {
        Some(0)
    } else {
        lyrics.offset.as_i64()
    };
    let mut valid = offset.is_some();
    let mut previous = 0;
    let mut lines: Vec<_> = lyrics
        .line
        .into_iter()
        .map(|line| {
            let at_ms = if lyrics.synced {
                let timestamp = line
                    .start
                    .as_u64()
                    .and_then(|start| i64::try_from(start).ok())
                    .zip(offset)
                    .and_then(|(start, offset)| start.checked_sub(offset))
                    .map(|at| at.max(0) as u64);
                valid &= timestamp.is_some_and(|at| at >= previous);
                if let Some(at) = timestamp {
                    previous = at;
                }
                timestamp
            } else {
                None
            };
            Line {
                text: line.value,
                at_ms,
                words: Vec::new(),
            }
        })
        .collect();
    let timed = lyrics.synced && valid;
    if !timed {
        for line in &mut lines {
            line.at_ms = None;
        }
    } else {
        words::attach(&mut lines, lyrics.cues, offset.unwrap_or(0));
    }
    Ok(Outcome::Found(Lyrics {
        lines,
        timed,
        invalid_timing: lyrics.synced && !valid,
    }))
}

pub fn structured(body: Value) -> Result<Outcome, String> {
    let entries = body
        .pointer("/lyricsList/structuredLyrics")
        .and_then(Value::as_array)
        .ok_or("The server returned invalid lyrics.")?;
    let mut fallback = Outcome::Missing;
    let mut malformed = false;
    for entry in entries {
        let Ok(entry) = serde_json::from_value::<Structured>(entry.clone()) else {
            malformed = true;
            continue;
        };
        if entry.kind.as_deref().is_some_and(|kind| kind != "main") {
            continue;
        }
        let result = parse(entry)?;
        match result {
            Outcome::Found(ref lyrics) if lyrics.timed => return Ok(result),
            Outcome::Found(_) if !matches!(fallback, Outcome::Found(_)) => fallback = result,
            Outcome::Instrumental if matches!(fallback, Outcome::Missing) => fallback = result,
            _ => (),
        }
    }
    if malformed && matches!(fallback, Outcome::Missing) {
        return Err("The server returned invalid lyrics.".into());
    }
    Ok(fallback)
}

pub fn plain(body: Value) -> Result<Outcome, String> {
    let value = body
        .get("lyrics")
        .filter(|value| value.is_object())
        .ok_or("The server returned invalid lyrics.")?;
    let text = match value.get("value") {
        None | Some(Value::Null) => "",
        Some(Value::String(text)) => text,
        _ => return Err("The server returned invalid lyrics.".into()),
    };
    if text.len() > MAX_TEXT {
        return Err("The server's lyrics are too large.".into());
    }
    parse(Structured {
        kind: None,
        cues: Vec::new(),
        synced: false,
        offset: Value::Null,
        instrumental: value
            .get("instrumental")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        line: text
            .lines()
            .map(|line| RawLine {
                value: line.into(),
                start: Value::Null,
            })
            .collect(),
    })
}

pub async fn fetch_server(client: &Client, song: &Song) -> Result<Outcome, String> {
    if client.supports_extension("songLyrics", 1).await? {
        let mut parameters = vec![("id", song.id.clone())];
        if client.supports_extension("songLyrics", 2).await? {
            parameters.push(("enhanced", "true".into()));
        }
        match client
            .checked_request("getLyricsBySongId", &parameters)
            .await
        {
            Ok(body) => return structured(body),
            Err(RequestError::Missing) => return Ok(Outcome::Missing),
            Err(error) => return Err(error.to_string()),
        }
    }
    if song.title.trim().is_empty() {
        return Ok(Outcome::Missing);
    }
    match client
        .checked_request(
            "getLyrics",
            &[
                ("artist", song.artist.clone()),
                ("title", song.title.clone()),
            ],
        )
        .await
    {
        Ok(body) => plain(body),
        Err(RequestError::Missing) => Ok(Outcome::Missing),
        Err(error) => Err(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn response(entry: Value) -> Value {
        json!({"lyricsList":{"structuredLyrics":[entry]}})
    }
    #[test]
    fn timestamps_apply_offset_and_keep_logical_bidi_text() {
        let text = "مرحبا hello עולם";
        let Outcome::Found(lyrics) = structured(response(json!({"synced":true,"offset":100,"line":[{"start":200,"value":text},{"start":400,"value":"next"}]}))).unwrap() else { panic!() };
        assert_eq!(lyrics.lines[0].text, text);
        assert_eq!(lyrics.lines[0].at_ms, Some(100));
        assert_eq!(lyrics.active_line(99), None);
        assert_eq!(lyrics.active_line(100), Some(0));
        assert_eq!(lyrics.active_line(300), Some(1));
    }
    #[test]
    fn malformed_timing_preserves_readable_text_without_seeking() {
        for start in [json!(-1), json!("wrong"), json!(null), json!(1.2)] {
            let Outcome::Found(lyrics) = structured(response(json!({"synced":true,"line":[{"start":start,"value":"first"},{"start":10,"value":"second"}]}))).unwrap() else { panic!() };
            assert!(!lyrics.timed);
            assert!(lyrics.invalid_timing);
            assert!(lyrics.lines.iter().all(|l| l.at_ms.is_none()));
            assert_eq!(lyrics.active_line(100), None);
        }
        let Outcome::Found(lyrics) = structured(response(
            json!({"synced":true,"line":[{"start":20,"value":"a"},{"start":10,"value":"b"}]}),
        ))
        .unwrap() else {
            panic!()
        };
        assert!(lyrics.invalid_timing);
        assert_eq!(lyrics.lines[0].text, "a");
    }
    #[test]
    fn plain_missing_instrumental_and_malformed_are_distinct() {
        let Outcome::Found(lyrics) = plain(json!({"lyrics":{"value":"one\n\nاثنان"}})).unwrap()
        else {
            panic!()
        };
        assert!(!lyrics.timed);
        assert_eq!(lyrics.lines.len(), 3);
        assert_eq!(plain(json!({"lyrics":{}})).unwrap(), Outcome::Missing);
        assert!(plain(json!({"lyrics":{"value":23}})).is_err());
        assert!(plain(json!({"lyrics":[]})).is_err());
        assert_eq!(
            plain(json!({"lyrics":{"value":"♪"}})).unwrap(),
            Outcome::Instrumental
        );
        assert!(structured(json!({"lyricsList":{}})).is_err());
    }
}

#[cfg(test)]
mod network_tests {
    use super::*;
    use crate::navidrome::test_server::{Server, response};
    use std::sync::{Arc, Mutex};
    #[tokio::test]
    async fn discovers_once_and_fetches_structured_lyrics_by_the_opaque_id() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let seen = calls.clone();
        let server = Server::spawn(move |request| {
            let url = reqwest::Url::parse(&format!(
                "http://localhost{}",
                request.split_whitespace().nth(1).unwrap()
            ))
            .unwrap();
            seen.lock().unwrap().push(url.path().to_owned());
            let body = match url.path() {
                "/rest/getOpenSubsonicExtensions.view" => {
                    serde_json::json!({"status":"ok","openSubsonicExtensions":[{"name":"songLyrics","versions":[1,2]}]})
                }
                "/rest/getLyricsBySongId.view" => {
                    assert_eq!(
                        url.query_pairs().find(|(k, _)| k == "id").unwrap().1,
                        "opaque:/α & song"
                    );
                    assert!(
                        !url.query_pairs()
                            .any(|(k, _)| k == "artist" || k == "title")
                    );
                    assert!(
                        url.query_pairs()
                            .any(|(k, v)| k == "enhanced" && v == "true")
                    );
                    serde_json::json!({"status":"ok","lyricsList":{"structuredLyrics":[{"synced":true,"line":[{"start":1000,"value":"Line"}]}]}})
                }
                _ => panic!("unexpected endpoint"),
            };
            response(
                "200 OK",
                "",
                serde_json::to_string(&serde_json::json!({"subsonic-response":body}))
                    .unwrap()
                    .as_bytes(),
            )
        });
        let client = Client::new(
            &server.url,
            "listener",
            "dummy".into(),
            &crate::settings::ProxyConfig::Off,
        )
        .unwrap();
        let song = Song {
            id: "opaque:/α & song".into(),
            ..Default::default()
        };
        for _ in 0..2 {
            assert!(matches!(
                fetch_server(&client.clone(), &song).await.unwrap(),
                Outcome::Found(Lyrics { timed: true, .. })
            ));
        }
        assert_eq!(
            calls.lock().unwrap().as_slice(),
            [
                "/rest/getOpenSubsonicExtensions.view",
                "/rest/getLyricsBySongId.view",
                "/rest/getLyricsBySongId.view"
            ]
        );
    }
    #[tokio::test]
    async fn legacy_server_falls_back_to_its_own_plain_lyrics_endpoint() {
        let server = Server::spawn(|request| {
            let url = reqwest::Url::parse(&format!(
                "http://localhost{}",
                request.split_whitespace().nth(1).unwrap()
            ))
            .unwrap();
            let body = match url.path() {
                "/rest/getOpenSubsonicExtensions.view" => {
                    serde_json::json!({"status":"failed","error":{"code":0}})
                }
                "/rest/getLyrics.view" => {
                    let params: std::collections::HashMap<_, _> = url.query_pairs().collect();
                    assert_eq!(params["artist"], "فنان");
                    assert_eq!(params["title"], "Song & title");
                    serde_json::json!({"status":"ok","lyrics":{"value":"First\nSecond"}})
                }
                _ => panic!("unexpected endpoint"),
            };
            response(
                "200 OK",
                "",
                serde_json::to_string(&serde_json::json!({"subsonic-response":body}))
                    .unwrap()
                    .as_bytes(),
            )
        });
        let client = Client::new(
            &server.url,
            "listener",
            "dummy".into(),
            &crate::settings::ProxyConfig::Off,
        )
        .unwrap();
        let song = Song {
            id: "id".into(),
            artist: "فنان".into(),
            title: "Song & title".into(),
            ..Default::default()
        };
        assert!(matches!(
            fetch_server(&client, &song).await.unwrap(),
            Outcome::Found(Lyrics { timed: false, .. })
        ));
    }
}
