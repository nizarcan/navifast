# Navifast

A native Navidrome desktop client written in Rust with egui. Navifast is
maintained independently and reuses the original interface, audio DSP and platform integrations from
[Spotifast](https://github.com/crmne/spotifast), originally by Carmine Paolino.
Original copyrights and the MIT license are retained.

## Current status

The first migration stage supports:

- Signing in to a Navidrome server, including servers hosted under a URL path.
- Browsing recently added or alphabetically sorted albums, with pagination,
  artwork and album track lists. Recently viewed covers stay visible when
  scrolling back, with full images restored from a bounded session cache.
- Streaming original audio locally, with pause, resume, seeking, previous,
  next and automatic progression through an album.
- The existing ten-band equalizer and a spectrum visualizer that follows the
  equalizer even when playback volume is zero.
- The original sidebar lists and grids, album pages, player bar and settings,
  including system/light/dark and custom themes, language, interface zoom and
  player-bar visualizers.
- Basic native media controls.

This is an early playback build. Search, favorites, playlists, queue editing,
shuffle/repeat, saved queues, lyrics, scrobbling, transcoding preferences,
Winamp/MilkDrop controls and tray behavior are still being migrated. Their
inherited code remains in the repository but these features are not available
in the normal Navidrome interface. The existing interface keeps its translations;
the server sign-in form currently uses English.

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
Xcode command-line tools, CMake and Python 3. On Debian/Ubuntu, install `build-essential`,
`pkg-config`, `cmake`, `clang`, `libclang-dev`, `libasound2-dev`, `libdbus-1-dev`,
`libpulse-dev`, `libxkbcommon-dev`, `libwayland-dev` and `libgl1-mesa-dev`.
See [CONTRIBUTING.md](CONTRIBUTING.md) for platform checks and optional MilkDrop
dependencies. Then run:

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
you in automatically on launch. **Sign out** forgets it. If the store is locked
or unavailable, playback still works and a message explains why the login could
not be saved. Passwords are never written to the settings files.
The server URL, username and playback preferences are saved in `navidrome.json`;
the existing interface and theme preferences use `settings.json`, both under
Navifast's config directory. Spotify profiles and
credentials are not imported. The app does not check for updates or contact
Spotify in normal use. See [privacy](docs/_reference/privacy.md).

## Development

See [CONTRIBUTING.md](CONTRIBUTING.md) for checks and contribution policy.
Deterministic Navidrome UI previews require the `demo` feature:

```sh
cargo run --locked --features demo --bin navifast -- --demo --demo-navidrome
```

Add `--demo-page album:alb0`, `--demo-page settings`, `--demo-show about`,
`--demo-show light,equalizer`, or `--demo-show signed-out`.
`--demo-shot PATH --demo-size 1100x760` captures a PNG.
The inherited `--demo` without `--demo-navidrome` is available for migration
comparisons and does not connect to Spotify. Legacy modules and their tests
remain until their remaining reusable features have moved over.

The first Navifast release candidate is `0.1.0-rc.1`. Release preparation does
not mean the downloads have been published: use the assets listed on
[GitHub Releases](https://github.com/nizarcan/navifast/releases), or build from
source until they are available. Earlier release notes document Spotifast history.

The Mac preview is not Developer ID-signed or notarized. After copying Navifast
from the DMG to Applications and trying to open it, macOS may require
**System Settings → Privacy & Security → Open Anyway**. See the
[preview installation guide](docs/_guide/download.md#installing-the-mac-preview).

For an optimized binary, run `cargo build --locked --release --bin navifast`.
See [PACKAGING.md](PACKAGING.md) for app bundles and
[the release checklist](packaging/RELEASING.md) for signing and publication.

---

Forked from [Spotifast](https://github.com/crmne/spotifast) by Carmine Paolino.
