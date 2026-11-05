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

Passwords are held in memory only for the current session. Preferences contain
the server base URL, username, theme, volume and equalizer settings. Sign-out
stops playback and clears the active account, library and artwork from memory.
Canceled in-flight audio reads may take up to their 15-second request deadline
to release their session reference. No new keyring entries are created in this
stage, and upstream Spotify profiles and credentials are not migrated.

Artwork is bounded in memory; stream input downloads up to 4 MiB ahead in
256 KiB byte ranges, plus the current block and one in-flight request. Pausing
allows that bounded reserve to fill; stopping or signing out cancels further
downloads and discards buffered audio. An in-flight request retains its existing
15-second deadline.
There is no offline audio cache. The local log records startup diagnostics,
not passwords or authentication responses. The private migration plan and
local QA artifacts are ignored and are not part of the distributed app.
