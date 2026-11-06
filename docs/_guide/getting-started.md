---
title: Getting Started
description: Connect Navifast to your Navidrome library.
nav_order: 2
---

Build this development version with `cargo run --locked --bin navifast` from
the repository. Use the compiler pinned in `rust-toolchain.toml`; see
[CONTRIBUTING.md](https://github.com/nizarcan/navifast/blob/main/CONTRIBUTING.md)
for native build dependencies. No Navifast binary release is promised yet.

Enter your Navidrome server's final base URL (for example
`https://music.example.com` or `https://example.com/music`), username and password.
Use the base URL without `/app`, `/rest`, a query string or credentials.
After a successful connection, Navifast saves the login in your system
credential store and connects automatically on the next launch. The main window
appears immediately with a loading indicator while the saved login is restored
and checked. The login form appears if there is no saved login or the connection
cannot be completed. Unlock the store if Navifast reports that it cannot remember
the login. A temporary server or network failure keeps the saved login, so you
can press **Connect** to retry.

Use **Albums** or the sidebar library list, open an album, then **Play** or
double-click a song. The sidebar retains its list/grid switch, filtering,
sorting and local pins. Its **Recently played** order remembers the last 100
albums across restarts for the last connected server and account.
More albums load as you scroll. Previous restarts the
current song after three seconds; otherwise it selects the previous song.
Next and natural completion advance through the selected album. Seeking while
paused keeps playback paused. Space toggles play/pause outside text fields.

The equalizer changes both the sound and spectrum; muting leaves the spectrum
active. Open **Settings** from the account menu for the original appearance,
system/light/dark or custom theme, language, zoom and EQ controls. These
preferences survive restarts. Sign out
forgets the saved login, stops playback and clears session metadata and artwork.

Use HTTPS for remote servers. Original playback needs byte-range support on
your server and reverse proxy. A redirect error means the final URL should be
entered directly. A failed request keeps the app usable and provides a retry.

This first stage has album playback, not the full inherited feature set.
Search, playlists, favorites, shuffle/repeat, saved queues, lyrics, scrobbling,
transcoding, Winamp/MilkDrop controls and tray operation are still pending.
