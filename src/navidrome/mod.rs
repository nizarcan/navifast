//! Native Navidrome playback and library browsing.

pub mod albums;
pub mod api;
pub mod backend;
pub mod browse;
pub mod lyrics;
pub mod player;
pub mod playlists;
pub mod queue;
pub mod ratings;
pub mod scrobble;
pub mod session;
pub mod shares;
pub mod stats;
pub mod stream;

#[cfg(test)]
pub(crate) mod test_server;
