import hashlib
import importlib.util
from pathlib import Path
import tempfile
import sys
import tomllib
import unittest
import xml.etree.ElementTree as ET

sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location("release_names", Path(__file__).with_name("release-names.py"))
release_names = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release_names)
check_spec = importlib.util.spec_from_file_location("check_release", Path(__file__).with_name("check-release.py"))
check_release = importlib.util.module_from_spec(check_spec)
check_spec.loader.exec_module(check_release)


class ReleaseNamesTest(unittest.TestCase):
    def test_tag_must_match_the_compiled_version(self):
        root = Path(__file__).resolve().parent.parent
        version = tomllib.loads((root / "Cargo.toml").read_text())["package"]["version"]
        self.assertEqual(check_release.check(root, f"v{version}"), version)
        with self.assertRaisesRegex(ValueError, "does not match"):
            check_release.check(root, "v99.0.0")

    def test_current_version_has_matching_release_metadata(self):
        root = Path(__file__).resolve().parent.parent
        version = tomllib.loads((root / "Cargo.toml").read_text())["package"]["version"]
        notes = root / "packaging/release-notes" / f"v{version}.md"
        self.assertTrue(notes.is_file(), f"Write {notes} before tagging the release")
        self.assertTrue(notes.read_text().strip(), "Release notes must not be empty")
        self.assertNotIn("Historical upstream release notes", notes.read_text())
        lock = tomllib.loads((root / "Cargo.lock").read_text())
        self.assertEqual(next(p["version"] for p in lock["package"] if p["name"] == "navifast"), version)
        metainfo = ET.parse(root / "packaging/flatpak/io.github.nizarcan.Navifast.metainfo.xml")
        self.assertEqual(metainfo.find("releases/release").get("version"), version)
        self.assertTrue((root / "packaging/release-notes" / f"v{version}" / "library.png").is_file())

    def test_first_prerelease_only_has_navifast_names_and_exact_checksums(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            artifacts = {"navifast-v0.1.0-rc.1-linux.tar.gz": b"Linux build", "navifast-v0.1.0-rc.1-macos.dmg": b"Mac build"}
            for name, data in artifacts.items():
                (root / name).write_bytes(data)
            release_names.prepare(root, "v0.1.0-rc.1")
            expected = "".join(f"{hashlib.sha256(data).hexdigest()}  {name}\n" for name, data in sorted(artifacts.items()))
            self.assertEqual((root / "checksums.txt").read_text(), expected)
            release_names.prepare(root, "v0.1.0-rc.1")
            self.assertEqual((root / "checksums.txt").read_text(), expected)
            self.assertEqual({p.name for p in root.iterdir()}, set(artifacts) | {"checksums.txt"})

    def test_foreign_names_and_wrong_versions_fail_before_writing_checksums(self):
        for name in ["fastpotify-v0.1.0-rc.1-linux.tar.gz", "spotifast-v0.1.0-rc.1-macos.dmg", "navifast-v0.11.2-linux.tar.gz"]:
            with self.subTest(name=name), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                (root / "navifast-v0.1.0-rc.1-linux.tar.gz").write_bytes(b"new")
                (root / name).write_bytes(b"other")
                with self.assertRaisesRegex(ValueError, "Unexpected"):
                    release_names.prepare(root, "v0.1.0-rc.1")
                self.assertEqual((root / name).read_bytes(), b"other")
                self.assertFalse((root / "checksums.txt").exists())

    def test_symlinks_and_directories_are_rejected(self):
        for kind in ["symlink", "directory"]:
            with self.subTest(kind=kind), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                path = root / "navifast-v0.1.0-rc.1-linux.tar.gz"
                if kind == "symlink":
                    path.symlink_to(root / "missing")
                else:
                    path.mkdir()
                with self.assertRaisesRegex(ValueError, "regular"):
                    release_names.prepare(root, "v0.1.0-rc.1")

    def test_missing_inputs_and_invalid_tags_fail(self):
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaisesRegex(ValueError, "No release"):
                release_names.prepare(Path(directory), "v0.1.0-rc.1")
            with self.assertRaisesRegex(ValueError, "release tag"):
                release_names.prepare(Path(directory), "main")


if __name__ == "__main__":
    unittest.main()
