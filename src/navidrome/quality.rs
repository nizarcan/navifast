//! Explicit, decoder-validated streaming choices and server-negotiated requests.
use super::api::Client;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Quality {
    #[serde(rename = "mp3_64")]
    Mp3Low,
    #[serde(rename = "mp3_128")]
    Mp3Standard,
    #[serde(rename = "mp3_192")]
    Mp3High,
    #[serde(rename = "mp3_320")]
    Mp3Maximum,
    #[default]
    #[serde(rename = "original", other)]
    Original,
}
impl Quality {
    pub const ALL: [Self; 5] = [
        Self::Original,
        Self::Mp3Low,
        Self::Mp3Standard,
        Self::Mp3High,
        Self::Mp3Maximum,
    ];
    pub fn kbps(self) -> Option<u32> {
        match self {
            Self::Original => None,
            Self::Mp3Low => Some(64),
            Self::Mp3Standard => Some(128),
            Self::Mp3High => Some(192),
            Self::Mp3Maximum => Some(320),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Capabilities {
    pub transcoding: bool,
    pub offset: bool,
}
impl Capabilities {
    pub fn can_seek(self, quality: Quality) -> bool {
        quality == Quality::Original || self.transcoding || self.offset
    }
}

/// Contains an authenticated URL and possibly an opaque server token. Never log it.
pub struct Plan {
    pub url: reqwest::Url,
    pub ranged: bool,
    pub seekable: bool,
    /// Original files seek locally; converted streams start at a server offset.
    pub decode_position: f64,
}

impl Client {
    pub async fn streaming_capabilities(&self) -> Result<Capabilities, String> {
        Ok(Capabilities {
            transcoding: self.supports_extension("transcoding", 1).await?,
            offset: self.supports_extension("transcodeOffset", 1).await?,
        })
    }
    pub async fn streaming_plan(
        &self,
        id: &str,
        quality: Quality,
        position: f64,
    ) -> Result<Plan, String> {
        let Some(kbps) = quality.kbps() else {
            return Ok(Plan {
                url: self.stream_url(id),
                ranged: true,
                seekable: true,
                decode_position: position,
            });
        };
        let capabilities = self.streaming_capabilities().await?;
        if position > 0.0 && !capabilities.can_seek(quality) {
            return Err(
                "This server cannot seek converted audio. Choose Original or restart the song."
                    .into(),
            );
        }
        let (url, ranged) = if capabilities.transcoding {
            let bitrate = kbps * 1000;
            let profile = serde_json::json!({
                "name": "Navifast", "platform": std::env::consts::OS,
                "maxAudioBitrate": bitrate, "maxTranscodingAudioBitrate": bitrate,
                "directPlayProfiles": [{"containers":["mp3"], "audioCodecs":["mp3"], "protocols":["http"], "maxAudioChannels":2}],
                "transcodingProfiles": [{"container":"mp3", "audioCodec":"mp3", "protocol":"http", "maxAudioChannels":2}],
                "codecProfiles": [{"type":"AudioCodec", "name":"mp3", "limitations":[{"name":"audioBitrate", "comparison":"LessThanEqual", "values":[bitrate.to_string()], "required":true}]}]
            });
            let body = self
                .checked_post(
                    "getTranscodeDecision",
                    &[("mediaId", id.into()), ("mediaType", "song".into())],
                    &profile,
                )
                .await
                .map_err(|e| e.to_string())?;
            let decision: Decision = serde_json::from_value(body["transcodeDecision"].clone())
                .map_err(|_| "The server returned an invalid streaming decision.".to_owned())?;
            decision.url(self, id, bitrate, position)?
        } else {
            let mut params = vec![
                ("id", id.into()),
                ("format", "mp3".into()),
                ("maxBitRate", kbps.to_string()),
            ];
            if position > 0.0 {
                params.push(("timeOffset", position.to_string()));
            }
            (self.url("stream", &params), false)
        };
        Ok(Plan {
            url,
            ranged,
            seekable: ranged || capabilities.can_seek(quality),
            decode_position: if ranged { position } else { 0.0 },
        })
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Decision {
    #[serde(default)]
    can_direct_play: bool,
    #[serde(default)]
    can_transcode: bool,
    #[serde(default)]
    transcode_params: String,
    source_stream: Stream,
    transcode_stream: Option<Stream>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Stream {
    protocol: String,
    container: String,
    codec: String,
    audio_channels: u32,
    audio_bitrate: u32,
}
impl Stream {
    fn matches(&self, bitrate: u32) -> bool {
        self.protocol == "http"
            && self.container == "mp3"
            && self.codec == "mp3"
            && (1..=2).contains(&self.audio_channels)
            && self.audio_bitrate > 0
            && self.audio_bitrate <= bitrate
    }
}
impl Decision {
    fn url(
        &self,
        client: &Client,
        id: &str,
        bitrate: u32,
        position: f64,
    ) -> Result<(reqwest::Url, bool), String> {
        let source_matches = self.can_direct_play && self.source_stream.matches(bitrate);
        let converted_matches = self.can_transcode
            && self
                .transcode_stream
                .as_ref()
                .is_some_and(|s| s.matches(bitrate));
        if source_matches {
            // The original already satisfies the requested bandwidth and codec.
            // Preserve local byte seeking and avoid a second lossy encode.
            return Ok((client.stream_url(id), true));
        }
        if !converted_matches || self.transcode_params.is_empty() {
            return Err(format!(
                "The server cannot provide MP3 at up to {} kbps. Check its MP3 encoder or choose another quality.",
                bitrate / 1000
            ));
        }
        // The opaque token is passed unchanged as a query value, never parsed or
        // reconstructed. The offset is expressed in source seconds.
        Ok((
            client.url(
                "getTranscodeStream",
                &[
                    ("mediaId", id.into()),
                    ("mediaType", "song".into()),
                    ("transcodeParams", self.transcode_params.clone()),
                    ("offset", position.to_string()),
                ],
            ),
            false,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::navidrome::test_server::{Server, response};
    use serde_json::json;
    fn client(server: &Server) -> Client {
        Client::new(
            &server.url,
            "fixture",
            "fixture-only".into(),
            &crate::settings::ProxyConfig::Off,
        )
        .unwrap()
    }
    fn json_response(value: serde_json::Value) -> Vec<u8> {
        response(
            "200 OK",
            "Content-Type: application/json\r\n",
            &serde_json::to_vec(&json!({"subsonic-response": value})).unwrap(),
        )
    }
    fn stream(codec: &str, bitrate: u32) -> serde_json::Value {
        json!({"protocol":"http", "container":codec, "codec":codec, "audioChannels":2, "audioBitrate":bitrate})
    }
    #[tokio::test]
    async fn negotiates_only_supported_decoder_profile_and_keeps_opaque_tokens() {
        let server = Server::spawn(|request| {
            if request.contains("/getOpenSubsonicExtensions.") {
                return json_response(
                    json!({"status":"ok", "openSubsonicExtensions":[{"name":"transcoding","versions":[1]}]}),
                );
            }
            assert!(request.starts_with("POST /rest/getTranscodeDecision.view?"));
            let body: serde_json::Value =
                serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
            assert_eq!(body["maxTranscodingAudioBitrate"], 128000);
            assert_eq!(body["transcodingProfiles"][0]["audioCodec"], "mp3");
            assert_eq!(
                body["transcodingProfiles"].as_array().unwrap().len(),
                1,
                "do not advertise an unimplemented Opus decoder"
            );
            json_response(
                json!({"status":"ok", "transcodeDecision":{"canTranscode":true, "transcodeParams":"opaque+%/=&token", "sourceStream":stream("flac", 900000), "transcodeStream":stream("mp3",128000)}}),
            )
        });
        let plan = client(&server)
            .streaming_plan("song / &", Quality::Mp3Standard, 12.5)
            .await
            .unwrap();
        assert!(!plan.ranged && plan.seekable);
        assert_eq!(plan.decode_position, 0.0);
        let pairs: std::collections::HashMap<_, _> = plan.url.query_pairs().into_owned().collect();
        assert_eq!(pairs["transcodeParams"], "opaque+%/=&token");
        assert_eq!(pairs["mediaId"], "song / &");
        assert_eq!(pairs["offset"], "12.5");
    }
    #[tokio::test]
    async fn legacy_quality_uses_standard_parameters_and_refuses_unsupported_seek() {
        let server = Server::spawn(|request| {
            assert!(request.contains("getOpenSubsonicExtensions"));
            json_response(json!({"status":"ok", "openSubsonicExtensions":[]}))
        });
        let client = client(&server);
        let plan = client
            .streaming_plan("song", Quality::Mp3Low, 0.0)
            .await
            .unwrap();
        assert!(!plan.seekable && !plan.ranged);
        let pairs: std::collections::HashMap<_, _> = plan.url.query_pairs().into_owned().collect();
        assert_eq!(pairs["maxBitRate"], "64");
        assert_eq!(pairs["format"], "mp3");
        assert!(
            client
                .streaming_plan("song", Quality::Mp3Low, 4.0)
                .await
                .is_err()
        );
        let original = client
            .streaming_plan("song", Quality::Original, 4.0)
            .await
            .unwrap();
        assert!(original.ranged && original.seekable);
        assert_eq!(original.decode_position, 4.0);
        assert!(
            original
                .url
                .query_pairs()
                .any(|(k, v)| k == "format" && v == "raw")
        );
    }
    #[test]
    fn server_decisions_cannot_silently_replace_selected_codec_or_bitrate() {
        let client = Client::new(
            "https://example.invalid",
            "fixture",
            "fixture-only".into(),
            &crate::settings::ProxyConfig::Off,
        )
        .unwrap();
        for (codec, bitrate) in [
            ("opus", 128000),
            ("flac", 900000),
            ("mp3", 320000),
            ("mp3", 0),
        ] {
            let decision: Decision = serde_json::from_value(json!({"canTranscode":true,"transcodeParams":"opaque","sourceStream":stream("flac",900000),"transcodeStream":stream(codec,bitrate)})).unwrap();
            assert!(decision.url(&client, "song", 128000, 0.0).is_err());
        }
        let direct: Decision = serde_json::from_value(
            json!({"canDirectPlay":true,"sourceStream":stream("mp3",64000)}),
        )
        .unwrap();
        let (url, ranged) = direct.url(&client, "song", 128000, 5.0).unwrap();
        assert!(ranged);
        assert!(url.query_pairs().any(|(k, v)| k == "format" && v == "raw"));
    }
    #[test]
    fn saved_quality_is_readable_and_unknown_values_keep_preferences_readable() {
        for quality in Quality::ALL {
            let value = serde_json::to_string(&quality).unwrap();
            assert_eq!(serde_json::from_str::<Quality>(&value).unwrap(), quality);
        }
        assert_eq!(
            serde_json::from_str::<Quality>("\"future_codec\"").unwrap(),
            Quality::Original
        );
    }
}
