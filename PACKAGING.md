# Packaging

[`native-packages.yaml`](native-packages.yaml) is the packaging configuration:
it pins the shared CLI and nFPM versions and declares Linux amd64/arm64 inputs,
DEB/RPM/AppImage contents, dependencies, recipe templates and downstream repositories.
Application assets and native recipes stay in `packaging/`.

The app and Linux packages use the original vinyl mark in
`packaging/icons/navifast.svg`. After editing it, run
`cargo run --locked --example app_icon` to regenerate the macOS PNG and Windows
ICO. The macOS bundle converts the PNG to ICNS. The monochrome menu-bar version
is `assets/navifast-tray.svg`.

Navifast is currently a source-built client. No Navifast release, public AUR
package, Homebrew cask or Flathub listing has been published. The recipes use
Navifast names and the application ID `io.github.nizarcan.Navifast`.

Install Navifast alongside the upstream app. Current packages install only
`navifast`, its launcher and its icon, and do not replace the upstream package,
claim its command name or migrate its Spotify profile. The Cargo compatibility
binary and older payload fixtures remain migration references. `packaging/release-notes/` contains only Navifast releases. Upstream release
history remains in the credited Spotifast repository and Git history.

`python3 packaging/test-launchers.py` exercises AUR and Flatpak installation
commands using isolated payload fixtures and Ruby to read YAML. Nix installs
the Linux desktop assets or the signed macOS bundle, without the compatibility
command. Flatpak uses its full application ID. Linux recipes also include the
optional Omarchy hook, whose active-app integration is deferred to a later stage.

```sh
gem install native-packages --version 0.8.1
native-packages validate
native-packages doctor --target linux-amd64 --target linux-arm64
native-packages build --release v1.2.3 --target linux-amd64 --target linux-arm64
```

Replace `v1.2.3` with an existing stable application release. Local use also
requires nFPM 2.47.0, `bsdtar`, `readelf` and, for the AppImage,
`mksquashfs` (squashfs-tools); AUR generation needs `makepkg`
or Docker. CI installs its tooling. To package local release archives, put
the selected target inputs and recipe assets under `dist/`, then run
`native-packages build --version 1.2.3 --target linux-amd64 --target linux-arm64`.
Outputs go to `dist/packages/1.2.3`; use `--output` for a fresh destination when rebuilding.

Before tagging, commit written notes at `packaging/release-notes/vVERSION.md`.
CI requires notes for the current Cargo version, and the release workflow
publishes that file immediately instead of a generated PR-only summary.
Verify the notes and their download links after the artifacts are published.

Stable tags run the existing native build jobs first. After binaries and
`checksums.txt` are published, the shared workflow verifies their hashes,
builds the configured packages, and attaches them to the GitHub release.
Configured recipes are attached as an archive. Package checksums are separate
from the original binary checksums. PR validation never publishes.

The Linux release runners are pinned to Ubuntu 24.04 (glibc 2.39). The native
packages target Ubuntu 24.04 / Debian 13 and newer, and Fedora 41 and newer.
Runtime-loaded GUI libraries must be declared explicitly in the YAML; ELF
inspection only discovers linked dependencies. The ALSA library mapping also
uses the Debian/Ubuntu `libasound2t64` name for this baseline.

Pushes and pull requests run packaging source regressions. Once a real fork
release exists, manually supply its version to Packaging CI, or let the release
workflow call it. It builds packages for both Linux architectures, checks them
in clean Ubuntu 24.04, Debian 13 and Fedora containers, and installs the generated
Homebrew cask on macOS. Each Linux check also installs an immutable upstream
Fastpotify 0.8.0 fixture and verifies that both applications and the upstream
settings survive Navifast installation and removal. No upstream Spotify binary
is republished as a Navifast validation release.

These checks validate installation, signatures and library resolution. They do
not validate a running desktop or Navidrome playback. Native package checks
remain required before publishing the first release.

To repeat a check locally on the matching architecture, with Docker and a C
compiler available:

```sh
bash packaging/test-install.sh ubuntu:24.04 dist/packages/1.2.3
bash packaging/test-install.sh fedora:latest dist/packages/1.2.3
```

Review or publish an existing build with the same installed CLI:

```sh
native-packages publish --from dist/packages/1.2.3 --to github --target linux-amd64 --target linux-arm64
native-packages repositories
native-packages status --offline
```

For applications with configured AUR or Homebrew destinations, stage the
recipes with `native-packages stage TARGET dist/packages/1.2.3/recipes`,
inspect `native-packages diff TARGET`, run native package validation, and
publish with `native-packages publish TARGET`. These destinations use ignored
managed Git clones, recorded in this application's YAML configuration.
AUR automation needs `PUBLISH_AUR=true`, `AUR_SSH_KEY` and `AUR_KNOWN_HOSTS`;
Homebrew automation needs `PUBLISH_HOMEBREW=true` and
`HOMEBREW_TAP_GITHUB_TOKEN`. Enable only configured destinations.

The macOS target, Windows and Flatpak build steps remain responsible
for their native artifacts. Additional nFPM formats require suitable platform
inputs and dependencies; adding a format does not port the application.
See the [shared CLI documentation](https://github.com/crmne/native-packages/tree/v0.8.1)
for commands and supported formats.

The manual **Flatpak from release** workflow can rebuild a missing Flatpak from
an existing tag. It verifies the published Linux archive against `checksums.txt`
and uses that tag's version and description with the current Flatpak app ID,
`io.github.nizarcan.Navifast`, without compiling or replacing binaries.
It uploads a workflow artifact only. After checking the bundle, attach it to the
existing release and add its hash to `checksums.txt`, preserving every existing
asset and checksum. Release tags stay immutable.

Check AUR source-directory compatibility against a downloaded source archive
with `bash packaging/test-arch-source.sh SOURCE_ARCHIVE VERSION`. It runs the
recipe's prepare/build/check directory handling with Cargo calls stubbed out;
it does not replace compilation or native package installation checks.

To upgrade the tool, change `tool.version` in `native-packages.yaml`, the matching
immutable workflow reference, and any release-job gem installation pin together.
Applications need no packaging Gemfile, lockfile or Ruby wrapper.

## Local macOS installation

From the repository root on a Mac, run:

```sh
make install
```

This builds the current checkout with `cargo build --locked --release --bin navifast`,
packages its host-architecture executable using `packaging/macos/bundle.sh`,
verifies the signature and version, and installs `/Applications/Navifast.app`.
It uses the Cargo executable on your PATH, including mise/mbx when configured,
and honors Cargo's target-directory settings. Install the pinned Rust toolchain,
Xcode command-line tools, CMake and Python 3 first, as described in the README.
No administrator command runs automatically.

For a user-owned destination, run:

```sh
make install APPLICATIONS_DIR="$HOME/Applications"
```

The equivalent direct command is
`bash packaging/macos/install.sh "$HOME/Applications"`; omitting the directory
uses `/Applications`. The existing app stays in place during compilation and
packaging. Replacement uses a staged bundle on the destination filesystem,
with the previous app restored if the final move fails. Temporary staging files
are removed afterward. Preferences and caches are unaffected. Quit and reopen
Navifast to use the new executable; the installer does not stop playback.

Local bundles use an ad-hoc signature unless a certificate is configured.
`CODESIGN_IDENTITY` selects an installed signing identity. To remember a local
choice for `make install` and `bundle.sh`, put its 40-character SHA-1 fingerprint
in `.release-secrets/macos-signing-identity` (one line). This directory is ignored
by Git. Find fingerprints with `security find-identity -v -p codesigning`.
An explicit `CODESIGN_IDENTITY` overrides the file; `CODESIGN_IDENTITY=-` selects
ad-hoc signing. CI ignores the local file. A configured identity that cannot sign
fails the build instead of silently changing the app's identity.

Using the same certificate and bundle identifier keeps the code-signing identity
stable across builds. A self-signed certificate does not guarantee persistent
Keychain approval: macOS can also restrict the saved item to the executable's
changing code hash, so an update may still prompt. It does not provide Developer
ID recognition or notarization. Test saved-login upgrades when moving to Developer ID.
Plain `cargo build` produces an unbundled executable; use `make install` for the
signed app. Installation does not notarize, publish or change the package version.

Run `python3 packaging/macos/test-install.py` for installer failure and replacement
regressions. These use isolated build and platform-tool fixtures; a real Mac build
and signature check are still required when changing the bundler.

## Self-signed macOS remote builds

The release job uses the `macos-signing` GitHub environment. Set its variable
`MACOS_SIGNING_MODE` to `self-signed` and configure these environment secrets:

- `MACOS_SIGNING_CERTIFICATE_P12`: base64 PKCS#12 containing only the selected certificate and private key.
- `MACOS_SIGNING_CERTIFICATE_PASSWORD`: the PKCS#12 export password.
- `MACOS_SIGNING_IDENTITY`: the certificate's 40-character SHA-1 fingerprint.

Restrict environment deployments to the `main` branch and `v*` release tags.
Protect those refs and review workflow changes before allowing them access to
signing keys. Never expose these secrets to pull-request builds. Export only the
intended identity, not every private key in the login Keychain.

`with-signing-keychain.sh` imports the identity into a temporary Keychain on the
disposable GitHub-hosted Mac. It checks the certificate fingerprint, grants
code-signing trust, signs using that explicit Keychain, and removes the temporary
trust and Keychain on success or failure. Missing or mismatched secrets fail.
The self-signed path does not invoke notarization, and macOS may still require
Open Anyway for downloaded previews.

Run `gh workflow run macos-signing.yml --ref main` to build and validate an app
without publishing a release. The workflow checks the final signature and
certificate, runs `--version`, and retains a ZIP artifact for seven days. It
uses the same certificate as the release job in self-signed mode.
`python3 packaging/macos/test-signing.py` covers import failure, missing secrets,
credential isolation and cleanup without accessing a real Keychain.

## Automatic macOS notarization

The macOS release job builds the app first, then selects
`macos-universal` from `native-packages.yaml` and uses `packaging/macos/dmg.rb`
to package it. Native Mac builds pass `--defer-recipes`, so they do not require
Linux inputs, AUR tooling or the not-yet-published DMG for Homebrew. The Linux
packaging job selects `linux-amd64,linux-arm64` from the same configuration
after the release assets exist and generates downstream recipes then.
The shared gem signs its owned input copy, notarizes the DMG, staples and validates
Apple's ticket, and only then records final checksums. Configure these repository
secrets, which the job exposes as environment variables:

- `APPLE_CERTIFICATE_P12`: base64 PKCS#12 Developer ID Application certificate and private key.
- `APPLE_CERTIFICATE_PASSWORD`: the export password.
- `APPLE_SIGNING_IDENTITY`: exact `Developer ID Application: Name (TEAMID)` identity.
- `APPLE_ID`, `APPLE_TEAM_ID`, `APPLE_APP_PASSWORD`: Apple email, Team ID and app-specific password.

For a production release, set `MACOS_SIGNING_MODE=developer-id` in the
`macos-signing` environment after configuring all six Apple secrets. This mode
requires every Apple credential and bypasses the self-signed identity. An unset
mode preserves the existing automatic behavior: a complete set enables
notarization, an incomplete set fails, and no values retain builds without
Developer ID signing. Explicit self-signed mode ignores Apple credentials.
Application inputs
and the user's normal keychains remain unchanged. See the shared
[Apple setup and phase contract](https://github.com/crmne/native-packages/blob/v0.8.1/docs/apple-notarization.md).

After preparing `dist/macos-input` on a Mac, test packaging without publishing:

```sh
native-packages build \
  --version 1.2.3 --target macos-universal --defer-recipes --output dist/macos-packages-test
```

Secret configuration applies to future builds. Existing published DMGs retain
their original signatures; this setup does not replace release assets.

`packaging/release-names.py DIST TAG` writes checksums for files named
`navifast-vVERSION-*`. Navifast starts its own version sequence at
`0.1.0-rc.1`; no Spotifast or Fastpotify compatibility downloads are generated.
Inputs with another identity or version fail before the manifest is written.
Run `python3 packaging/test-release-names.py` when changing this step.

Historical fixtures remain only for installation and coexistence regressions.
They are not Navifast releases. See [RELEASING.md](packaging/RELEASING.md)
for the first release's repository setup, local build, and publication steps.
