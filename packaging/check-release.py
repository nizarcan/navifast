#!/usr/bin/env python3
"""Check release identity before building or publishing artifacts."""

import argparse
from pathlib import Path
import re
import tomllib
import xml.etree.ElementTree as ET


def check(root, tag=None):
    version = tomllib.loads((root / "Cargo.toml").read_text())["package"]["version"]
    if tag is not None and tag != f"v{version}":
        raise ValueError(f"Tag {tag} does not match package version v{version}")
    lock = tomllib.loads((root / "Cargo.lock").read_text())
    package = next(p for p in lock["package"] if p["name"] == "navifast")
    if package["version"] != version:
        raise ValueError("Refresh Cargo.lock for the release version")
    metadata = ET.parse(root / "packaging/flatpak/io.github.nizarcan.Navifast.metainfo.xml")
    release = metadata.find("releases/release")
    if release is None or release.get("version") != version:
        raise ValueError("Flatpak metadata must describe the current release")
    notes = root / "packaging/release-notes" / f"v{version}.md"
    if not notes.is_file() or "## Thanks" not in notes.read_text() or "Historical upstream" in notes.read_text():
        raise ValueError("Write Navifast release notes before tagging")
    if not (notes.parent / f"v{version}" / "library.png").is_file():
        raise ValueError("Capture the release screenshot before tagging")
    public_key = (root / "assets/update-public-key.hex").read_text().strip()
    if not re.fullmatch(r"[0-9a-f]{64}", public_key):
        raise ValueError("Expected a 32-byte hexadecimal Ed25519 public key")
    if public_key == "e44c87b1550484c687947bd927ae333ff431cc7adc6cc33533c628f82016b554":
        raise ValueError("Navifast needs its own release key")
    return version


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tag")
    args = parser.parse_args()
    print(f"Release metadata ready: {check(Path(__file__).resolve().parent.parent, args.tag)}")
