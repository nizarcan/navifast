---
title: Privacy
description: What the current Navidrome client stores and sends.
nav_order: 4
---

Navifast has no analytics, telemetry, advertising or hosted service. In normal
use it contacts only the Navidrome server you configure, using your system
HTTP proxy settings. Sign-in and playback use `ping`, `getAlbumList2`,
`getAlbum`, `getCoverArt` and `stream`. Searching sends the term to `search3`
and matches playlist names locally using `getPlaylists`. Opening an artist or
playlist requests its contents with `getArtist` or `getPlaylist`. The artist
page also requests the expanded album's tracks and asks your Navidrome server
for a biography and similar artists with `getArtistInfo2`. Navidrome may use
the external metadata integrations configured by your server administrator.
Navifast loads artist images through your server's `getCoverArt` endpoint.
Navifast reads your favorites with `getStarred2` and sends `star` or `unstar`
with the selected item ID to your server. Favorites and ratings are independent. Their latest intended values and completed
listens are saved in an account-scoped local sync journal, then sent on reconnect.
Settings > Sync shows pending changes and offers Retry or Dismiss.
**Scrobble to Navidrome** is enabled by default and can be switched off in Settings.
It sends song IDs and playback start timestamps to your server's `scrobble` endpoint
for now playing and qualifying listens. It counts consumed audio, excluding pauses,
buffering and seek jumps. Your server may forward reports to external scrobbling
services you configured there. Navifast makes no direct connection to those services.
Disabling stops new reports and pauses pending completed listens. Now-playing
notices stay ephemeral. An uncertain completed listen is retained for review and
never automatically resent, since the server may already have counted it.
The local queue is checkpointed in `navidrome-session.json` in the state directory
and deleted on sign-out. It contains opaque song/album/playlist IDs, occurrence
order, playback position, modes and a hash identifying the server and username.
It contains no passwords, signed URLs or audio. After that account signs in,
`getSong` validates saved songs and `getAlbum` resolves pending additions.
Playback restores paused. Adding an album also reads `getAlbum`; no server play
queue is read or saved.
Optional **Autoplay** is off by default. It requests up to 20 random songs from
your configured server through `getRandomSongs`, choosing at most one suggestion
when the queue is about to end. It makes no request to an external music service.
Clear, disabling it, replacing playback or changing accounts cancels the refill.
The preference is saved in `navidrome.json`; suggested queue entries retain their
origin in the same local checkpoint described above.
Sign-in checks `getOpenSubsonicExtensions` using a shared client cache. Streaming
quality choices are saved with the current account in `navidrome.json`. Choosing
MP3 may send a decoder profile, requested bitrate and song ID to
`getTranscodeDecision`, then its opaque token and playback offset to
`getTranscodeStream`. Legacy servers receive `format`, `maxBitRate` and supported
`timeOffset` parameters on `stream`. These requests go only to your server; tokens
and authenticated URLs are never persisted. Converted audio uses bounded forward
reading instead of byte ranges. No third-party transcoding service is contacted.
Opening Lyrics reuses `getOpenSubsonicExtensions` for the authenticated client.
When `songLyrics` version 1 is supported, `getLyricsBySongId` receives the opaque
song ID. Version 2 adds `enhanced=true` for word timestamps. Legacy servers use
`getLyrics` with the artist and title instead. These requests go to the configured
server. Its administrator controls any server-side
external lyric integrations. The current song's lyrics stay only in memory; closing
the panel cancels its lookup, and sign-out clears the document and capability cache.
**Use LRCLIB for missing lyrics** is off by default. When explicitly enabled in
Settings, a missing server result can trigger a direct HTTPS request to
`lrclib.net/api/get`. It sends only the recording's artist, title, album and duration,
plus Navifast's name/version and project URL as the client identifier. LRCLIB also
receives the connection's IP address. No Navidrome URL, account, credentials, song
IDs, audio or artwork are sent. The configured proxy policy applies. Redirects
are refused. No provider lookup runs for a server hit or server error. Disabling
the setting cancels pending lookups and removes external lyrics immediately.
Provider lyrics remain in memory only and are never uploaded or published by Navifast.

Track details displays metadata already received from your server and observations
from the local decoder and configured output stream. Opening it makes no additional
network request and does not expose authenticated URLs, credentials or file paths.
Its selected-song snapshot stays in memory until dismissal or sign-out.

Creating a lyric image holds the selected song's lyric document and cached cover
in memory while you edit. Rendering uses the native text layout and makes no
network requests. Copy writes the PNG to the system clipboard and reads it back
to check that the copy succeeded. Save writes a PNG only to the path you choose.
An already-open draft stays intact if playback changes or lyric lookup is disabled.
Closing the selection or signing out discards the draft. Exported files and copied
images remain under your control.

It does not contact Spotify, download
web fonts, fetch external emoji or check for updates.

Subsonic authentication sends your username, a new random salt and a token
derived from your password for each request. HTTPS protects requests in
transit. Authenticated URLs are never stored in metadata, desktop media
metadata or ordinary preferences. App errors omit request URLs and untrusted
server error text. HTTP redirects are not followed. Server and proxy access
logs are controlled by the server administrator and may record request URLs.

After a successful sign-in, the server URL, username and password are saved
together in the system credential store: Keychain on macOS, Credential Manager
on Windows, or Secret Service on Linux. Navifast restores this login at startup
and contacts that server automatically. If storage is unavailable, the password
remains in memory for the session and Navifast reports the storage failure.
Preferences contain the server base URL, username, theme, volume, equalizer
settings and up to 100 recently played album identifiers, but never the password.
The recent album order stays on this computer in `navidrome.json`, survives
restarts and is cleared when a different server or account signs in successfully.
The same file keeps the ten most recent submitted search terms separately for
each server and username. You can remove individual terms from the search page.
Home keeps metadata for at most 24 recently played collections and 24 artists in
the same account-scoped file: opaque IDs, names, secondary labels and cover IDs.
Each field is limited to 1 KiB. No audio, artwork bytes or authenticated URLs are
saved with this history. Playback updates it locally; no listening report is sent.
Opening Home reads playlists with the existing `getPlaylists` request and loads
visible artwork through the same bounded cache as Albums.

Sign-out disables automatic sign-in, deletes the saved login, stops playback
and clears the active account, library and artwork from memory. A non-secret
revocation marker prevents restoration if the system store cannot delete the
password; a message reports that failure. Canceled in-flight audio reads may
take up to their 15-second request deadline to release their session reference.
Upstream Spotify profiles and credentials are not migrated.

Artwork is bounded in memory. Recently viewed covers retain downloaded bytes
and small previews for local restoration when scrolling back; they are cleared
on sign-out and are not written to disk. Original stream input downloads up to 4 MiB ahead in
256 KiB byte ranges per occurrence, plus the current block and one in-flight
request. At most the playing occurrence and one queued successor are downloaded.
Pausing cancels the prepared successor while allowing the playing track's bounded
reserve to fill. Queue edits cancel obsolete preparation; stopping or signing out cancels further
downloads and discards buffered audio. An in-flight request retains its existing
15-second deadline.
Audio cache in Settings is off by default. When enabled, streamed bytes and
range/digest metadata remain in the cache directory under hashed account, server,
song, format and quality keys. No signed URLs, credentials or tokens are saved.
The default limit is 1 GiB, adjustable from 128 MiB to 8 GiB. Least recently used
inactive entries leave first; entries expire after seven days when next accessed
or on startup. Clear and disabling remove inactive entries immediately and active
files when playback releases them, preserving play and seek. Sign-out retains
cached bytes, but other accounts cannot reuse them. Cache hits avoid audio requests;
server capability and transcoding negotiation can still contact the server.

Explicit downloads use separate state storage, persist until removed and never
count toward this cache budget. Download manifests contain song and collection
metadata. Offline playback uses only verified downloads, never partial cache data.
Both stores contain unencrypted media protected by your OS account. The sync journal
also persists across sign-out and holds item metadata, intended edits and original
listen timestamps without credentials. See [How it connects](/reference/how-it-connects/)
for bounds and cleanup details.

The local log records startup diagnostics,
not passwords or authentication responses. Local development tools and QA artifacts are not part of the distributed app.
