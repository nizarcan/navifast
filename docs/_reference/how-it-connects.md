---
title: How It Connects
description: How Navidrome requests, local playback and the inherited interface connect.
nav_order: 1
---

## Current Navidrome path

The default launch runs Rune through `src/app/navidrome/desktop.rs`. Views in
`src/ui/desktop/` render the server sign-in form, library and player. Their
actions go to the controller in `src/app/navidrome.rs`; the presentation
adapter reuses the existing models and preferences.
No Spotify service backend is included.
Artwork uses opaque, session-scoped image-loader keys rather than authenticated
URLs, with bounded CPU/GPU caching. Full decoded covers have a 32 MiB memory
budget. A separate 64 MiB cache retains downloaded bytes and small previews, so
scrolling back paints a preview immediately while the worker restores the full
cover without another server request. Both caches evict the least recently
viewed artwork; only a cover evicted from the second cache needs downloading
again. GPU textures are released with their corresponding images. Sign-out
clears both caches, and no artwork is written to disk. `navidrome::backend`
executes API requests on a dedicated Tokio runtime with cancellation and
session/request generations. IDs are opaque strings. Album pages use
`getAlbumList2` and `getAlbum`. Artwork is fetched and decoded off the UI.

After sign-in, opening Favorites, or a manual refresh, `getStarred2` reads
the account’s starred songs, albums and artists. The UI shares one membership
set across the overview and full grids. Cards load visible artwork from the
returned `coverArt` IDs through the same cache as the Albums page. Each item
has at most one `star` or `unstar` write in flight, using
`id` for a song, `albumId` for an album and `artistId` for an artist.
After a write, a fresh list confirms the change. Reads that still disagree
are retried with backoff. Signing out discards pending edits and reads.

Search waits for a 300 ms pause in typing or an explicit submission. It sends
`search3` and `getPlaylists` independently and filters playlist names locally.
Editing or clearing the term cancels both requests. Responses must match both
the session and the current query generation. Retry repeats only the failed
request. Opening a playlist loads its ordered songs with `getPlaylist`.
The Playlists page requests the full accessible list using `getPlaylists`,
independently of search. Refresh keeps existing rows available if it fails.
Leaving the page cancels its pending list request and ignores late results.
Artist links use the IDs from OpenSubsonic's individual credits or the older
`artistId` field. The artist page uses album-artist IDs from `getArtist` to separate
the discography from guest appearances. When the server returns only the
discography, a paginated, song-only `search3` lookup checks performer IDs and
groups appearances by album ID. Opening a release loads its songs with
`getAlbum`. Artist portraits and visible
release covers use the same bounded artwork cache as the library. The artist
header requests a 1200-pixel portrait with `getCoverArt`. Sleeves and small
portraits request 300 pixels. A separate `getArtistInfo2`
request asks for a biography and library-only similar artists. Navigating
away cancels these reads and discards late responses.

`navidrome::api::Client` uses salted Subsonic authentication and supports
server base paths. It does not follow redirects or persist request URLs.
Preferences are atomically written outside drawing. After a successful `ping`,
the login is saved in the platform credential store. Startup restores the
server, account and password together, then checks `ping` before loading the
library. The main window shows its loading state during both steps; the login
form appears only if restoration or connection cannot complete. The loading
window does not mark the session authenticated or enable playback.
Credential-store operations run on a dedicated worker; a locked store
does not delay playback or shutdown. Sign-out revokes restoration before
deleting the secret. Demo mode never restores or saves a login.

`navidrome::player` owns local transport. Output and decoding have separate
workers. A download worker reads original audio using 256 KiB HTTP byte ranges
and queues up to 4 MiB ahead independently of decoding. Including the current
block and an in-flight request, audio input holds at most 4.5 MiB. Downloads stop
at that bound even when playback is paused; audio is not cached to disk.
The decoder
converts mono/stereo input to stereo 44.1 kHz, then uses the existing EQ and
limiter. It emits 20 ms PCM blocks into a 500 ms channel. Output gathers a 200 ms
reserve at startup and after an underrun, so high sample rates or small codec
packets do not shrink the playback buffer. A short end-of-file tail drains
without filling the reserve. Buffering silence does not advance the position.
Consumed PCM feeds the visualization tap before output volume; preparing the
reserve does not advance the visualizer. Generation checks discard stale
selections and canceled reads; seeks also retire queued and in-flight byte
ranges from the old position. Cancellation wakes a waiting decoder without
waiting for the HTTP request's deadline.

See [Privacy](/privacy/) for endpoints and storage and
[Getting started](/getting-started/) for user-facing limitations.
