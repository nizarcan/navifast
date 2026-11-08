"""Exercise the local Mac installer with isolated Cargo and platform tools.

Runs on macOS or Linux without building Rust, signing apps, or using Applications.
Native build/signature validation is separate from these failure-path fixtures.
"""
import json
import os
from pathlib import Path
import plistlib
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]

TOOLS = r'''
import json
import os
from pathlib import Path
import shutil
import sys

tool = Path(sys.argv[0]).name
args = sys.argv[1:]
if tool == "uname":
    print(os.environ.get("FIXTURE_OS", "Darwin"))
elif tool == "cargo":
    Path(os.environ["BUILD_LOG"]).write_text(json.dumps(args))
    print(json.dumps({"reason": "compiler-artifact", "target": {
        "name": "navifast", "kind": ["bin"]},
        "executable": os.environ["ARTIFACT"]}))
    sys.exit(int(os.environ.get("BUILD_FAIL", "0")))
elif tool == "sips":
    Path(args[args.index("--out") + 1]).write_bytes(b"fixture png")
elif tool == "iconutil":
    Path(args[args.index("-o") + 1]).write_bytes(b"fixture icon")
elif tool == "codesign":
    if "--sign" in args:
        Path(os.environ["SIGN_LOG"]).write_text(json.dumps(args))
    if "--sign" in args and os.environ.get("SIGN_FAIL"):
        sys.exit(1)
    if "--verify" in args and os.environ.get("VERIFY_FAIL"):
        sys.exit(1)
elif tool == "mv":
    if Path(args[0]).name == "Navifast.app" and os.environ.get("MOVE_FAIL"):
        sys.exit(1)
    shutil.move(*args)
'''


class InstallTest(unittest.TestCase):
    def setUp(self):
        cache = ROOT / ".cache"
        cache.mkdir(exist_ok=True)
        self.temporary = tempfile.TemporaryDirectory(prefix="mac-install-test-", dir=cache)
        self.addCleanup(self.temporary.cleanup)
        self.base = Path(self.temporary.name)
        self.project = self.base / "checkout with spaces"
        packaging = self.project / "packaging/macos"
        packaging.mkdir(parents=True)
        for name in ["install.sh", "bundle.sh", "Info.plist"]:
            shutil.copyfile(ROOT / "packaging/macos" / name, packaging / name)
        (packaging / "icon-1024.png").write_bytes(b"fixture source icon")
        shutil.copyfile(ROOT / "Makefile", self.project / "Makefile")
        self.applications = self.base / "My Applications"
        self.applications.mkdir()
        self.app = self.applications / "Navifast.app"
        self.binary = self.base / "custom target/release/navifast"
        self.binary.parent.mkdir(parents=True)
        self.binary.write_text('#!/bin/bash\necho "navifast 1.2.3-rc.4"\n')
        self.binary.chmod(0o755)
        self.log = self.base / "cargo-args.json"
        self.sign_log = self.base / "sign-args.json"
        tools = self.base / "tools"
        tools.mkdir()
        for name in ["uname", "cargo", "sips", "iconutil", "codesign", "mv"]:
            command = tools / name
            command.write_text(f"#!{sys.executable}\n" + TOOLS)
            command.chmod(0o755)
        self.env = {
            **os.environ,
            "PATH": str(tools) + os.pathsep + os.environ["PATH"],
            "ARTIFACT": str(self.binary),
            "BUILD_LOG": str(self.log),
            "SIGN_LOG": str(self.sign_log),
        }
        # Do not use a developer's signing identity in a fixture.
        self.env.pop("CODESIGN_IDENTITY", None)
        self.env.pop("CODESIGN_KEYCHAIN", None)
        self.env.pop("CI", None)

    def local_identity(self, value="A" * 40):
        directory = self.project / ".release-secrets"
        directory.mkdir(exist_ok=True)
        (directory / "macos-signing-identity").write_text(value + "\n")

    def signed_with(self):
        args = json.loads(self.sign_log.read_text())
        return args[args.index("--sign") + 1]

    def old_app(self):
        self.app.mkdir()
        (self.app / "old-build").write_text("keep this until installation succeeds")

    def install(self, **env):
        return subprocess.run(
            ["bash", str(self.project / "packaging/macos/install.sh"), str(self.applications)],
            cwd=self.base, env={**self.env, **env}, capture_output=True, text=True,
        )

    def assert_clean(self):
        self.assertEqual(list(self.applications.glob(".navifast-install.*")), [])
        self.assertEqual(list((self.project / ".cache").glob("macos-install.*")), [])

    def check_installed(self):
        self.assertFalse((self.app / "old-build").exists())
        self.assertEqual((self.app / "Contents/MacOS/Navifast").read_bytes(), self.binary.read_bytes())
        with (self.app / "Contents/Info.plist").open("rb") as file:
            info = plistlib.load(file)
        self.assertEqual(info["CFBundleIdentifier"], "io.github.nizarcan.Navifast")
        self.assertEqual(info["CFBundleShortVersionString"], "1.2.3-rc.4")
        self.assertEqual(info["CFBundleVersion"], "1.2.3")
        self.assertEqual(json.loads(self.log.read_text()), [
            "build", "--locked", "--release", "--bin", "navifast",
            "--message-format=json-render-diagnostics",
        ])
        self.assert_clean()

    def test_make_install_uses_cargo_artifact_and_custom_applications_directory(self):
        result = subprocess.run(
            ["make", "install", f"APPLICATIONS_DIR={self.applications}"],
            cwd=self.project, env=self.env, capture_output=True, text=True,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.check_installed()

    def test_successful_upgrade_replaces_the_bundle(self):
        self.old_app()
        result = self.install()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.check_installed()

    def test_remembered_local_identity_signs_every_install(self):
        self.local_identity()
        for _ in range(2):
            result = self.install()
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(self.signed_with(), "A" * 40)
            self.check_installed()

    def test_explicit_identity_overrides_local_choice(self):
        self.local_identity()
        for identity, expected in [("B" * 40, "B" * 40), ("-", "-"), ("", "-")]:
            with self.subTest(identity=identity):
                result = self.install(CODESIGN_IDENTITY=identity)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(self.signed_with(), expected)

    def test_ci_ignores_local_configuration(self):
        self.local_identity("invalid local configuration")
        result = self.install(CI="true")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.signed_with(), "-")

    def test_invalid_local_identity_preserves_existing_app(self):
        self.old_app()
        self.local_identity("invalid local configuration")
        result = self.install()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("fingerprint", result.stderr)
        self.assertTrue((self.app / "old-build").is_file())
        self.assertFalse(self.sign_log.exists())
        self.assert_clean()

    def test_explicit_keychain_is_passed_to_codesign(self):
        result = self.install(CODESIGN_IDENTITY="A" * 40, CODESIGN_KEYCHAIN="/fixture keychain")
        self.assertEqual(result.returncode, 0, result.stderr)
        args = json.loads(self.sign_log.read_text())
        self.assertEqual(args[args.index("--keychain") + 1], "/fixture keychain")

    def test_failed_certificate_signing_never_falls_back_to_ad_hoc(self):
        self.old_app()
        self.local_identity()
        result = self.install(SIGN_FAIL="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.signed_with(), "A" * 40)
        self.assertTrue((self.app / "old-build").is_file())
        self.assert_clean()

    def test_build_signing_and_verification_failures_preserve_the_installed_app(self):
        self.old_app()
        for failure in ["BUILD_FAIL", "SIGN_FAIL", "VERIFY_FAIL", "MOVE_FAIL"]:
            with self.subTest(failure=failure):
                result = self.install(**{failure: "1"})
                self.assertNotEqual(result.returncode, 0)
                self.assertTrue((self.app / "old-build").is_file())
                self.assert_clean()

    def test_failed_first_install_leaves_no_partial_app(self):
        for failure in ["BUILD_FAIL", "SIGN_FAIL", "VERIFY_FAIL", "MOVE_FAIL"]:
            with self.subTest(failure=failure):
                result = self.install(**{failure: "1"})
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse(self.app.exists())
                self.assert_clean()

    def test_unusable_or_misidentified_binary_never_replaces_the_app(self):
        self.old_app()
        for body in ['exit 1', 'echo "another-app 1.2.3"']:
            with self.subTest(body=body):
                self.binary.write_text("#!/bin/bash\n" + body + "\n")
                result = self.install()
                self.assertNotEqual(result.returncode, 0)
                self.assertTrue((self.app / "old-build").is_file())
                self.assert_clean()

    def test_symlink_destination_is_not_replaced(self):
        other = self.base / "other-app"
        other.mkdir()
        self.app.symlink_to(other, target_is_directory=True)
        result = self.install()
        self.assertNotEqual(result.returncode, 0)
        self.assertTrue(self.app.is_symlink())
        self.assertFalse(self.log.exists())
        self.assert_clean()

    def test_non_macos_fails_before_building(self):
        result = self.install(FIXTURE_OS="Linux")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("requires macOS", result.stderr)
        self.assertFalse(self.log.exists())
        self.assert_clean()


if __name__ == "__main__":
    unittest.main()
