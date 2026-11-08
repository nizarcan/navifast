#!/bin/bash
# Use a self-signed identity on a disposable GitHub-hosted macOS runner.
# The Developer ID/notarization path remains owned by native-packages.
set -euo pipefail
umask 077

if [[ $# == 0 ]]; then
    echo "Usage: bash packaging/macos/with-signing-keychain.sh COMMAND [ARG ...]" >&2
    exit 1
fi
if [[ ${GITHUB_ACTIONS:-} != true || $(uname -s) != Darwin || -z ${RUNNER_TEMP:-} ]]; then
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
        security remove-trusted-cert "$certificate" >/dev/null 2>&1 || true
    fi
    security delete-keychain "$keychain" >/dev/null 2>&1 || true
    rm -rf "$work"
    return "$status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

printf '%s' "$MACOS_SIGNING_CERTIFICATE_P12" | base64 --decode > "$work/identity.p12"
keychain_password="$(openssl rand -hex 32)"
security create-keychain -p "$keychain_password" "$keychain" >/dev/null
security set-keychain-settings -lut 3600 "$keychain"
security unlock-keychain -p "$keychain_password" "$keychain"
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
security set-key-partition-list -S apple-tool:,apple: -s \
    -k "$keychain_password" "$keychain" >/dev/null
# Trust only code signing, only on this disposable runner. Always remove it.
trusted=true
security add-trusted-cert -r trustRoot -p codeSign -k "$keychain" "$certificate"
export CODESIGN_IDENTITY="$MACOS_SIGNING_IDENTITY"
export CODESIGN_KEYCHAIN="$keychain"
unset MACOS_SIGNING_CERTIFICATE_P12 MACOS_SIGNING_CERTIFICATE_PASSWORD keychain_password
"$@"
