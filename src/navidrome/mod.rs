//! Native Navidrome playback and library browsing.

pub mod api;
pub mod backend;
pub mod player;
pub mod playlists;
pub mod stream;

#[cfg(test)]
pub(crate) mod test_server;
