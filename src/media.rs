//! Shared state and commands for desktop media controls.
//!
//! Platform modules translate this interface to MPRIS on Linux, System Media
//! Transport Controls on Windows, and Now Playing on macOS.

use std::path::PathBuf;

use crate::player::{Playback, RepeatMode};

#[derive(Clone, Debug, PartialEq)]
pub enum MediaCommand {
    Play,
    Pause,
    PlayPause,
    Stop,
    Next,
    Previous,
    SeekBy(i64),
    SetPosition { track_uri: String, position_ms: u32 },
    SetVolume(f64),
    SetShuffle(bool),
    SetRepeat(RepeatMode),
    OpenUri(String),
    Raise,
    Quit,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MediaTrack {
    pub uri: String,
    pub title: String,
    pub artists: Vec<String>,
    pub album: String,
    pub art_url: Option<String>,
    /// The same artwork on disk, once the art cache holds it. Windows and
    /// macOS take this rather than `art_url`, because they load the image
    /// themselves; MPRIS only passes the URL along, so Linux keeps using
    /// `art_url`.
    pub art_file: Option<PathBuf>,
    pub duration_ms: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MediaState {
    pub playback: Playback,
    pub track: Option<MediaTrack>,
    pub position_ms: u32,
    pub volume: f64,
    pub shuffle: bool,
    pub repeat: RepeatMode,
    pub can_control: bool,
}

impl Default for MediaState {
    fn default() -> Self {
        Self {
            playback: Playback::Stopped,
            track: None,
            position_ms: 0,
            volume: 1.0,
            shuffle: false,
            repeat: RepeatMode::Off,
            can_control: true,
        }
    }
}

// D-Bus object paths cannot contain the punctuation or Unicode in opaque
// server IDs. Encode the complete URI so identities round-trip without loss.
#[cfg(any(target_os = "linux", test))]
pub(crate) const TRACK_OBJECT_PATH_PREFIX: &str = "/io/github/nizarcan/Navifast/Track/";

#[cfg(any(target_os = "linux", test))]
pub(crate) fn track_object_path(uri: &str) -> Option<String> {
    if !uri.starts_with("navidrome:track:") {
        return None;
    }
    use std::fmt::Write;
    let mut path = format!("{TRACK_OBJECT_PATH_PREFIX}uri_");
    for byte in uri.bytes() {
        write!(path, "{byte:02x}").ok()?;
    }
    Some(path)
}

#[cfg(any(target_os = "linux", test))]
pub(crate) fn track_uri_from_path(path: &str) -> Option<String> {
    let encoded = path
        .strip_prefix(TRACK_OBJECT_PATH_PREFIX)?
        .strip_prefix("uri_")?;
    let (pairs, remainder) = encoded.as_bytes().as_chunks::<2>();
    if !remainder.is_empty() {
        return None;
    }
    let bytes: Option<Vec<u8>> = pairs
        .iter()
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok())
        .collect();
    let uri = String::from_utf8(bytes?).ok()?;
    track_object_path(&uri)?;
    Some(uri)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_track_ids_preserve_opaque_ids_and_session_identity() {
        let uris = [
            "navidrome:track:14XWXWv5FoCbFzLksawpEe",
            "navidrome:track:1:album/song-雪:_&",
            "navidrome:track:1:ab",
            "navidrome:track:1:a/b",
            "navidrome:track:2:a/b",
        ];
        let paths: Vec<_> = uris
            .iter()
            .map(|uri| track_object_path(uri).unwrap())
            .collect();
        for (uri, path) in uris.iter().zip(&paths) {
            assert_eq!(track_uri_from_path(path).as_deref(), Some(*uri));
            assert!(
                path.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '/')
            );
        }
        assert_ne!(paths[2], paths[3]);
        assert_ne!(paths[3], paths[4]);
        for path in [
            "/foreign/Track/uri_00",
            "/io/github/nizarcan/Navifast/Track/uri_1",
            "/io/github/nizarcan/Navifast/Track/uri_zz",
            "/io/github/nizarcan/Navifast/Track/uri_ff",
        ] {
            assert!(track_uri_from_path(path).is_none());
        }
    }
}
