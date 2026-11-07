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
with the selected item ID to your server. Favorites and pending changes are
held in memory for the signed-in session. Ratings are independent.
The local queue is checkpointed in `navidrome-session.json` in the state directory
and deleted on sign-out. It contains opaque song/album/playlist IDs, occurrence
order, playback position, modes and a hash identifying the server and username.
It contains no passwords, signed URLs or audio. After that account signs in,
`getSong` validates saved songs and `getAlbum` resolves pending additions.
Playback restores paused. Adding an album also reads `getAlbum`; no server play
queue is read or saved.
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

Sign-out disables automatic sign-in, deletes the saved login, stops playback
and clears the active account, library and artwork from memory. A non-secret
revocation marker prevents restoration if the system store cannot delete the
password; a message reports that failure. Canceled in-flight audio reads may
take up to their 15-second request deadline to release their session reference.
Upstream Spotify profiles and credentials are not migrated.

Artwork is bounded in memory. Recently viewed covers retain downloaded bytes
and small previews for local restoration when scrolling back; they are cleared
on sign-out and are not written to disk. Stream input downloads up to 4 MiB ahead in
256 KiB byte ranges, plus the current block and one in-flight request. Pausing
allows that bounded reserve to fill; stopping or signing out cancels further
downloads and discards buffered audio. An in-flight request retains its existing
15-second deadline.
There is no offline audio cache. The local log records startup diagnostics,
not passwords or authentication responses. Local development tools and QA artifacts are not part of the distributed app.
