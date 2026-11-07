//! Native Navidrome playback and library browsing.

pub mod api;
pub mod backend;
pub mod lyrics;
pub mod player;
pub mod playlists;
pub mod queue;
pub mod session;
pub mod stream;

#[cfg(test)]
pub(crate) mod test_server;
