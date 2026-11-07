//! Optional, credential-free fallback. The server always gets the first chance.
use super::{MAX_LINES, MAX_TEXT, Outcome, Source};
use crate::{
    navidrome::api::{Client, Song},
    settings::ProxyConfig,
};
use serde::Deserialize;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

const MAX_BODY: usize = 4 * MAX_TEXT;
const SPACING: Duration = Duration::from_millis(500);
pub struct Lookup {
    endpoint: String,
    timeout: Duration,
    gate: Mutex<Gate>,
}
struct Gate {
    ready: Instant,
    blocked: Option<Instant>,
    client: Option<(ProxyConfig, reqwest::Client)>,
}
impl Default for Lookup {
    fn default() -> Self {
        Self {
            endpoint: "https://lrclib.net/api/get".into(),
            timeout: Duration::from_secs(10),
            gate: Mutex::new(Gate {
                ready: Instant::now(),
                blocked: None,
                client: None,
            }),
        }
    }
}

pub async fn fetch(
    client: &Client,
    song: &Song,
    provider: &Lookup,
    proxy: Option<&ProxyConfig>,
) -> Result<(Source, Outcome), String> {
    let server = super::fetch_server(client, song).await?;
    if server != Outcome::Missing || proxy.is_none() {
        return Ok((Source::Server, server));
    }
    provider
        .fetch(song, proxy.unwrap())
        .await
        .map(|outcome| (Source::Lrclib, outcome))
}

impl Lookup {
    async fn fetch(&self, song: &Song, proxy: &ProxyConfig) -> Result<Outcome, String> {
        if [&song.title, &song.artist, &song.album]
            .iter()
            .any(|s| s.trim().is_empty() || s.len() > 512)
            || !song.duration.is_finite()
            || !(1.0..=3600.0).contains(&song.duration)
        {
            return Ok(Outcome::Ambiguous);
        }
        let mut gate = self.gate.lock().await;
        if gate.blocked.is_some_and(|until| until > Instant::now()) {
            return Err("LRCLIB asked us to wait. Try again later.".into());
        }
        tokio::time::sleep_until(gate.ready.into()).await;
        if gate
            .client
            .as_ref()
            .is_none_or(|(configured, _)| configured != proxy)
        {
            let client = crate::http::client_builder(proxy)?
                .user_agent(concat!(
                    "Navifast/",
                    env!("CARGO_PKG_VERSION"),
                    " (",
                    env!("CARGO_PKG_REPOSITORY"),
                    ")"
                ))
                .redirect(reqwest::redirect::Policy::none())
                .timeout(self.timeout)
                .build()
                .map_err(|_| "Could not connect to LRCLIB.")?;
            gate.client = Some((proxy.clone(), client));
        }
        // Keep the delay even if changing songs aborts this request.
        gate.ready = Instant::now() + SPACING;
        let client = &gate.client.as_ref().unwrap().1;
        let result = client
            .get(&self.endpoint)
            .query(&[
                ("artist_name", song.artist.clone()),
                ("track_name", song.title.clone()),
                ("album_name", song.album.clone()),
                ("duration", song.duration.to_string()),
            ])
            .send()
            .await;
        gate.ready = Instant::now() + SPACING;
        let mut response = result.map_err(|error| {
            if error.is_timeout() {
                "LRCLIB did not respond in time. Try again."
            } else {
                "Could not connect to LRCLIB. Try again."
            }
        })?;
        let status = response.status();
        if status.as_u16() == 429 || status.as_u16() == 503 {
            let delay = response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|s| s.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(60);
            // No automatic retry. Jitter also spreads explicit retries after the limit.
            let jitter = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| u64::from(d.subsec_millis()) % 500);
            gate.blocked = Some(
                Instant::now()
                    .checked_add(
                        Duration::from_secs(delay).saturating_add(Duration::from_millis(jitter)),
                    )
                    .unwrap_or_else(|| Instant::now() + Duration::from_secs(365 * 24 * 3600)),
            );
            return Err("LRCLIB asked us to wait. Try again later.".into());
        }
        if status == reqwest::StatusCode::NOT_FOUND {
            return Ok(Outcome::Missing);
        }
        if !status.is_success() {
            return Err("LRCLIB could not complete the lookup. Try again.".into());
        }
        if response
            .content_length()
            .is_some_and(|n| n > MAX_BODY as u64)
        {
            return Err("LRCLIB's lyrics are too large.".into());
        }
        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| "Could not read LRCLIB's response.")?
        {
            if body.len().saturating_add(chunk.len()) > MAX_BODY {
                return Err("LRCLIB's lyrics are too large.".into());
            }
            body.extend_from_slice(&chunk);
        }
        let record: Record =
            serde_json::from_slice(&body).map_err(|_| "LRCLIB returned invalid lyrics.")?;
        record.lyrics(song)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Record {
    track_name: String,
    artist_name: String,
    album_name: String,
    duration: f64,
    #[serde(default)]
    instrumental: bool,
    plain_lyrics: Option<String>,
    synced_lyrics: Option<String>,
    lyricsfile: Option<String>,
}
fn normalized(value: &str) -> String {
    value
        .split_whitespace()
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join(" ")
}
impl Record {
    fn lyrics(self, song: &Song) -> Result<Outcome, String> {
        // Preserve qualifiers such as Live/Remastered and all performer names.
        // Missing album/duration metadata is not enough to pick a recording.
        if normalized(&self.artist_name) != normalized(&song.artist)
            || normalized(&self.track_name) != normalized(&song.title)
            || normalized(&self.album_name) != normalized(&song.album)
            || !self.duration.is_finite()
            || (self.duration - song.duration).abs() > 2.0
        {
            return Ok(Outcome::Ambiguous);
        }
        if self.instrumental {
            return Ok(Outcome::Instrumental);
        }
        let file = self
            .lyricsfile
            .as_deref()
            .and_then(super::lyricsfile::parse);
        if matches!(&file, Some(Outcome::Found(lyrics)) if lyrics.timed) {
            return Ok(file.unwrap());
        }
        if let Some(synced) = self.synced_lyrics.as_deref().and_then(lrc) {
            return Ok(synced);
        }
        if let Some(file) = file {
            return Ok(file);
        }
        super::plain(serde_json::json!({"lyrics":{"value":self.plain_lyrics}}))
            .map_err(|_| "LRCLIB returned invalid lyrics.".into())
    }
}

fn timestamp(value: &str) -> Option<u64> {
    let (minutes, seconds) = value.split_once(':')?;
    let minutes = minutes.parse::<u64>().ok()?;
    let (seconds, fraction) = seconds.split_once('.').unwrap_or((seconds, ""));
    let seconds = seconds.parse::<u64>().ok()?;
    if seconds >= 60 || fraction.len() > 3 || !fraction.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let millis = if fraction.is_empty() {
        0
    } else {
        fraction.parse::<u64>().ok()? * 10u64.pow(3 - fraction.len() as u32)
    };
    minutes
        .checked_mul(60_000)?
        .checked_add(seconds * 1000 + millis)
}
fn lrc(text: &str) -> Option<Outcome> {
    if text.len() > MAX_TEXT {
        return None;
    }
    let mut lines = Vec::new();
    let mut text_bytes: usize = 0;
    for raw in text.lines().filter(|s| !s.trim().is_empty()) {
        if ["[ar:", "[ti:", "[al:", "[by:", "[re:", "[ve:", "[length:"]
            .iter()
            .any(|tag| raw.starts_with(tag))
            || raw == "[offset:0]"
        {
            continue;
        }
        let mut rest = raw;
        let mut times = Vec::new();
        while let Some(tag) = rest.strip_prefix('[') {
            if times.len() == MAX_LINES {
                return None;
            }
            let (value, suffix) = tag.split_once(']')?;
            times.push(timestamp(value)?);
            rest = suffix;
        }
        if times.is_empty() {
            return None;
        }
        for time in times {
            if lines.len() == MAX_LINES {
                return None;
            }
            text_bytes = text_bytes.checked_add(rest.len())?;
            if text_bytes > MAX_TEXT {
                return None;
            }
            lines.push(super::Line {
                text: rest.to_owned(),
                at_ms: Some(time),
                words: vec![],
            });
        }
    }
    if lines.is_empty() {
        return None;
    }
    lines.sort_by_key(|line| line.at_ms);
    if lines.iter().map(|l| l.text.len()).sum::<usize>() > MAX_TEXT {
        return None;
    }
    Some(Outcome::Found(super::Lyrics {
        lines,
        timed: true,
        invalid_timing: false,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::navidrome::test_server::{Server, response};
    use serde_json::json;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    fn song() -> Song {
        Song {
            id: "private-id".into(),
            title: "Home (Live)".into(),
            artist: "A & B".into(),
            album: "Evening".into(),
            duration: 180.,
            ..Default::default()
        }
    }
    fn record() -> serde_json::Value {
        json!({"trackName":"Home (Live)","artistName":"A & B","albumName":"Evening","duration":180,"instrumental":false,"plainLyrics":"A light\nAt home","syncedLyrics":"[00:01.00]A light\n[00:02.50]At home","lyricsfile":null})
    }
    fn lookup(server: &Server) -> Lookup {
        Lookup {
            endpoint: format!("{}/api/get", server.url),
            ..Default::default()
        }
    }
    fn navidrome(lyrics: &'static str) -> Server {
        Server::spawn(move |request| {
            let body = if request.contains("getOpenSubsonicExtensions") {
                json!({"subsonic-response":{"status":"ok","openSubsonicExtensions":[]}})
            } else {
                json!({"subsonic-response":{"status":"ok","lyrics":{"value":lyrics}}})
            };
            response("200 OK", "", &serde_json::to_vec(&body).unwrap())
        })
    }
    fn client(server: &Server) -> Client {
        Client::new(
            &server.url,
            "private-user",
            "private-password".into(),
            &ProxyConfig::Off,
        )
        .unwrap()
    }
    #[tokio::test]
    async fn server_hit_and_disabled_provider_make_zero_external_requests() {
        let count = Arc::new(AtomicUsize::new(0));
        let calls = count.clone();
        let external = Server::spawn(move |_| {
            calls.fetch_add(1, Ordering::SeqCst);
            response("404 Not Found", "", b"")
        });
        let provider = lookup(&external);
        let server = navidrome("Server words");
        assert!(matches!(
            fetch(
                &client(&server),
                &song(),
                &provider,
                Some(&ProxyConfig::Off)
            )
            .await
            .unwrap(),
            (Source::Server, Outcome::Found(_))
        ));
        let server = navidrome("");
        assert_eq!(
            fetch(&client(&server), &song(), &provider, None)
                .await
                .unwrap(),
            (Source::Server, Outcome::Missing)
        );
        assert_eq!(count.load(Ordering::SeqCst), 0);
    }
    #[tokio::test]
    async fn provider_receives_only_recording_metadata_and_line_timing_stays_line_only() {
        let external = Server::spawn(|request| {
            let url = reqwest::Url::parse(&format!(
                "http://localhost{}",
                request.split_whitespace().nth(1).unwrap()
            ))
            .unwrap();
            let parameters: std::collections::HashMap<_, _> = url.query_pairs().collect();
            assert_eq!(parameters.len(), 4);
            assert_eq!(parameters["artist_name"], "A & B");
            assert_eq!(parameters["track_name"], "Home (Live)");
            assert_eq!(parameters["album_name"], "Evening");
            assert_eq!(parameters["duration"], "180");
            assert!(!request.to_lowercase().contains("authorization"));
            assert!(!request.contains("private-"));
            assert!(request.contains("Navifast/"));
            response(
                "200 OK",
                "Content-Type: application/json\r\n",
                &serde_json::to_vec(&record()).unwrap(),
            )
        });
        let server = navidrome("");
        let (source, Outcome::Found(lyrics)) = fetch(
            &client(&server),
            &song(),
            &lookup(&external),
            Some(&ProxyConfig::Off),
        )
        .await
        .unwrap() else {
            panic!()
        };
        assert_eq!(source, Source::Lrclib);
        assert!(lyrics.timed);
        assert_eq!(lyrics.lines[1].at_ms, Some(2500));
        assert!(lyrics.lines.iter().all(|line| line.words.is_empty()));
    }
    #[test]
    fn covers_remasters_albums_and_durations_must_match_without_fuzzy_guessing() {
        for (key, value) in [
            ("artistName", json!("A")),
            ("trackName", json!("Home")),
            ("albumName", json!("Evening (Remastered)")),
            ("duration", json!(190)),
        ] {
            let mut r = record();
            r[key] = value;
            assert_eq!(
                serde_json::from_value::<Record>(r)
                    .unwrap()
                    .lyrics(&song())
                    .unwrap(),
                Outcome::Ambiguous
            );
        }
        let mut r = record();
        r["syncedLyrics"] = json!(null);
        let Outcome::Found(lyrics) = serde_json::from_value::<Record>(r)
            .unwrap()
            .lyrics(&song())
            .unwrap()
        else {
            panic!()
        };
        assert!(!lyrics.timed);
    }
    #[tokio::test]
    async fn missing_rate_limited_redirected_and_slow_responses_are_bounded() {
        let missing = Server::spawn(|_| response("404 Not Found", "", b""));
        assert_eq!(
            lookup(&missing)
                .fetch(&song(), &ProxyConfig::Off)
                .await
                .unwrap(),
            Outcome::Missing
        );
        let count = Arc::new(AtomicUsize::new(0));
        let calls = count.clone();
        let limited = Server::spawn(move |_| {
            calls.fetch_add(1, Ordering::SeqCst);
            response("429 Too Many Requests", "Retry-After: 60\r\n", b"not JSON")
        });
        let provider = lookup(&limited);
        for _ in 0..2 {
            assert!(
                provider
                    .fetch(&song(), &ProxyConfig::Off)
                    .await
                    .unwrap_err()
                    .contains("wait")
            );
        }
        assert_eq!(count.load(Ordering::SeqCst), 1);
        let redirect =
            Server::spawn(|_| response("302 Found", "Location: https://example.invalid/\r\n", b""));
        assert!(
            lookup(&redirect)
                .fetch(&song(), &ProxyConfig::Off)
                .await
                .is_err()
        );
        let slow = Server::spawn(|_| {
            std::thread::sleep(Duration::from_millis(150));
            response("404 Not Found", "", b"")
        });
        let provider = Lookup {
            timeout: Duration::from_millis(30),
            ..lookup(&slow)
        };
        assert!(
            provider
                .fetch(&song(), &ProxyConfig::Off)
                .await
                .unwrap_err()
                .contains("in time")
        );
    }
    #[test]
    fn lrc_expansion_has_a_text_budget_and_preserves_repeated_timestamps() {
        assert!(lrc(&format!("{}{}", "[00:01]".repeat(100), "x".repeat(100_000))).is_none());
        let Outcome::Found(lyrics) = lrc("[00:01.5][00:04]Return\n[00:02]Home").unwrap() else {
            panic!()
        };
        assert_eq!(
            lyrics
                .lines
                .iter()
                .map(|l| l.at_ms.unwrap())
                .collect::<Vec<_>>(),
            [1500, 2000, 4000]
        );
        assert!(lrc("[00:99]invalid").is_none());
    }

    #[tokio::test]
    async fn canceling_a_lookup_releases_its_request_and_throttle_lock() {
        let (started, start) = tokio::sync::oneshot::channel();
        let (release, released) = std::sync::mpsc::channel();
        let mut started = Some(started);
        let server = Server::spawn(move |_| {
            if let Some(started) = started.take() {
                let _ = started.send(());
                released.recv_timeout(Duration::from_secs(5)).unwrap();
            }
            response("404 Not Found", "", b"")
        });
        let provider = Arc::new(lookup(&server));
        let pending = provider.clone();
        let task = tokio::spawn(async move { pending.fetch(&song(), &ProxyConfig::Off).await });
        start.await.unwrap();
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        release.send(()).unwrap();
        assert_eq!(
            provider.fetch(&song(), &ProxyConfig::Off).await.unwrap(),
            Outcome::Missing
        );
    }
}
