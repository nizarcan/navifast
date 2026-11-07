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

In the player bar, select the sleeve to open its album. Hover over the sleeve
and select the up arrow to expand it at the bottom of the sidebar. The down
arrow on the expanded sleeve collapses it. Scrolling over the volume slider
adjusts the level in five-percent steps.

Use the star in search results, an album or artist header, a song row, or
the player bar to add or remove a favorite. For a library album or an
artist-page cover, open its context menu. Choose **Favorites** in the sidebar
to browse up to two rows each of saved songs, albums and artists. Choose
**See all** beside a heading to open the full artwork grid for that group.
Select a cover to play a song or open an album or artist. Stars update across the app
as soon as you click. If a change fails, **Retry** repeats it for that item.
Open Favorites again or select **Refresh** to fetch changes from another client.

Type in the search box above the library, or focus it with Ctrl+F (Cmd+F on
macOS). Results appear as you type. Use Up and Down to select a result and
Enter to play a song, open an album, see an artist's albums, or open a playlist
and play from a chosen song. Clicking a result also opens or plays it. If one
part of the search fails, **Retry** reloads that part while the other results
stay available.

Open **Playlists** in the sidebar to browse playlists available to your account,
including shared and smart playlists. They use the same artwork card grid as
albums. Open a card and choose **Play** to start at
the top, or double-click a particular song to start at that occurrence.
Playlists use the same artwork header, circular Play/Pause control, selection
and sortable track table as albums. Playback follows the displayed order,
including repeated songs. **Refresh** reloads the list; **Retry** recovers
a failed request without interrupting playback. A removed playlist reports
that it is unavailable, while an empty playlist shows an empty state.

Choose **Create playlist** in the playlist browser, or select tracks on an album
or playlist and use **Add to playlists** in a track's menu. Check one or more
writable destinations and choose **Add**, or enter a name and choose **Create**.
Songs are added in displayed order, including repeated tracks. Each destination
shows its own progress and result. Retry affects only that destination; after an
uncertain response it checks the server without adding the tracks again.

Use **Remove from playlist** in a selected track's menu to remove those specific
occurrences. If another client has changed the order, refresh and select them
again. The playlist's **More** menu offers **Delete playlist**, with its name in
the confirmation. Songs remain in your library. Shared or smart playlists that
are read-only can still be played.

Select an artist credit in search, on an album or playlist, or in the player
bar to explore that artist. Navifast links each artist your server identifies.
Select a sleeve in Discography to open the full release. Use **See more**
to browse the whole catalogue. The **Albums** and **Singles & EPs** filters appear
when the library has both kinds of release.

The artist header crops the portrait around its center and fades it behind the name.
When the server has no portrait, the header shows a record.

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
Use **Play next** or **Add to queue** in a song or album's menu to change what
plays next. Select several tracks to add them in their displayed order. Open
**Queue** in the player bar to move, remove or directly play an occurrence.
**Clear** removes manual additions and keeps the current song and collection.
The queue appears beside browsing on wide windows and fills the content area on
narrow windows. See [The queue's rules](/reference/queue/) for keyboard controls
and pending album behavior.

**Shuffle** in the player or a collection's action row changes the remaining
playing collection, preserving the current song, position and paused state.
Manual additions stay first. Turning it off restores the remaining collection's
original play order. **Repeat** cycles through Off, All and One; a small 1 marks
Repeat One. Explicit Next still skips to another available occurrence. Repeat All
loops the collection, with manual additions played once. A queue without an album
or playlist loops its current and upcoming manual songs instead.

Your queue and position return after restart once the same account signs in.
Playback stays paused until you choose to play. Shuffle order, repeat mode and
separate occurrences of duplicate songs are preserved. Tracks removed from the
server are skipped; starting new playback takes precedence over restoration.
Sign-out clears the saved session. Position is checkpointed every ten seconds
while playing, and on pause and orderly exit.

Open **Lyrics** in the player to read the playing song's words from your server.
The lyrics and queue share the right panel; at narrow widths they use the content
area. Timed lyrics highlight the current line. Click one to seek while keeping the
current pause state. Scrolling suspends automatic following; **Follow** returns to
the current line. Plain lyrics have no seek actions. Invalid timing keeps the words
readable and shows a notice. Missing lyrics, instrumental markers and request errors
have separate states. **Try again** retries a failed lookup.

External lyric providers, scrobbling, transcoding and tray operation are still pending.
