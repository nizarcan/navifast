#!/usr/bin/env python3
"""Disposable real-server QA: start, test, or stop. Requires Docker and Cargo."""

import argparse
import hashlib
import http.client
import json
import math
import os
from pathlib import Path
import secrets
import shutil
import struct
import subprocess
import time
import urllib.error
import urllib.parse
import urllib.request
import wave

REPO = Path(__file__).resolve().parent.parent
ROOT = REPO / ".cache" / "navidrome-fixture"
NAME = "navifast-qa-" + hashlib.sha256(str(REPO).encode()).hexdigest()[:10]
# Navidrome 0.64.2. Pin the fixture so server changes cannot silently change QA.
IMAGE = "deluan/navidrome@sha256:38dc2727bfcfd5ede290f8ada114fc90368146f265ae4701ddddbcbe2a44ee52"


def docker(*args):
    return subprocess.check_output(["docker", *args], text=True).strip()


def start():
    account = ROOT / "account.json"
    if account.exists():
        config = json.loads(account.read_text())
        try:
            with urllib.request.urlopen(config["url"] + "/ping", timeout=2) as response:
                response.read()
        except (urllib.error.URLError, TimeoutError, http.client.RemoteDisconnected):
            raise SystemExit("The saved fixture is unavailable. Run stop, then start again.")
        return config
    if (
        subprocess.run(
            ["docker", "inspect", NAME],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        ).returncode
        == 0
    ):
        raise SystemExit(
            "The fixture container already exists without its account file. Run stop first."
        )
    (ROOT / "music").mkdir(parents=True, exist_ok=True)
    # A generated 40-second stereo tone, with no music or user library involved.
    second = b"".join(
        struct.pack("<hh", value, value)
        for i in range(44100)
        for value in [int(4000 * math.sin(i * 440 * math.tau / 44100))]
    )
    with wave.open(str(ROOT / "music" / "Synthetic QA.wav"), "wb") as output:
        output.setparams((2, 2, 44100, 0, "NONE", "not compressed"))
        for _ in range(40):
            output.writeframes(second)
    docker(
        "create",
        "--rm",
        "--read-only",
        "--name",
        NAME,
        "--cpus=1",
        "--memory=512m",
        "--user",
        f"{os.getuid()}:{os.getgid()}",
        "-p",
        "127.0.0.1::4533",
        "-e",
        "ND_ENABLEINSIGHTSCOLLECTOR=false",
        "-e",
        "ND_ENABLEGRAVATAR=false",
        "-e",
        "ND_ENABLEEXTERNALSERVICES=false",
        "--tmpfs",
        f"/data:rw,uid={os.getuid()},gid={os.getgid()},size=67108864",
        "--tmpfs",
        "/tmp:rw,size=67108864",
        IMAGE,
    )
    docker("cp", str(ROOT / "music") + "/.", NAME + ":/music")
    docker("start", NAME)
    url = "http://" + docker("port", NAME, "4533/tcp").splitlines()[0]
    credentials = {"username": "fixture", "password": secrets.token_urlsafe(24)}
    deadline = time.monotonic() + 30
    while True:
        try:
            with urllib.request.urlopen(url + "/ping", timeout=1) as response:
                response.read()
            break
        except (urllib.error.URLError, TimeoutError, http.client.RemoteDisconnected):
            if time.monotonic() >= deadline:
                raise SystemExit("Fixture startup timed out. Run stop to remove it.")
            time.sleep(0.2)
    request = urllib.request.Request(
        url + "/auth/createAdmin",
        data=json.dumps(credentials).encode(),
        headers={"Content-Type": "application/json"},
    )
    with urllib.request.urlopen(request, timeout=10) as response:
        response.read()  # Never print the authorization response.
    config = {"url": url, **credentials}
    account.write_text(json.dumps(config))
    account.chmod(0o600)
    # Startup scanning is automatic; wait for the one synthetic song to appear.
    salt = secrets.token_hex(8)
    query = urllib.parse.urlencode(
        {
            "u": credentials["username"],
            "t": hashlib.md5((credentials["password"] + salt).encode()).hexdigest(),
            "s": salt,
            "v": "1.16.1",
            "c": "navifast-fixture",
            "f": "json",
            "query": "",
            "songCount": 1,
            "albumCount": 0,
            "artistCount": 0,
        }
    )
    while time.monotonic() < deadline:
        with urllib.request.urlopen(
            url + "/rest/search3?" + query, timeout=5
        ) as response:
            found = (
                json.load(response)["subsonic-response"]
                .get("searchResult3", {})
                .get("song", [])
            )
        if found:
            return config
        time.sleep(0.2)
    raise SystemExit("Fixture scan timed out. Run stop to remove it.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=["start", "test", "stop"])
    args = parser.parse_args()
    if args.action == "stop":
        subprocess.run(
            ["docker", "rm", "--force", NAME],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            check=False,
        )
        if ROOT.exists():
            shutil.rmtree(ROOT)
        print("Removed the disposable Navidrome fixture.")
        return
    config = start()
    print("Isolated Navidrome 0.64.2 fixture ready at " + config["url"], flush=True)
    if args.action == "test":
        env = os.environ | {
            "NAVIFAST_TEST_SERVER": config["url"],
            "NAVIFAST_TEST_USER": config["username"],
            "NAVIFAST_TEST_PASSWORD": config["password"],
        }
        raise SystemExit(
            subprocess.run(
                ["cargo", "test", "--lib", "local_navidrome", "--", "--ignored"],
                cwd=REPO,
                env=env,
            ).returncode
        )


if __name__ == "__main__":
    main()
