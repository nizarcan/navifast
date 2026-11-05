# Publishing Navifast

The first candidate is `0.1.0-rc.1`. Navifast is maintained independently from
Spotifast. Its version sequence, signatures, downloads, and app identity belong
to this project. Keep the original MIT notices and README attribution.

## One-time repository setup

1. In GitHub Actions, enable workflows for `nizarcan/navifast` if necessary.
   Confirm that a push to `main` actually starts CI. All CI jobs, including
   Linux, Windows, macOS, and Nix, must pass for the exact release commit.
2. Create the `release-signing` environment in Settings → Environments. Limit
   deployment tags to `v*`, and require maintainer approval when available.
3. Upload the project's Ed25519 private PEM key as the environment secret
   `NAVIFAST_UPDATE_SIGNING_KEY`. Its public key is committed in
   `assets/update-public-key.hex`. Keep an encrypted backup in your secret
   manager. Never commit the private key or attach it to a release.
4. For a Developer ID-signed and notarized Mac download, add the six Apple
   secrets listed in [PACKAGING.md](../PACKAGING.md#automatic-macos-notarization).
   Without them the workflow creates an ad-hoc-signed DMG, which does not
   establish a trusted developer identity for downloaded apps. The first
   public Mac release should be notarized and checked on a second Mac.
5. Windows installers are currently unsigned. Authenticode signing is optional
   for a preview, but recipients may see an unrecognized-publisher warning.
   Check the installer and portable ZIP on Windows before sharing them.

If the release key was generated during local preparation, it is in the
ignored `.release-secrets/navifast-release-key.pem` with owner-only permissions.
After creating the environment, upload it without displaying its contents:

```sh
gh auth login
gh secret set NAVIFAST_UPDATE_SIGNING_KEY \
  --repo nizarcan/navifast --env release-signing \
  < .release-secrets/navifast-release-key.pem
```

GitHub Pages is optional for publishing binaries. To deploy the documentation,
set Settings → Pages → Source to GitHub Actions. Keep the website's version
selector on Development until the first stable release exists. A prerelease
does not update Homebrew or AUR; do not enable those publishers yet.

## Build and test the Mac candidate locally

Run these commands from the repository root on an Apple Silicon Mac. Use a
fresh output directory; the bundle script replaces its target app and the DMG
script refuses to replace an existing disk image.

```sh
cargo build --locked --release --bin navifast
mkdir -p dist/v0.1.0-rc.1/macos-input
bash packaging/macos/bundle.sh target/release/navifast \
  dist/v0.1.0-rc.1/macos-input/Navifast.app 0.1.0-rc.1
cp README.md LICENSE dist/v0.1.0-rc.1/macos-input/
ruby packaging/macos/dmg.rb dist/v0.1.0-rc.1/macos-input \
  dist/v0.1.0-rc.1/navifast-v0.1.0-rc.1-aarch64-apple-darwin.dmg
```

This local build contains only the host architecture. The release workflow
builds both Mac architectures, combines them, and publishes a universal DMG.
It also builds Linux and Windows x64/ARM64 downloads. Flatpak is optional;
its absence must not be described as a successful Flatpak release.

Test server sign-in, album browsing, original high-quality audio, seeking,
pause/resume, album progression, EQ, volume, and recovery from a short network
interruption. Test the packaged application, not only `cargo run`. Demo
screenshots and unit tests do not replace playback against a real server.

## Publish after verification

1. Update `Cargo.toml`, the matching Flatpak release entry, and `Cargo.lock`.
   Refresh the vendor hash in `flake.nix` and verify it in Nix CI. Write notes
   and capture a screenshot under `packaging/release-notes/vVERSION/`.
2. Run the checks in CONTRIBUTING.md and `python3 packaging/check-release.py`.
   Commit and push the preparation to `main`. Wait for every CI job to pass.
   A release tag must point at that exact successful commit.
3. When signing secrets and native validation are ready, tag and push:

   ```sh
   git tag v0.1.0-rc.1
   git push origin v0.1.0-rc.1
   ```

4. Review the build results and approve `release-signing`. The release
   preflight refuses a tag/version mismatch or a commit without successful CI.
5. Check the published notes, screenshot, Mac/Windows/Linux downloads,
   `checksums.txt`, and `checksums.txt.sig`. Verify the manifest with
   `native-packages verify-checksums` and the committed public key. Confirm
   macOS notarization and run the downloaded packages on their native systems.

Stop here for a prerelease. A later stable release also needs the website,
Homebrew, AUR, and native package validation described in AGENTS.md and
PACKAGING.md. Do not publish generated placeholder release notes.
