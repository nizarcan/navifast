//! Facts observed at separate stages of the audio path. No URLs or credentials.
use super::quality::Quality;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Decoded {
    pub codec: String,
    pub sample_rate: u32,
    pub channels: u32,
    pub bit_depth: Option<u32>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Output {
    pub sample_rate: u32,
    pub channels: u16,
    pub sample_format: String,
}
impl Output {
    pub fn configured(stream: &rodio::OutputStream) -> Self {
        let stream = stream.config();
        Self {
            sample_rate: stream.sample_rate(),
            channels: stream.channel_count(),
            sample_format: stream.sample_format().to_string(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Playback {
    pub decoded: Decoded,
    pub output: Output,
    pub quality: Quality,
}

#[cfg(test)]
mod tests {
    #[test]
    fn source_metadata_is_optional_and_preserves_real_zero_gain() {
        use crate::navidrome::api::Song;
        let missing: Song = serde_json::from_str(r#"{"id":"test","replayGain":null}"#).unwrap();
        assert_eq!(missing.sampling_rate, None);
        assert_eq!(missing.bit_rate, None);
        assert_eq!(missing.replay_gain, None);
        let known: Song = serde_json::from_str(r#"{"id":"test","bitRate":2700,"samplingRate":96000,"bitDepth":24,"channelCount":2,"replayGain":{"trackGain":0,"albumGain":-2.5}}"#).unwrap();
        assert_eq!(known.sampling_rate, Some(96_000));
        assert_eq!(known.bit_rate, Some(2700));
        assert_eq!(known.bit_depth, Some(24));
        assert_eq!(known.channel_count, Some(2));
        let replay = known.replay_gain.unwrap();
        assert_eq!(replay.track_gain, Some(0.0));
        assert_eq!(replay.album_gain, Some(-2.5));
        assert_eq!(replay.album_peak, None);
    }
}
