//! Native Navidrome playback and library browsing.

pub mod albums;
pub mod api;
pub mod audio;
pub mod autoplay;
pub mod backend;
pub mod browse;
pub mod downloads;
pub mod lyrics;
pub mod player;
pub mod playlists;
pub mod quality;
pub mod queue;
pub mod ratings;
pub mod scrobble;
pub mod session;
pub mod stats;
pub mod stream;
pub mod sync;

#[cfg(test)]
pub(crate) mod test_server;
