#!/bin/bash
# Build and install this checkout on the current Mac. Never runs sudo.
set -euo pipefail

usage() {
    echo "Usage: bash packaging/macos/install.sh [applications-directory]"
    echo "Builds a release Navifast.app and installs it into /Applications by default."
}

if [[ "${1:-}" == --help || "${1:-}" == -h ]]; then
    usage
    exit 0
fi
if [[ $# -gt 1 ]]; then
    usage >&2
    exit 1
fi
if [[ "$(uname -s)" != Darwin ]]; then
    echo "This installer requires macOS." >&2
    exit 1
fi
for tool in cargo python3 codesign sips iconutil; do
    if ! command -v "$tool" >/dev/null; then
        echo "Missing $tool. See the build prerequisites in README.md." >&2
        exit 1
    fi
done

applications="${1:-/Applications}"
mkdir -p "$applications"
applications="$(cd "$applications" && pwd -P)"
destination="$applications/Navifast.app"
if [[ ! -w "$applications" ]]; then
    echo "Cannot write to $applications. Try: make install APPLICATIONS_DIR=\"\$HOME/Applications\"" >&2
    exit 1
fi
if [[ -L "$destination" || ( -e "$destination" && ! -d "$destination" ) ]]; then
    echo "Refusing to replace a symlink or non-directory at $destination." >&2
    exit 1
fi

root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$root"
mkdir -p .cache
work="$(mktemp -d "$root/.cache/macos-install.XXXXXX")"
staging=""
cleanup() {
    local status=$?
    rm -rf "$work"
    if [[ -n "$staging" ]]; then
        # If replacement failed after moving the previous app, put it back.
        if [[ -d "$staging/previous.app" && ! -e "$destination" ]]; then
            mv "$staging/previous.app" "$destination" || true
        fi
        if [[ -e "$staging/previous.app" ]]; then
            echo "Previous app preserved at $staging/previous.app" >&2
        else
            rm -rf "$staging"
        fi
    fi
    return "$status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

echo "Building Navifast from $root..."
cargo build --locked --release --bin navifast --message-format=json-render-diagnostics > "$work/build.json"
# Cargo reports the actual executable, including custom target directories and
# host-target configuration. Do not guess target/release or parse Cargo.toml.
binary="$(python3 - "$work/build.json" <<'PY'
import json
import sys

binary = None
with open(sys.argv[1]) as messages:
    for line in messages:
        message = json.loads(line)
        target = message.get("target", {})
        if (message.get("reason") == "compiler-artifact"
                and target.get("name") == "navifast"
                and "bin" in target.get("kind", [])
                and message.get("executable")):
            binary = message["executable"]
if binary is None:
    sys.exit("Cargo did not report a Navifast executable.")
print(binary)
PY
)"
reported_version="$("$binary" --version)"
version="${reported_version#navifast }"
if [[ "$reported_version" != "navifast $version" || ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.+-]+)?$ ]]; then
    echo "The built executable did not report a valid Navifast version." >&2
    exit 1
fi

# Stage on the destination filesystem, then rename the complete, verified app.
# Keep icon generation and Cargo diagnostics out of the system temp directory.
staging="$(mktemp -d "$applications/.navifast-install.XXXXXX")"
TMPDIR="$work" bash packaging/macos/bundle.sh "$binary" "$staging/Navifast.app" "$version"
codesign --verify --deep --strict "$staging/Navifast.app"
if [[ "$("$staging/Navifast.app/Contents/MacOS/Navifast" --version)" != "$reported_version" ]]; then
    echo "The packaged executable did not report the expected version." >&2
    exit 1
fi
if [[ -e "$destination" ]]; then
    mv "$destination" "$staging/previous.app"
fi
mv "$staging/Navifast.app" "$destination"
rm -rf "$staging/previous.app"
echo "Installed Navifast $version at $destination"
echo "Quit and reopen Navifast to use this build."
