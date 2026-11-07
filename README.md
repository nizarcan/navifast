# Navifast

A native Navidrome desktop client written in Rust with Rune. Navifast is
maintained independently and reuses the original interface, audio DSP and platform integrations from
[Spotifast](https://github.com/crmne/spotifast), originally by Carmine Paolino.
Original copyrights and the MIT license are retained.

## Current status

The current build supports:

- Signing in to a Navidrome server, including servers hosted under a URL path.
- Browsing recently added or alphabetically sorted albums, with pagination,
  artwork and album track lists. Recently viewed covers stay visible when
  scrolling back, with full images restored from a bounded session cache.
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
- The sidebar's recently played album order survives restarts for the last
  connected server and account.
- Streaming original audio locally, with pause, resume, seeking, previous,
  next and automatic progression through an album. Scroll over the volume slider
  to adjust it in 5% steps. Expand the playing cover into the sidebar with its arrow.
- A local queue with **Play next** and **Add to queue** for songs and albums.
  Open **Queue** in the player to play, move or remove upcoming occurrences.
  **Clear** removes manual additions while preserving the current song and collection.
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
- Basic native media controls.

This is an early playback build. Scrobbling, transcoding preferences,
and tray behavior are not available in the current Navidrome interface.
The existing interface keeps its translations; the server sign-in form
currently uses English.

Audio is decoded with Symphonia and converted to stereo 44.1 kHz for the
existing DSP. MP3, FLAC, WAV, AAC/ALAC, AIFF and Vorbis codecs are enabled;
individual container/codec combinations still need broader real-library testing.
Multichannel conversion, gapless playback and bit-perfect output are not
claimed. Original streaming requires a server/proxy that honors HTTP byte
ranges. There is no whole-file download fallback or offline cache.
Streaming downloads up to 4 MiB ahead in memory and prepares a 200 ms audio
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

This packages the current checkout with its icon and a local ad-hoc signature,
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

The first Navifast release candidate is `0.1.0-rc.1`. Release preparation does
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

---

Forked from [Spotifast](https://github.com/crmne/spotifast) by Carmine Paolino.
