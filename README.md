# Navifast

A native Navidrome desktop client written in Rust with Rune. Navifast is
maintained independently and reuses the original interface, audio DSP and platform integrations from
[Spotifast](https://github.com/crmne/spotifast), originally by Carmine Paolino.
Original copyrights and the MIT license are retained.

## Current status

The current build supports:

- Signing in to a Navidrome server, including servers hosted under a URL path.
- Opening on **Home**, with quick access to recently played albums and playlists,
  the shared playlist artwork grid, and recently played artists. **More** expands
  a section without leaving Home. First visits offer albums from your library;
  reopening still restores playback paused.
- Browsing recently added or alphabetically sorted albums, with pagination,
  artwork and album track lists. Small rows use thumbnails; album grids keep
  Retina-ready covers, with larger images on collection pages and in the expanded
  player. Mounted artwork stays resident while older covers share a bounded
  session cache. Scrolling back shows a preview while its image is restored.
- Click an album cover to view the complete artwork fitted to the window.
  **Save image** writes a JPEG or PNG copy to a location you choose. Escape
  closes the viewer and returns focus to the cover.
- The primary menu is **Home**, **Playlists**, **Favorites**, and **Discover**.
  Discover groups Albums, Songs, Genres, and Listening stats as top pills.
- Click the Library shelf's **Albums** label to cycle **Albums → Playlists → All**.
  The choice is remembered, and search filters the selected collections. Each
  mode remembers its sort; All starts in name order and mixes albums and playlists.
  Recently added is available for albums only. The **Library options** menu
  switches between list and grid views or hides the sidebar.
  Use **Show sidebar** in the top bar to bring it back.
- Choosing **Year range**, **Frequently played**, **Recently played on server**
  or **Random selection** from the Albums order menu. Year bounds are inclusive.
  Server history belongs to this account and stays separate from local sidebar
  history. Random offers up to 60 albums and stays fixed until Refresh or sign-out.
- Opening **Discover → Listening stats** for account-wide plays, estimated listening time,
  and the ten most-played songs, albums and artists. Refresh scans the library
  on demand; missing server counts and incomplete metadata are labeled. Song
  entries open their album and select the track without starting playback.
- Browsing **Songs** and **Genres** from Discover. Songs requires the server's
  OpenSubsonic declaration; genres offer separate song and album views, including
  tracks on compilations. Pages load as you scroll, with retry and end-of-list
  feedback. Playing a row uses the currently loaded songs in their displayed
  order. The song menu opens its album when the server supplies an album ID.
- Searching songs, albums, artists and playlists from the top bar. Select a
  result with the arrow keys and press Enter to play the song or open the
  album, artist or playlist.
- Browsing **Playlists** from the sidebar, including shared and smart playlists
  available to your account. The browser uses the album artwork card grid.
  Playlist details share the album header, playback controls
  and sortable track table. Double-click a song occurrence or use its play
  control; repeated songs keep their positions in the selected play order.
- Creating named playlists and adding selected album or playlist tracks to several
  writable destinations. Each destination shows its own save status. Playlist
  menus remove selected occurrences or confirm deletion by name.
- Opening credited artists from search results, album tracks, playlists, and the
  player bar. Artist pages show Discography, About, Similar artists from your
  library, and Appears on for guest releases. The header shows the artist portrait
  when available and a record otherwise. Open a release cover to pick a song.
- Starring songs, albums and artists on your Navidrome account. Open **Favorites**
  in the sidebar for two-row artwork previews. Choose **See all** to browse
  the full grid of favorite songs, albums or artists.
- Rating songs and albums from **More** or their context menu. Choose 1–5 or
  **Clear rating**; ratings are shared across views and independent of Favorites.
  Changes made offline are kept on this device and sync after reconnecting.
  **Sync** in Settings shows pending changes, last sync and item-specific retry
  or dismissal. Favorites and ratings keep the latest intended value; completed
  listens retain separate occurrences and their original timestamps.
- The sidebar's recently played album order survives restarts for the last
  connected server and account.
- Streaming original audio locally, with pause, resume, seeking, previous,
  next and automatic progression through an album. Scroll over the volume slider
  to adjust it in 5% steps. Expand the playing cover into the sidebar with its arrow.
- Optional **Audio cache** in Settings reuses recently streamed bytes. It is off
  by default, with a 128 MiB to 8 GiB limit, usage display and Clear control.
  Eviction and clearing preserve active playback and seeking. Explicit downloads
  have separate storage; partial streams are never advertised as offline songs.
- Reporting now playing and completed listens to Navidrome. **Scrobble to Navidrome**
  in Settings is enabled by default. Tracks at least 30 seconds long count after
  50% of their duration has actually played; pauses, buffering and seek jumps
  do not count. Repeat plays count separately. Failed reporting leaves playback
  running. An uncertain submission is kept for review and is never blindly
  resent, since the server may already have counted it. Disabling scrobbling
  pauses pending completed listens; now-playing notices are never replayed.
- A local queue with **Play next** and **Add to queue** for songs and albums.
  Open **Queue** in the player to play, move or remove upcoming occurrences.
  **Clear** removes manual and automatic additions while preserving the current song and collection.
- Optional **Autoplay** in Queue continues with random songs from your own library.
  It is off by default, adds one visible suggestion after the selected queue,
  and keeps manual additions first. Clear or disable it to cancel a refill.
  Right-click a song, album or artist to **Exclude from autoplay** or **Include
  in autoplay**. Settings lists exclusions for this account on this device.
  Exclusions affect suggestions only; direct playback and manual queue additions
  keep working. Artist rules include track and album artist IDs supplied by the server.
- Shuffle keeps one visible order for remaining collection tracks, after manual
  additions. Repeat cycles through Off, All and One without changing playback position.
- The queue, position, shuffle order and repeat mode survive restart for the same
  server and account. Restoration stays paused until you choose to play.
- The existing ten-band equalizer and a spectrum visualizer that follows the
  equalizer even when playback volume is zero.
- The original sidebar lists and grids, album pages, player bar and settings,
  including system/light/dark and custom themes, language, interface zoom and
  player-bar visualizers.
- Lyrics from your Navidrome server, with timed-line highlighting, click-to-seek
  and a Follow control after reading ahead. Opt-in LRCLIB lookup can fill missing
  lyrics. Word highlighting uses genuine timestamps from either source.
- Select lyric lines and preview a local image with optional cover art and a
  choice of background. Copy it or save a PNG through the native file chooser.
- Original or MP3 streaming at up to 64, 128, 192 or 320 kbps, selected in Settings.
  Server capabilities determine whether converted audio supports seeking.
- Track details in song and queue menus, and on a right-click of the playing
  track. It separates server file metadata, decoded audio, processing format and
  configured device output, with unknown values shown explicitly.
- Download songs, albums or playlists in Original quality from their menus.
  Home → Available offline → Manage shows progress, retry, cancellation and removal.
  Overlapping selections share files; removing one keeps music owned by another.
  Use **Open downloads offline** on the sign-in screen with the same server and
  username. Completed downloads play and seek locally, including after restart.
- Basic native media controls.

This is an early playback build. Tray behavior is not available in the current
Navidrome interface.
The existing interface keeps its translations; the server sign-in form
currently uses English.

Audio is decoded with Symphonia and converted to stereo 44.1 kHz for the
existing DSP. MP3, FLAC, WAV, AAC/ALAC, AIFF and Vorbis codecs are enabled;
individual container/codec combinations still need broader real-library testing.
Queued tracks share a continuous output source with bounded preparation of the
next occurrence. Gapless software output is regression-tested with 44.1 kHz WAV,
FLAC and MP3 files carrying LAME delay/padding metadata, including conversion to
48 kHz stereo output. Other source rates, codec/container combinations and physical
device loopback are not certified. See the [validation fixtures](tests/fixtures/gapless/README.md).
Multichannel conversion and bit-perfect output are not claimed. Original streaming requires a server/proxy that honors HTTP byte
ranges. Converted MP3 streams accept unknown lengths without byte ranges.
An unsupported encoder or wrong returned codec reports an error; it never silently
falls back to a larger original file. Quality changes retain position and pause.
Servers without converted-audio seeking apply changes on the next song instead.
Opus is not offered because the current decoder does not support it. There is no
automatic whole-file streaming fallback. Explicit downloads are separate from streaming.
Original streaming downloads up to 4 MiB ahead per occurrence, with at most the playing
and next occurrence active, and prepares a 200 ms audio
reserve before starting or recovering from an interruption. This cushions brief
network delays, including with high-resolution originals. Prolonged stalls or
a connection slower than the original file's bitrate can still pause playback.

## Run

Install the Rust toolchain pinned in `rust-toolchain.toml`. On macOS, install
Xcode command-line tools and Python 3. On Debian/Ubuntu, install `build-essential`,
`pkg-config`, `libasound2-dev`, `libdbus-1-dev`,
`libpulse-dev`, `libxkbcommon-dev`, `libwayland-dev` and `libgl1-mesa-dev`.
The Linux desktop renderer also needs `libvulkan1` and a Vulkan driver for the
graphics device.
See [CONTRIBUTING.md](CONTRIBUTING.md) for platform checks. Then run:

```sh
cargo run --locked --bin navifast
```

On macOS, build an optimized app and install it into `/Applications` with:

```sh
make install
```

This packages the current checkout with its icon and the configured signing
certificate (or an ad-hoc signature when none is configured),
then replaces `Navifast.app` only after the build and bundle checks succeed.
Quit and reopen Navifast to use the new build. To install for your user only,
run `make install APPLICATIONS_DIR="$HOME/Applications"`. See
[local Mac installation](PACKAGING.md#local-macos-installation) for details.

Enter your server's final base URL, username and password. Open an album and
press **Play**, or double-click a song. Space toggles playback when a text
field is not focused. Use the transport bar to seek and adjust volume.

Navifast remembers a successful login in the system credential store and signs
you in automatically on launch. The main window opens while the saved login
connects, without flashing the login form. **Sign out** forgets it. If the store
is locked or unavailable, playback still works and a message explains why the
login could not be saved. Passwords are never written to the settings files.
The server URL, username, playback preferences and up to 100 recently played
album identifiers are saved in `navidrome.json`, along with the ten most recent
submitted searches for each server and account.
Home also keeps bounded local metadata for the last 24 collections and 24 artists
in that file. This history belongs to the saved server/account, contains no audio
or image files, and changes accounts with your login. Restored covers load as soon
as sign-in finishes. Home looks up missing photos for visible recent artists from
your server and remembers their artwork IDs for the next launch.
`navidrome-session.json` in the state directory stores the listening session using
opaque IDs and a server/account identity hash, without credentials or stream URLs.
Sign-out deletes it.
The existing interface and theme preferences use `settings.json`. Both files live under
Navifast's config directory. Spotify profiles and
credentials are not imported. The app does not check for updates or contact
Spotify in normal use. See [privacy](docs/_reference/privacy.md).

## Repository layout

| Path | Purpose |
| --- | --- |
| `src/entrypoint.rs`, `src/navifast.rs` | Application launch and command handling |
| `src/ui/desktop/` | Active Rune views |
| `src/app/navidrome.rs`, `src/app/navidrome/` | Navidrome state, actions and desktop integration |
| `src/navidrome/` | OpenSubsonic requests, streaming and local playback |
| `src/eq.rs`, `src/limiter.rs`, `src/vis.rs` | Shared audio processing and visualizers |
| `src/settings.rs`, `src/theme/`, `src/i18n.rs`, `assets/` | Shared preferences, themes, translations and assets |
| Other `src/ui/` modules | Shared layout, icon and visualizer helpers |
| `tests/`, `examples/` | Regression coverage and development utilities |
| `packaging/`, `.github/` | Current installers, release automation and CI |
| `docs/_guide/`, `docs/_reference/` | Current product guides and technical contracts |

The legacy renderer, demo, Spotify service modules and their dependencies have
been removed. Rune is the only UI framework, including in deterministic
previews. Historical implementation remains available in Git. Compatibility
command names and installation fixtures protect existing installations. Settings
retain unknown historical fields without compiling their retired features.

Keep generated binaries, dependency downloads and local QA evidence out of the
tracked source. Cargo output belongs in the ignored, mbx-managed `target/`;
packaged output belongs in ignored `dist/`. Local toolchains and reusable caches
can live in the user's cache directories. `packaging/release-notes/` contains
Navifast release notes only; upstream history remains in Git and
[Spotifast's releases](https://github.com/crmne/spotifast/releases).

## Development

See [CONTRIBUTING.md](CONTRIBUTING.md) for checks and contribution policy.
The normal build uses Rune and excludes egui, Fastframe and Spotify crates. Deterministic Navidrome UI
previews use the same dependency set with the `navidrome-demo` feature:

```sh
cargo run --locked --features navidrome-demo --bin navifast -- --demo --demo-navidrome
```

Add `--demo-page search`, `--demo-page album:alb0`, `--demo-page settings`, `--demo-show about`,
`--demo-show light,equalizer`, `--demo-show signed-out`, or
`--demo-show restoring` for the startup loading state.
`--demo-shot PATH --demo-size 1100x760` captures a PNG.
Preview sessions use isolated profiles and local fixtures. The preview feature
adds no alternate renderer or service dependencies.

The current development version is `0.2.1-rc.1`. Release preparation does
not mean the downloads have been published: use the assets listed on
[GitHub Releases](https://github.com/nizarcan/navifast/releases), or build from
source until they are available. Upstream release history is available in the
credited Spotifast repository.

The Mac preview is not Developer ID-signed or notarized. After copying Navifast
from the DMG to Applications and trying to open it, macOS may require
**System Settings → Privacy & Security → Open Anyway**. See the
[preview installation guide](docs/_guide/download.md#installing-the-mac-preview).

For an optimized binary, run `cargo build --locked --release --bin navifast`.
See [PACKAGING.md](PACKAGING.md) for app bundles and
[the release checklist](packaging/RELEASING.md) for signing and publication.
Local installations can remember a signing certificate in the ignored
`.release-secrets/macos-signing-identity` file. Using that same certificate for
local and remote preview builds keeps the signing identity stable across reinstalls.
Self-signed updates can still trigger macOS Keychain authorization prompts.
See [macOS signing setup](PACKAGING.md#self-signed-macos-remote-builds).

---

Forked from [Spotifast](https://github.com/crmne/spotifast) by Carmine Paolino.
