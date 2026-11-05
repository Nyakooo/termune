"""Exercise installer downloads with fixture binaries, never real installations."""
import hashlib
import os
from pathlib import Path
import subprocess
import sys
import tempfile

SCRIPT = Path(__file__).resolve().parents[1] / "install.sh"
REPO = "https://github.com/Nyakooo/termune"
BINARY = b"#!/bin/sh\nprintf 'installer fixture only\\n'\n"

with tempfile.TemporaryDirectory() as root:
    root = Path(root)
    fixtures = root / "fixtures"
    mocks = root / "mocks"
    fixtures.mkdir()
    mocks.mkdir()
    checksums = []
    for target in ["x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu", "x86_64-apple-darwin", "aarch64-apple-darwin"]:
        binary = fixtures / f"termune-{target}"
        binary.write_bytes(BINARY)
        checksums.append(f"{hashlib.sha256(BINARY).hexdigest()}  {binary.name}\n")
    checksum_text = "".join(checksums)
    (fixtures / "SHA256SUMS").write_text(checksum_text)
    (mocks / "curl").write_text(f"#!{sys.executable}\n" + '''import os,sys,shutil
from pathlib import Path
args=sys.argv[1:]
if '-w' in args:
 if os.environ.get('MOCK_NO_RELEASE')=='1': sys.exit(22)
 print('https://github.com/Nyakooo/termune/releases/tag/v0.1.0',end='')
else:
 url=next(a for a in args if a.startswith('https://'))
 name=url.rsplit('/',1)[1]
 if not url.startswith('https://github.com/Nyakooo/termune/releases/download/v0.1.0/'): sys.exit(22)
 source=Path(os.environ['MOCK_FIXTURES'])/name
 if not source.is_file(): sys.exit(22)
 shutil.copyfile(source,args[args.index('-o')+1])
''')
    (mocks / "uname").write_text('#!/bin/sh\nif [ "$1" = -s ]; then printf "%s\\n" "$MOCK_OS"; else printf "%s\\n" "$MOCK_ARCH"; fi\n')
    (mocks / "sysctl").write_text('#!/bin/sh\nprintf "%s\\n" "${MOCK_APPLE_ARM:-0}"\n')
    for file in mocks.iterdir():
        file.chmod(0o755)
    destination = root / "install directory with spaces"
    env = dict(os.environ, PATH=f"{mocks}:{os.environ['PATH']}",
               TERMUNE_INSTALL_DIR=str(destination), TERMUNE_NO_MODIFY_PATH="1",
               MOCK_FIXTURES=str(fixtures), MOCK_OS="Linux", MOCK_ARCH="x86_64")
    env.pop("TERMUNE_VERSION", None)

    def run(success=True):
        result = subprocess.run(["sh", str(SCRIPT)], env=env, capture_output=True, text=True)
        assert (result.returncode == 0) == success, result.stdout + result.stderr
        assert not list(destination.glob(".termune.*"))
        return result

    for system, architecture in [("Linux", "x86_64"), ("Linux", "aarch64"), ("Darwin", "x86_64"), ("Darwin", "arm64")]:
        env.update(MOCK_OS=system, MOCK_ARCH=architecture)
        run()
        installed = destination / "termune"
        assert installed.read_bytes() == BINARY and os.access(installed, os.X_OK)
    # Rosetta must choose a native Apple Silicon package.
    env.update(MOCK_OS="Darwin", MOCK_ARCH="x86_64", MOCK_APPLE_ARM="1")
    assert "aarch64-apple-darwin" in run().stdout
    env.update(MOCK_OS="Linux", MOCK_ARCH="x86_64", MOCK_APPLE_ARM="0", TERMUNE_VERSION="0.1.0")
    run()
    # Bad downloads must leave the old executable intact.
    installed.write_bytes(b"previous working executable")
    (fixtures / "SHA256SUMS").write_text(checksum_text.replace(checksums[0][:64], "0" * 64))
    assert "SHA256 mismatch" in run(False).stderr
    assert installed.read_bytes() == b"previous working executable"
    (fixtures / "SHA256SUMS").write_text("")
    run(False)
    assert installed.read_bytes() == b"previous working executable"
    (fixtures / "SHA256SUMS").write_text(checksum_text)
    env["TERMUNE_VERSION"] = "../../bad"
    run(False)
    env.pop("TERMUNE_VERSION")
    env["MOCK_NO_RELEASE"] = "1"
    run(False)
    env.pop("MOCK_NO_RELEASE")
    env["MOCK_ARCH"] = "armv7l"
    run(False)
    print("PASS: four Unix targets, Rosetta, latest/pinned versions, paths with spaces, checksum failures, no release, unsupported architecture and safe updates")
