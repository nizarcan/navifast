---
title: Privacy
description: What the current Navidrome client stores and sends.
nav_order: 4
---

Navifast has no analytics, telemetry, advertising or hosted service. In normal
use it contacts only the Navidrome server you configure, using your system
HTTP proxy settings. The initial client uses `ping`, `getAlbumList2`,
`getAlbum`, `getCoverArt` and `stream`. It does not contact Spotify, download
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
Preferences contain the server base URL, username, theme, volume and equalizer
settings, but never the password.

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
not passwords or authentication responses. The private migration plan and
local QA artifacts are ignored and are not part of the distributed app.
