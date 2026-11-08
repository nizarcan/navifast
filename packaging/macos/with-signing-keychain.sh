#!/bin/bash
# Use a self-signed identity on a disposable GitHub-hosted macOS runner.
# The Developer ID/notarization path remains owned by native-packages.
set -euo pipefail
umask 077

if [[ $# == 0 ]]; then
    echo "Usage: bash packaging/macos/with-signing-keychain.sh COMMAND [ARG ...]" >&2
    exit 1
fi
if [[ ${GITHUB_ACTIONS:-} != true || ${RUNNER_ENVIRONMENT:-} != github-hosted || $(uname -s) != Darwin || -z ${RUNNER_TEMP:-} ]]; then
    echo "This helper requires a disposable GitHub-hosted macOS runner." >&2
    exit 1
fi
for name in MACOS_SIGNING_CERTIFICATE_P12 MACOS_SIGNING_CERTIFICATE_PASSWORD MACOS_SIGNING_IDENTITY; do
    if [[ -z ${!name:-} ]]; then
        echo "Missing $name. Refusing to fall back to ad-hoc signing." >&2
        exit 1
    fi
done
if [[ ! "$MACOS_SIGNING_IDENTITY" =~ ^[[:xdigit:]]{40}$ ]]; then
    echo "MACOS_SIGNING_IDENTITY must be a certificate SHA-1 fingerprint." >&2
    exit 1
fi

work="$(mktemp -d "$RUNNER_TEMP/navifast-signing.XXXXXX")"
keychain="$work/signing.keychain-db"
certificate="$work/certificate.pem"
trusted=false
cleanup() {
    local status=$?
    if [[ "$trusted" == true ]]; then
        # remove-trusted-cert hangs on hosted macOS (runner-images#12116).
        # An unspecified result clears our trust grant without that API. The
        # inert settings record disappears when GitHub discards this VM.
        echo "Clearing temporary code-signing trust."
        sudo -n security add-trusted-cert -d -r unspecified -p codeSign \
            -k "$keychain" "$certificate" >/dev/null 2>&1 || true
    fi
    echo "Deleting the temporary signing Keychain."
    security delete-keychain "$keychain" >/dev/null 2>&1 || true
    rm -rf "$work"
    return "$status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

printf '%s' "$MACOS_SIGNING_CERTIFICATE_P12" | base64 --decode > "$work/identity.p12"
keychain_password="$(openssl rand -hex 32)"
echo "Creating the temporary signing Keychain."
security create-keychain -p "$keychain_password" "$keychain" >/dev/null
# codesign still needs the keychain in the user search list when --keychain
# is explicit. Preserve existing entries; delete-keychain removes only ours.
python3 - "$keychain" <<'PY'
import shlex
import subprocess
import sys
paths = shlex.split(subprocess.check_output(["security", "list-keychains", "-d", "user"], text=True))
if sys.argv[1] not in paths:
    subprocess.run(["security", "list-keychains", "-d", "user", "-s", *paths, sys.argv[1]], check=True)
PY
security set-keychain-settings -lut 3600 "$keychain"
security unlock-keychain -p "$keychain_password" "$keychain"
echo "Importing the signing identity."
security import "$work/identity.p12" -k "$keychain" \
    -P "$MACOS_SIGNING_CERTIFICATE_PASSWORD" -T /usr/bin/codesign >/dev/null
# Let Security.framework decode PKCS#12. Apple exports can use algorithms that
# the runner's OpenSSL 3 disables even though Keychain imports them correctly.
security find-certificate -a -p "$keychain" > "$certificate"
if [[ $(grep -c 'BEGIN CERTIFICATE' "$certificate") != 1 ]]; then
    echo "Expected exactly one self-signed certificate in the export." >&2
    exit 1
fi
fingerprint="$(openssl x509 -in "$certificate" -noout -fingerprint -sha1 | cut -d= -f2 | tr -d ':')"
if [[ "$fingerprint" != "$(printf '%s' "$MACOS_SIGNING_IDENTITY" | tr '[:lower:]' '[:upper:]')" ]]; then
    echo "The exported certificate does not match MACOS_SIGNING_IDENTITY." >&2
    exit 1
fi
echo "Authorizing codesign to use the imported key."
security set-key-partition-list -S apple-tool:,apple: -s \
    -k "$keychain_password" "$keychain" >/dev/null
# User-domain trust asks for GUI authorization. Use the administrator domain
# non-interactively, only on this disposable runner, and always remove it.
trusted=true
echo "Temporarily trusting the certificate for code signing."
sudo -n security add-trusted-cert -d -r trustRoot -p codeSign -k "$keychain" "$certificate"
if ! security find-identity -v -p codesigning "$keychain" | grep -F "$fingerprint" >/dev/null; then
    security find-identity -p codesigning "$keychain" >&2
    echo "The imported certificate has no usable code-signing identity." >&2
    exit 1
fi
export CODESIGN_IDENTITY="$MACOS_SIGNING_IDENTITY"
export CODESIGN_KEYCHAIN="$keychain"
unset MACOS_SIGNING_CERTIFICATE_P12 MACOS_SIGNING_CERTIFICATE_PASSWORD keychain_password
echo "Signing with the verified certificate."
"$@"
