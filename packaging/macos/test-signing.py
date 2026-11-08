"""Test CI signing isolation and cleanup without touching a real Keychain."""
import base64
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
IDENTITY = "A" * 40
TOOLS = r'''
import json
import os
from pathlib import Path
import sys

tool = Path(sys.argv[0]).name
args = sys.argv[1:]
if tool == "uname":
    print(os.environ.get("FIXTURE_OS", "Darwin"))
elif tool == "openssl":
    if args[0] == "x509":
        print("SHA1 Fingerprint=" + os.environ.get("FIXTURE_FINGERPRINT", "A" * 40))
    elif args[0] == "rand":
        print("fixture-keychain-password")
    else:
        sys.exit(1)
elif tool == "security":
    with open(os.environ["SECURITY_LOG"], "a") as log:
        log.write(args[0] + "\n")
    if args[0] == os.environ.get("SECURITY_FAIL"):
        sys.exit(9)
    if args[0] == "create-keychain":
        Path(args[-1]).touch()
    if args[0] == "find-certificate":
        print("-----BEGIN CERTIFICATE-----\nfixture\n-----END CERTIFICATE-----")
elif tool == "signed-command":
    assert "MACOS_SIGNING_CERTIFICATE_P12" not in os.environ
    assert "MACOS_SIGNING_CERTIFICATE_PASSWORD" not in os.environ
    assert Path(os.environ["CODESIGN_KEYCHAIN"]).is_file()
    Path(os.environ["COMMAND_LOG"]).write_text(json.dumps({
        "identity": os.environ["CODESIGN_IDENTITY"],
        "keychain": os.environ["CODESIGN_KEYCHAIN"],
        "args": args,
    }))
    sys.exit(int(os.environ.get("COMMAND_EXIT", "0")))
'''


class SigningTest(unittest.TestCase):
    def setUp(self):
        cache = ROOT / ".cache"
        cache.mkdir(exist_ok=True)
        self.temporary = tempfile.TemporaryDirectory(prefix="mac-signing-test-", dir=cache)
        self.addCleanup(self.temporary.cleanup)
        self.base = Path(self.temporary.name)
        self.tools = self.base / "tools"
        self.tools.mkdir()
        for name in ["uname", "openssl", "security", "signed-command"]:
            command = self.tools / name
            command.write_text(f"#!{sys.executable}\n" + TOOLS)
            command.chmod(0o755)
        self.runner = self.base / "runner temp"
        self.runner.mkdir()
        self.sentinel = self.runner / "unrelated"
        self.sentinel.write_text("preserve")
        self.security_log = self.base / "security.log"
        self.command_log = self.base / "command.json"
        self.env = {
            **os.environ,
            "PATH": str(self.tools) + os.pathsep + os.environ["PATH"],
            "GITHUB_ACTIONS": "true",
            "RUNNER_TEMP": str(self.runner),
            "MACOS_SIGNING_CERTIFICATE_P12": base64.b64encode(b"fixture p12").decode(),
            "MACOS_SIGNING_CERTIFICATE_PASSWORD": "fixture-export-password",
            "MACOS_SIGNING_IDENTITY": IDENTITY,
            "SECURITY_LOG": str(self.security_log),
            "COMMAND_LOG": str(self.command_log),
        }

    def run_signing(self, **env):
        result = subprocess.run([
            "bash", str(ROOT / "packaging/macos/with-signing-keychain.sh"),
            "signed-command", "path with spaces",
        ], env={**self.env, **env}, capture_output=True, text=True)
        self.assertEqual(list(self.runner.iterdir()), [self.sentinel])
        self.assertNotIn("fixture-export-password", result.stdout + result.stderr)
        self.assertNotIn("fixture-keychain-password", result.stdout + result.stderr)
        return result

    def test_signs_with_isolated_keychain_and_removes_trust(self):
        result = self.run_signing()
        self.assertEqual(result.returncode, 0, result.stderr)
        command = json.loads(self.command_log.read_text())
        self.assertEqual(command["identity"], IDENTITY)
        self.assertEqual(command["args"], ["path with spaces"])
        self.assertFalse(Path(command["keychain"]).exists())
        operations = self.security_log.read_text().splitlines()
        self.assertEqual(operations[-2:], ["remove-trusted-cert", "delete-keychain"])

    def test_failure_preserves_exit_status_and_cleans_keychain(self):
        result = self.run_signing(COMMAND_EXIT="23")
        self.assertEqual(result.returncode, 23)
        self.assertTrue(self.command_log.exists())
        self.assertIn("remove-trusted-cert", self.security_log.read_text())

    def test_partial_import_and_trust_failures_are_cleaned(self):
        for failure in ["create-keychain", "import", "add-trusted-cert"]:
            with self.subTest(failure=failure):
                result = self.run_signing(SECURITY_FAIL=failure)
                self.assertEqual(result.returncode, 9)
                self.assertFalse(self.command_log.exists())
                self.assertIn("delete-keychain", self.security_log.read_text())

    def test_missing_secrets_fail_before_running_command(self):
        for name in ["MACOS_SIGNING_CERTIFICATE_P12", "MACOS_SIGNING_CERTIFICATE_PASSWORD", "MACOS_SIGNING_IDENTITY"]:
            with self.subTest(name=name):
                result = self.run_signing(**{name: ""})
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("Missing " + name, result.stderr)
                self.assertFalse(self.command_log.exists())
                self.assertFalse(self.security_log.exists())

    def test_wrong_fingerprint_never_trusts_or_signs(self):
        result = self.run_signing(FIXTURE_FINGERPRINT="B" * 40)
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(self.command_log.exists())
        self.assertNotIn("add-trusted-cert", self.security_log.read_text())

    def test_refuses_local_or_non_macos_execution(self):
        for env in [{"GITHUB_ACTIONS": ""}, {"FIXTURE_OS": "Linux"}, {"RUNNER_TEMP": ""}]:
            with self.subTest(env=env):
                result = self.run_signing(**env)
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse(self.security_log.exists())


if __name__ == "__main__":
    unittest.main()
