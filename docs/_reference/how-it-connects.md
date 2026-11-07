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
clears both caches. Artwork stays in memory unless you explicitly save a copy. `navidrome::backend`
executes API requests on a dedicated Tokio runtime with cancellation and
session/request generations. IDs are opaque strings. Album pages use
`getAlbumList2` and `getAlbum`. Artwork is fetched and decoded off the UI.

Opening an album cover requests `getCoverArt` without `size`, using its own
session-scoped Original cache key. The existing sleeve paints during loading.
The encoded response is limited to 8 MiB; decoding accepts JPEG/PNG up to
4096 pixels per dimension and 64 MiB of decoder allocation. The display texture
is reduced to a maximum 2048-pixel edge (16 MiB RGBA), fits the complete image,
and shares the existing cache budgets. Save uses the validated original bytes,
a matching extension and a native picker. One background export at a time writes
an exclusive sibling temporary file and atomically replaces the selected path.
Canceling the picker writes nothing. This is not an automatic disk artwork cache.

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
are retried with bounded backoff. Signing out cancels requests and clears the
visible account, while the durable journal retains pending edits for that account.

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

Optional Autoplay uses the core OpenSubsonic
[`getRandomSongs`](https://opensubsonic.netlify.app/docs/endpoints/getrandomsongs/)
endpoint with `size=20`. It makes at most one refill request per final occurrence,
under the same four-request worker limit, with its own cancellation group. No
external catalogue is contacted. Navigation keeps the request; Clear, disabling
Autoplay, replacing playback or changing accounts cancels it. Session and request
generations reject late results. Empty, invalid or failed samples do not retry.
Before selection, the sample is filtered against local song, album and artist
exclusions. Modern `artists` and `albumArtists` IDs from
[OpenSubsonic Child](https://opensubsonic.netlify.app/docs/responses/child/) are
checked without extra lookups; legacy `artistId` applies when modern track IDs
are absent. Missing IDs do not trigger name matching. The controller checks the
result again and invalidates old requests when rules change.

`autoplay_exclusions` in `navidrome.json` stores IDs and short display labels by
the existing server/username hash. The ordinary atomic preferences writer saves
changes. There are at most 2,000 rules across 32 accounts, with 1 KiB IDs and
256-byte UTF-8 labels. Reaching the limit reports an error and keeps existing
rules. Old preferences default to no exclusions. Settings renders eight entries
per page. Rules persist across sign-out, remain separate for another account and
send no server mutation. See [The queue's rules](/reference/queue/) for priority
and playback behavior.

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

`navidrome::player` owns local transport. Two fixed decoder workers serve the
playing occurrence and one prepared successor. Each original-audio download uses
256 KiB HTTP byte ranges with at most 4 MiB queued ahead, plus the current block
and one in-flight request (4.5 MiB per occurrence). Preparation is bounded to one
resolved next occurrence; it never skips a pending album. Streamed audio stays in
memory unless Audio cache is enabled in Settings.

Streaming quality is saved per current account in `navidrome.json`, defaulting to
Original. On sign-in, the shared `getOpenSubsonicExtensions` cache discovers
`transcoding` and `transcodeOffset`. Original uses `stream(format=raw)`. An MP3
choice negotiates a JSON MP3-only client profile through `getTranscodeDecision`
when supported, validating the declared codec, protocol, channels and bitrate.
`getTranscodeStream` receives the unchanged opaque token and source-time `offset`.
An original MP3 already within the requested limit retains the ranged path.
Legacy servers receive `stream(format=mp3,maxBitRate=...)`, with `timeOffset`
only when advertised. API decisions run on the bounded backend runtime and cancel
when obsolete. See the [transcoding contract](https://opensubsonic.netlify.app/docs/extensions/transcoding/).

Converted bodies use a forward reader, never fake byte lengths or range requests.
The decoder verifies MP3 before delivering audio. Errors do not trigger an Original
fallback. Reopening a converted stream seeks with a fresh server time offset.
Without that capability, seeking and lyric seek actions are disabled; a mid-song
quality change is saved for the next song, preserving the current stream. Supported
quality changes preserve position and paused intent and use playback generations
to reject stale replies. Paused decoding errors surface without pressing Play.
The reader checks cancellation around each read; a blocked read retains the
15-second inactivity deadline. Only bounded PCM and HTTP buffers remain in memory.

Decoders produce stereo 44.1 kHz PCM in 20 ms blocks, through a 500 ms channel per
occurrence. One continuous device source gathers a 200 ms reserve at startup or
after starvation and applies EQ and limiting across track boundaries. Limiter
lookahead is removed once at startup and drained once at the end. Metadata and
listening counters follow the valid samples through that delay. The visualizer
taps consumed, shaped audio before output volume. Preparing audio does not move
its clock or picture. A short EOF tail drains below the normal reserve.

The next track becomes current when its first valid sample reaches the output
source. Repeat One reserves a distinct listening occurrence. Queue edits and
pauses cancel prepared audio atomically; seek, skip and sign-out cancel the whole
chain. A new generation cannot reuse a reserved handoff token. Failed preparation
finishes the current tail before reporting an error for the next track. Late
preparation emits ordinary buffering silence without counting it or repeating
samples. Network cancellation interrupts decoder waits without waiting for an
in-flight HTTP request's deadline.

Gapless regression coverage uses synthetic 44.1 kHz WAV, FLAC and LAME-tagged MP3
references, plus rodio's 48 kHz stereo output conversion on macOS. Other source
rates, codec/container combinations and physical device loopback are not certified.
Symphonia's gapless option honors delay/padding in supported codec/container
pairs; enabling a decoder alone does not certify gapless support.

See [Privacy](/privacy/) for endpoints and storage and
[Getting started](/getting-started/) for user-facing limitations.

## Stream cache

Audio cache is opt-in, with a default 1 GiB budget adjustable from 128 MiB to
8 GiB. `navidrome::cache` stores disposable 256 KiB blocks under `streams/` in
the cache directory. Keys hash the account/server scope, song ID, source format,
size/duration, selected quality and converted source offset. They contain no
credentials, authenticated URLs or transcoding tokens. An atomic versioned index
records exact byte ranges, lengths and SHA-256 digests. Range gaps remain gaps;
converted bodies become reusable only after EOF and complete coverage. Cache hits
avoid audio requests; streaming capability negotiation may still contact the server.

One control worker handles configuration and cleanup; decoder workers read/write
bounded blocks. No cache I/O runs on the UI or audio callback. Index checkpoints
are batched at 1 MiB, completion and release, with usage updates limited to four
per second. Incomplete files from interrupted writes are collected on restart.
Read-time checks reject damaged blocks. Disk errors leave network streaming usable
and expose Retry in Settings. Metadata is bounded to 4,096 entries and 32,768
blocks per entry, with a 4 MiB index limit per entry.

Least recently used inactive entries leave first when space is needed or the limit
shrinks. Clear and disabling retire all entries; active leases keep their files
readable for play and seek until released, and stop saving new bytes. Usage can
temporarily exceed a reduced limit while those leases remain. Entries older than
seven days are discarded when the cache next opens or uses them. No timer or
background download fills the cache. Explicit downloads are outside this budget.
Sign-out retains cached bytes, but another account cannot reuse their keys. These
are unencrypted local media protected by the OS account. This disposable cache is
not offered as an offline library; use explicit downloads for offline playback.

## Track details

Track details reads the already loaded `Song` metadata, including optional
`bitRate`, `samplingRate`, `bitDepth`, `channelCount` and `replayGain` fields from
[OpenSubsonic Child](https://opensubsonic.netlify.app/docs/responses/child/).
No extra request, file path or authenticated URL is needed. Missing or invalid
positive measurements display Unknown; a real zero ReplayGain stays 0 dB.
ReplayGain peaks are ratios relative to full scale. These are source tags, not
an assertion that ReplayGain processing is enabled.

The decoder publishes its codec, sample rate, channels and available bit depth
on the first decoded packet and subsequent format changes, before resampling.
This is independent of the server's source metadata and selected quality. The
processing stage is stereo 44100 Hz f32. The output worker reports the configured
rodio device stream's rate, channels and sample format; the OS or hardware may
perform further conversion, so this is not a hardware DAC or bit-perfect claim.

Facts follow the playback generation. Loading or seeking clears the previous
observation; stale observations are rejected. Prepared successor facts remain
private until the sample-aligned handoff. Inspecting another song shows its
source metadata without attaching the active song's playback facts. The modal
keeps its selected song while playback changes and closes on sign-out.

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

Writes serialize and coalesce rapid edits to the latest intention. All menus
immediately share the chosen value. Readback after a save must agree before the
optimistic value is released; lagging reads never undo it. Rejected changes keep
the intended value and identify the item in Settings > Sync, with Retry or Dismiss.
A pending durable edit suppresses ordinary rating-menu refreshes. A later menu
read can observe edits from another client once local intent has been confirmed.

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
audio callback. At most one play's counters and metadata are retained. Now-playing
requests remain ephemeral, use the shared four-request limit, and replace unsent
older announcements. Their failures wait for the next genuine start.
Account and preference generations reject late audio events and responses.
Disabling excludes subsequent samples; enabling again cannot count an already
submitted occurrence twice. Navifast contacts only the configured server, which
may forward reports to services configured on that server.

## Offline synchronization

Favorites, ratings and qualifying completed listens enter an account-scoped journal
before their network mutation. The native Settings > Sync panel shows pending
count, last confirmed sync (UTC), storage errors, and up to eight actionable failures
at a time. A quiet banner links there when attention is needed. Explicit offline
mode sends nothing. Reconnect resumes the matching account's journal; signing out
retains it and signing into another account cannot replay it there.

State lives in `sync/<account hash>.json` beneath the state directory. It contains
pending values, small item metadata, original listen timestamps, up to 10,000 cached
favorites and 10,000 recently confirmed ratings. It contains no credentials or
signed URLs. This is a favorites snapshot, not a general offline catalogue.
The journal allows 10,000 pending entries and a 16 MiB serialized limit. Writes use
an exclusive sibling temporary file, flush and sync it, then atomically replace
the prior file. Disk work runs on the library runtime's blocking pool. A damaged or
incompatible file is preserved; no new work is sent until storage is writable.
A failed save keeps in-memory intent and asks the user to keep the app open,
fix storage and retry. The old durable file remains intact.

One sync job runs at a time, independent of ordinary navigation. Favorite and
rating keys coalesce by entity; each listening occurrence has a boot nonce and
occurrence key, so two plays of the same song remain separate. Now-playing notices
never enter this journal. A sending checkpoint is persisted before the request.
Desired-state jobs read first, write only if needed, and read back after success
or an uncertain acknowledgement. Retry begins with another read. A stale result
cannot retire a newer edit. A deleted item or permanent rejection fails only
that entry, leaving other work free to proceed.

Definitely unsent connection failures and desired-state verification failures
back off for 2, 4 and 8 seconds, then wait for Retry or reconnect. A restarted
sending desired-state edit is safe to reconcile. A sending listen becomes uncertain
on interruption or an ambiguous response, since OpenSubsonic supplies no scrobble
idempotency key or occurrence lookup. It is never automatically or manually resent;
Dismiss removes the local uncertainty record without changing server counts.
Definite listen rejections can be retried with their original timestamp.
Turning scrobbling off holds unsent listens; re-enabling resumes them. Canceling an
in-flight listen conservatively records uncertainty. Storage and server failures
do not pause playback.

## Explicit downloads

Download in a song, album or playlist menu saves the original file returned by
OpenSubsonic `download`, after checking the current user's `downloadRole` through
`getUser`. There is one transfer at a time on the library runtime. HTTP connects
are limited to eight seconds and stalled reads to fifteen seconds; each failed
track waits for explicit Retry. Completed tracks remain usable after a storage
failure. Downloads do not use transcoding or the optional streaming quality.

Only explicitly selected song metadata and ordered membership are persisted.
This is not a whole-library offline index. State lives under
`downloads/<account hash>/`, scoped by normalized server URL and username. Song
IDs become hashed filenames, never paths or authenticated URLs. Overlapping
selections share one Original file per song, while repeated playlist entries keep
their play order. Selection intent is atomically written before work begins;
progress stays in memory and small completion records are written per track.
The manifest is limited to 16 MiB, 1,000 selections and 10,000 unique tracks.

Transfers stream into a private `.partial` file with no whole-file memory buffer.
A completed download must match its declared length (or the original song size
for a chunked response), stay within 8 GiB and produce audio through the shipping
Symphonia decoder. Invalid, missing, unsupported and interrupted sources are never
advertised ready. The worker flushes the file and atomically publishes it only
after validation. Restart requires the completion record and a matching file size;
interrupted work restarts from zero. Cancel stops unfinished work, Retry preserves
completed files, and Remove deletes only files with no remaining selection owner.
Unowned managed files from interrupted removals are collected after a valid list
is loaded. Damaged or incompatible lists preserve their audio for recovery.

Home's Available offline shelf opens downloaded collections. Manage shows unique
bytes used and each selection's progress. The original track table and queue show
local availability. The sign-in screen's Open downloads offline uses the entered
server and username without requiring a password or contacting the network. Local
files use the same seek, EQ, visualizer and prepared playback path as streaming.
A missing local file reports an error rather than silently contacting the server
in offline mode. Reconnect restores the saved credential flow. Artwork and lyrics
are not downloaded; offline playback makes no server or LRCLIB requests. Open a
downloaded collection to resume listening after an offline launch.

Explicit downloads stay until removed, including across sign-out, and are not
subject to streaming-cache eviction. They are ordinary unencrypted media and
metadata protected by the operating system account, not a separate encrypted
library. No credentials or signed URLs are stored in download files.
