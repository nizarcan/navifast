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

Songs enumerates `search3` with an empty query, zero artist/album counts and
60-song pages. This follows the OpenSubsonic empty-query contract and requires
the response's `openSubsonic: true` declaration; legacy servers show an unsupported
message instead of an apparently empty library. Genres uses `getGenres`, then
independent `getSongsByGenre` and `getAlbumList2(type=byGenre)` pages of 60.
Genre names are passed unchanged, including whitespace, punctuation and Unicode.
These reads share the bounded worker request slots and a navigation cancellation
group. Session, query, request and offset guards reject late pages. Failed pages
retain loaded rows; Retry repeats the failed offset, including a failed refresh.
IDs are deduplicated in server order, and a repeated page stops automatic loading.
Eight cached query slots share a 20,000-item inactive-page allowance. The active
page grows only through demand paging; leaving an oversized page evicts it.
Tables and grids virtualize their rows. Back navigation restores cached selection
and scroll position. No browse index is persisted to disk yet.

Listening stats performs a complete, sequential `search3` song scan on first entry
or explicit Refresh, on the library worker using one shared request slot. Leaving
the page or signing out cancels it. The summary is cached only for this account
session, with no background refresh or persisted listening database. Failed scans
retain the previous completed summary and timestamp. Repeated full pages, more
than one million songs or more than 64 MiB of scanned identifying metadata stop
the scan with an error instead of reporting incomplete totals as complete.

Each unique song's optional `playCount` contributes once to total plays. Missing
counts differ from zero; album and artist groups use stable IDs, with duplicate
artist credits removed per song. Ties use ID order. Every credited artist receives
the song's count, so artist totals must not be summed into account totals.
Listening time is an estimate from known duration × play count. Missing metadata
is labeled partial; no available counts is labeled unavailable. Only ten entries
per ranking reach the UI. The scan retains IDs and compact group counts, not the
full song catalogue. UTC timestamps record completed scans, not server freshness.

The Albums order menu extends `getAlbumList2` with `frequent`, `recent`, `random`
and `byYear`. Year bounds are inclusive integers from 1 to 9999 with `fromYear`
no later than `toYear`; invalid inputs never start a request. Frequent and recent
are this account's server-reported history, including other clients, not the local
sidebar's recent contexts. Discovery views share the paged-query cache and stale
response guards above while leaving the sidebar library available. Random fetches
one selection of up to 60 albums and disables further paging because independent
random requests do not define a stable continuation. This small sample stays cached
for the account session even when other query caches expire. Only explicit Refresh
requests a new sample; failures and cancelled refreshes keep the prior selection.

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
Home shares this playlist list and its cancellation group. Moving directly between
Home and Playlists retains the current request; leaving both cancels it. Startup
loads Home after authentication. Its recent collections and artist metadata live
in bounded, account-scoped preferences and reuse playback metadata without extra
lookup requests. Old album-only history is filled from already loaded albums.
Home uses virtual rows, including expanded playlist and artist sections.
Playlist edits use `createPlaylist`, `updatePlaylist`, and `deletePlaylist` on
workers. The destination list respects the server's `readonly` flag, falling
back to ownership when the flag is absent. Each write checks fresh metadata.
Appends preserve the selected order and duplicate IDs; removals use the original
server positions only after checking that the server's ordered IDs still match.
The protocol has no conditional-write revision, so this cannot lock out another
client changing the same playlist between that check and the write.

Each destination has independent progress, confirmation, and retry. An uncertain
write keeps its checkpoint: Retry reads the server and never blindly repeats an
append or creation. Creation reconciles a newly returned server ID, distinguishing
pre-existing playlists with the same name. Local rows remain visible until reads
observe the change. A rejected write restores the prior rows and offers recovery.
Signing out cancels pending work and discards account-specific checkpoints.

Queue additions are local. A whole album that is not already loaded reserves a
queue position and reads `getAlbum` on the backend worker. These reads have their
own cancellation group, independent of navigation. Clear cancels pending album
reads; sign-out discards the entire queue and its local checkpoint. No server play
queue is written. After a successful sign-in, `navidrome-session.json` is read on
a worker only for the matching server/account identity. Unique saved song IDs are
validated with `getSong`, using at most four shared request slots. Pending albums
use `getAlbum`. Missing tracks are omitted, while transport errors preserve the
checkpoint for a later launch. A new playback or queue action cancels restoration.
The restored player is loaded paused. Position and queue checkpoints are atomically
written on the backend worker; see [The queue's rules](/reference/queue/).

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

## Server lyrics

`src/navidrome/lyrics.rs` discovers the
[songLyrics extension](https://opensubsonic.netlify.app/docs/extensions/songlyrics/)
through the client's shared capability cache. Version 1 uses `getLyricsBySongId`;
version 2 adds `enhanced=true` and validates cue byte ranges against the unchanged
UTF-8 text. Unusable cues keep the parent line timing. Independent translations
and pronunciations are not mixed into the main lyric track.
A legacy server that rejects capability discovery falls back to `getLyrics`.
Transport failures remain retryable.
Responses stay scoped to the account, request generation and playing song ID.
Changing songs clears the previous text immediately. Seeks and pause changes keep
the current document; opening the panel starts a lookup only when needed.

The first valid timed document is used, falling back to the first readable plain
version. Positive server offsets make lines appear sooner. Invalid, missing or
out-of-order timestamps disable all seek/highlight actions for that document while
preserving logical text order. Empty responses mean missing lyrics; an explicit
instrumental flag or musical-note-only document has its own state. Documents are
limited to 10,000 lines and 1 MiB of text. Rune virtualizes the lines and uses a
binary search of timestamps, updating active-line highlighting from audible position.

The optional [LRCLIB API](https://lrclib.net/docs.txt) runs on the same cancellable
backend worker after a missing server result, only with explicit Settings opt-in.
It uses a separate credential-free HTTP client, no redirects, a ten-second timeout,
sequential requests with at least 500 ms spacing, and a 4 MiB response limit.
HTTP 429/503 responses honor the documented `Retry-After` seconds with jitter;
there is no automatic retry. Account changes, song changes, closing the panel and
opting out cancel pending work. Request generations also reject late responses.
Artist, title and album must match after case/whitespace normalization; duration
must differ by no more than two seconds. Qualifiers such as Live and Remastered
are preserved. Missing identifying metadata or a mismatch produces no match.

LRCLIB's [Lyricsfile 1.0](https://github.com/tranxuanthang/lyricsfile/blob/main/SPECIFICATION.md)
is parsed with bounded YAML depth, events and scalar bytes, without aliases or
includes. Unknown versions and undefined nonzero offsets fall back to legacy LRC
or plain text. Valid word text must reconstruct its line exactly. Invalid word
ranges/timing retain readable line timing, and no word timestamps are synthesized
from line duration. Logical text goes to one Rune text element with colored byte
ranges, preserving wrapping and bidi shaping. The UI repaints when the active line
or word changes, including while the listener has suspended Follow.

### Account ratings

Song and album `userRating` metadata supplies the account's initial rating.
Opening a rating menu checks `getSong` or `getAlbum` for edits made in other
clients. Choosing 1–5 or clearing to 0 calls `setRating`, through the same
worker, authenticated client and proxy policy as library requests. Favorites
use their own independent endpoints and state.

Writes serialize per entity and coalesce rapid edits to the latest intention.
All menus immediately share the chosen value. Readback after a save must agree
before the optimistic value is released; lagging reads never undo it. A definite
rejection restores the last confirmed value, reconciles it with the server, and
names the failed item with Retry. An uncertain save retains the chosen value.
Automatic verification uses bounded backoff and stops after four reads, leaving
Retry available. Closing a menu does not cancel a save. Sign-out cancels work
and clears this account's ratings; late replies cannot affect the next account.
Pending ratings are currently in memory only, not a durable offline edit queue.

## Listening reports

**Scrobble to Navidrome** is enabled by default and saved in the existing settings
file. The player sends `scrobble(submission=false)` after the first consumed audio,
and `submission=true` once a track of at least 30 seconds reaches 50% of its
metadata duration in consumed PCM. The counter excludes buffering silence, pauses
and skipped-over positions. Seeking preserves the play's count; a natural repeat
or playing again after completion starts another occurrence. Reports carry the
original playback start time in Unix milliseconds, including after seeks.

The output worker uses its existing atomic sample counter. It checks at position
updates and drains the last count on transitions, without network work in the
audio callback. At most one play's counters and metadata are retained. The library
worker serializes report requests and shares its four-request limit. At most 64
reports wait in memory; a new now-playing announcement replaces unsent older
announcements. Reports survive browsing, while disabling the preference or signing
out cancels active and queued requests. Account and preference generations reject
late audio events and responses. Disabling excludes subsequent samples; enabling
again cannot count an already submitted occurrence twice.

Up to eight reporting failures remain visible without pausing playback. A definite
rejection of a completed submission offers Retry with its original timestamp.
An uncertain result is not automatically retried because the server may already
have counted it. Now-playing failures wait for the next genuine start. Reports
and failures are not persisted across exit or disconnection yet; durable replay
belongs to offline synchronization. Navifast contacts only the configured server,
which may forward reports to scrobbling services configured on that server.
