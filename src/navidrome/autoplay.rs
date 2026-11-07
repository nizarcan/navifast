//! One bounded random-library read per refill, using the ordinary account client.
use super::api::{Client, Song};

pub mod exclusions;
use exclusions::Exclusions;

const SAMPLE_SIZE: usize = 20;

impl Client {
    pub async fn autoplay_song(
        &self,
        current: &str,
        exclusions: &Exclusions,
    ) -> Result<Option<Song>, String> {
        let body = self
            .checked_request("getRandomSongs", &[("size", SAMPLE_SIZE.to_string())])
            .await
            .map_err(|error| error.to_string())?;
        let group = body
            .get("randomSongs")
            .filter(|v| v.is_object())
            .ok_or("The server returned no random song results.")?;
        let Some(values) = group.get("song").filter(|v| !v.is_null()) else {
            return Ok(None);
        };
        let songs = values
            .as_array()
            .ok_or("The server returned invalid random songs.")?;
        if songs.len() > SAMPLE_SIZE {
            return Err("The server returned too many random songs.".into());
        }
        let mut fallback = None;
        for value in songs {
            if value.get("isDir").and_then(|v| v.as_bool()) == Some(true)
                || value.get("isVideo").and_then(|v| v.as_bool()) == Some(true)
            {
                continue;
            }
            let song: Song = serde_json::from_value(value.clone())
                .map_err(|_| "The server returned invalid random song metadata.")?;
            // Match the enabled Symphonia formats. The decoder still validates
            // the actual container and codec when playback starts.
            if exclusions.blocks(&song)
                || song.id.is_empty()
                || !song.duration.is_finite()
                || song.duration <= 0.
                || !matches!(
                    song.suffix.to_ascii_lowercase().as_str(),
                    "mp3"
                        | "flac"
                        | "wav"
                        | "wave"
                        | "aac"
                        | "m4a"
                        | "m4b"
                        | "mp4"
                        | "alac"
                        | "aif"
                        | "aiff"
                        | "ogg"
                        | "oga"
                )
            {
                continue;
            }
            if song.id != current {
                return Ok(Some(song));
            }
            fallback.get_or_insert(song);
        }
        // A one-song library can continue without an unbounded retry loop.
        Ok(fallback)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{navidrome::test_server::Server, settings::ProxyConfig};

    fn response(mut body: serde_json::Value) -> Vec<u8> {
        if body.get("status").is_none() {
            body["status"] = "ok".into();
        }
        crate::navidrome::test_server::response(
            "200 OK",
            "Content-Type: application/json\r\n",
            &serde_json::to_vec(&serde_json::json!({"subsonic-response":body})).unwrap(),
        )
    }

    #[tokio::test]
    async fn random_library_read_is_bounded_filters_unplayable_and_prefers_another_song() {
        let server = Server::spawn(|request| {
            assert!(request.contains("getRandomSongs.view"));
            assert!(request.contains("size=20"));
            response(serde_json::json!({"randomSongs":{"song":[
                {"id":"current","duration":60,"suffix":"flac"},
                {"id":"video","duration":60,"suffix":"mp4","isVideo":true},
                {"id":"unsupported","duration":60,"suffix":"wma"},
                {"id":"silent","duration":0,"suffix":"flac"},
                {"id":"next/夜","duration":70,"suffix":"FLAC"}
            ]}}))
        });
        let client = Client::new(
            &server.url,
            "listener",
            "dummy".into(),
            &ProxyConfig::default(),
        )
        .unwrap();
        assert_eq!(
            client
                .autoplay_song("current", &Exclusions::default())
                .await
                .unwrap()
                .unwrap()
                .id,
            "next/夜"
        );
    }

    #[tokio::test]
    async fn random_one_song_empty_and_failed_libraries_terminate_without_retrying() {
        for (body, expected) in [
            (
                serde_json::json!({"randomSongs":{"song":[{"id":"only","duration":30,"suffix":"mp3"}]}}),
                Some("only"),
            ),
            (serde_json::json!({"randomSongs":{}}), None),
        ] {
            let server = Server::spawn(move |_| response(body.clone()));
            let client = Client::new(
                &server.url,
                "listener",
                "dummy".into(),
                &ProxyConfig::default(),
            )
            .unwrap();
            assert_eq!(
                client
                    .autoplay_song("only", &Exclusions::default())
                    .await
                    .unwrap()
                    .map(|s| s.id),
                expected.map(str::to_owned)
            );
        }
        let server = Server::spawn(|_| {
            response(
                serde_json::json!({"status":"failed","error":{"code":50,"message":"private server detail"}}),
            )
        });
        let client = Client::new(
            &server.url,
            "listener",
            "dummy".into(),
            &ProxyConfig::default(),
        )
        .unwrap();
        assert!(
            client
                .autoplay_song("only", &Exclusions::default())
                .await
                .is_err()
        );
    }
    #[tokio::test]
    async fn excluded_samples_stop_after_one_request_and_never_fall_back_to_excluded_current() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        for all in [false, true] {
            let hits = Arc::new(AtomicUsize::new(0));
            let counted = hits.clone();
            let server = Server::spawn(move |_| {
                counted.fetch_add(1, Ordering::SeqCst);
                response(serde_json::json!({"randomSongs":{"song":[
                    {"id":"current","duration":60,"suffix":"flac"},
                    {"id":"album-song","albumId":"excluded-album","duration":60,"suffix":"flac"},
                    {"id":"guest-song","artists":[{"id":"guest"}],"duration":60,"suffix":"flac"},
                    {"id":"album-artist-song","albumArtists":[{"id":"guest"}],"duration":60,"suffix":"flac"},
                    {"id":"allowed","duration":60,"suffix":"flac"}
                ]}}))
            });
            let client =
                Client::new(&server.url, "listener", "dummy".into(), &ProxyConfig::Off).unwrap();
            let mut rules = exclusions::Store::default();
            for (kind, id) in [
                (super::super::api::FavoriteKind::Song, "current"),
                (super::super::api::FavoriteKind::Album, "excluded-album"),
                (super::super::api::FavoriteKind::Artist, "guest"),
            ] {
                rules.set(&client.scope, kind, id, id, true).unwrap();
            }
            if all {
                rules
                    .set(
                        &client.scope,
                        super::super::api::FavoriteKind::Song,
                        "allowed",
                        "Allowed",
                        true,
                    )
                    .unwrap();
            }
            let result = client
                .autoplay_song("current", rules.get(&client.scope).unwrap())
                .await
                .unwrap();
            assert_eq!(result.map(|s| s.id), (!all).then(|| "allowed".to_owned()));
            assert_eq!(hits.load(Ordering::SeqCst), 1);
        }
    }
    #[tokio::test]
    #[ignore = "requires the disposable local Navidrome fixture"]
    async fn local_navidrome_autoplay_exclusions_filter_the_real_library_sample() {
        use crate::navidrome::api::FavoriteKind;
        let server = std::env::var("NAVIFAST_TEST_SERVER").unwrap();
        assert!(server.starts_with("http://127.0.0.1:"));
        let client = Client::new(
            &server,
            &std::env::var("NAVIFAST_TEST_USER").unwrap(),
            std::env::var("NAVIFAST_TEST_PASSWORD").unwrap(),
            &ProxyConfig::Off,
        )
        .unwrap();
        let song = client
            .autoplay_song("", &Exclusions::default())
            .await
            .unwrap()
            .unwrap();
        for (kind, id) in [
            (FavoriteKind::Song, song.id.as_str()),
            (FavoriteKind::Album, song.album_id.as_deref().unwrap()),
            (FavoriteKind::Artist, song.credits.ids().next().unwrap()),
        ] {
            let mut store = exclusions::Store::default();
            store.set(&client.scope, kind, id, id, true).unwrap();
            assert!(
                client
                    .autoplay_song(&song.id, store.get(&client.scope).unwrap())
                    .await
                    .unwrap()
                    .is_none()
            );
        }
    }
}
