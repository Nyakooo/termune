"""Release contract shared by GitHub Actions and termune update. Python 3.11+."""
import argparse
import hashlib
import os
from pathlib import Path
import re
import shutil
import tarfile
import tomllib

ROOT = Path(__file__).resolve().parent.parent
TARGETS = (
    "x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu",
    "x86_64-apple-darwin", "aarch64-apple-darwin",
)


def version(root=ROOT, ref=None):
    package = tomllib.loads((root / "Cargo.toml").read_text())["package"]
    value = package["version"]
    if not re.fullmatch(r"\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?", value):
        raise ValueError("Cargo package version must be a release SemVer")
    lock = tomllib.loads((root / "Cargo.lock").read_text())
    entries = [p for p in lock["package"] if p["name"] == "termune"]
    if len(entries) != 1 or entries[0]["version"] != value:
        raise ValueError("Cargo.toml and Cargo.lock versions differ")
    ref = os.environ.get("GITHUB_REF", "") if ref is None else ref
    if ref.startswith("refs/tags/") and ref != f"refs/tags/v{value}":
        raise ValueError("Release tag must match the Cargo version")
    return value


def names(value):
    return {name for target in TARGETS for name in (
        f"termune-{target}", f"termune-{target}.tar.gz")}


def package(target, root=ROOT):
    if target not in TARGETS:
        raise ValueError("Unsupported release target")
    value = version(root)
    executable = root / "target" / target / "release" / "termune"
    if not executable.is_file():
        raise ValueError(f"Missing executable: {executable}")
    dist = root / "dist"
    dist.mkdir(exist_ok=True)
    binary = dist / f"termune-{target}"
    shutil.copyfile(executable, binary)
    binary.chmod(0o755)
    prefix = f"termune-v{value}-{target}"
    with tarfile.open(dist / f"termune-{target}.tar.gz", "w:gz") as archive:
        archive.add(binary, arcname=f"{prefix}/termune")
        for name in ("README.md", "README.en.md", "RELEASE_NOTES.md"):
            archive.add(root / name, arcname=f"{prefix}/{name}")
        archive.add(root / "docs", arcname=f"{prefix}/docs", filter=lambda item:
                    None if "termune-hero" in item.name else item)
    print(f"Packaged {target} v{value}")


def checksums(root=ROOT):
    expected = names(version(root))
    dist = root / "dist"
    actual = {p.name for p in dist.iterdir() if p.is_file() and p.name != "SHA256SUMS"}
    if actual != expected:
        raise ValueError(f"Incomplete or unexpected release assets: missing={expected-actual}, extra={actual-expected}")
    lines = []
    for name in sorted(expected):
        path = dist / name
        if path.stat().st_size == 0:
            raise ValueError(f"Empty asset: {name}")
        with path.open("rb") as stream:
            digest = hashlib.file_digest(stream, "sha256").hexdigest()
        lines.append(f"{digest}  {name}\n")
    (dist / "SHA256SUMS").write_text("".join(lines))
    print("Verified all four platforms; wrote SHA256SUMS")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--check", action="store_true")
    group.add_argument("--checksums", action="store_true")
    group.add_argument("--target", choices=TARGETS)
    args = parser.parse_args()
    if args.check:
        print(f"Version v{version()} verified")
    elif args.checksums:
        checksums()
    else:
        package(args.target)
