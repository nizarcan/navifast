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

Type in the search box above the library, or focus it with Ctrl+F (Cmd+F on
macOS). Results appear as you type. Use Up and Down to select a result and
Enter to play a song, open an album, see an artist's albums, or open a playlist
and play from a chosen song. Clicking a result also opens or plays it. If one
part of the search fails, **Retry** reloads that part while the other results
stay available.

Select an artist credit in search, on an album or playlist, or in the player
bar to explore that artist. Navifast links each artist your server identifies.
Select a sleeve in Discography to open the full release. Use **See more**
to browse the whole catalogue. The **Albums** and **Singles & EPs** filters appear
when the library has both kinds of release.

Below Discography, **About** shows the artist biography. **Similar artists**
links to artists on your server. **Appears on** shows other artists’ releases
that credit this artist on a track. Select a cover in either release shelf,
then double-click a song on its album page to play it. Back returns to your place on the
artist page while playback continues.

Clear the box to see your ten most recent submitted searches for this server
and account. Pressing Enter, choosing a result, or reusing a recent term
moves that term to the top. Use the × beside a term to remove it.

The equalizer changes both the sound and spectrum; muting leaves the spectrum
active. Open **Settings** from the account menu for the original appearance,
system/light/dark or custom theme, language, zoom and EQ controls. These
preferences survive restarts. Sign out
forgets the saved login, stops playback and clears session metadata and artwork.

Use HTTPS for remote servers. Original playback needs byte-range support on
your server and reverse proxy. A redirect error means the final URL should be
entered directly. A failed request keeps the app usable and provides a retry.

This first stage has album playback, not the full inherited feature set.
Playlist management, favorites, shuffle/repeat, saved queues, lyrics, scrobbling,
transcoding, Winamp/MilkDrop controls and tray operation are still pending.
