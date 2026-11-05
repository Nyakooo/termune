import importlib.util
import tempfile
from pathlib import Path
import unittest
import os
from unittest.mock import patch
import tarfile
import zipfile

spec = importlib.util.spec_from_file_location("package", Path(__file__).resolve().parents[1] / "scripts/package_release.py")
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class ReleaseContract(unittest.TestCase):
    def setUp(self):
        environment = patch.dict(os.environ, {"GITHUB_REF": ""})
        environment.start()
        self.addCleanup(environment.stop)

    def fixture(self, directory):
        root = Path(directory)
        (root / "Cargo.toml").write_text('[package]\nname="termune"\nversion="0.1.0"\n')
        (root / "Cargo.lock").write_text('[[package]]\nname="termune"\nversion="0.1.0"\n')
        return root

    def test_tag_and_lock_must_match_cargo(self):
        with tempfile.TemporaryDirectory() as d:
            root = self.fixture(d)
            self.assertEqual(module.version(root, "refs/tags/v0.1.0"), "0.1.0")
            with self.assertRaises(ValueError):
                module.version(root, "refs/tags/v0.2.0")
            (root / "Cargo.lock").write_text('[[package]]\nname="termune"\nversion="0.0.1"\n')
            with self.assertRaises(ValueError):
                module.version(root, "")

    def test_unix_and_windows_packages_have_matching_executable_names(self):
        with tempfile.TemporaryDirectory() as d:
            root = self.fixture(d)
            (root / "docs").mkdir()
            for name in ("README.md", "RELEASE_NOTES.md", "install.sh", "install.ps1"):
                (root / name).write_text("fixture")
            for target in module.TARGETS:
                filename = "termune.exe" if "windows" in target else "termune"
                directory = root / "target" / target / "release"
                directory.mkdir(parents=True)
                (directory / filename).write_bytes(b"executable fixture")
                module.package(target, root)
                binary, archive = module.asset_names(target)
                self.assertEqual((root / "dist" / binary).read_bytes(), b"executable fixture")
                member = f"termune-v0.1.0-{target}/{filename}"
                if "windows" in target:
                    with zipfile.ZipFile(root / "dist" / archive) as package:
                        self.assertEqual(package.read(member), b"executable fixture")
                        self.assertFalse(any("README.en" in n or n.endswith(".png") for n in package.namelist()))
                else:
                    with tarfile.open(root / "dist" / archive) as package:
                        self.assertEqual(package.extractfile(member).read(), b"executable fixture")
                        self.assertTrue(package.getmember(member).mode & 0o111)

    def test_incomplete_release_cannot_be_published(self):
        with tempfile.TemporaryDirectory() as d:
            root = self.fixture(d)
            dist = root / "dist"
            dist.mkdir()
            with self.assertRaises(ValueError):
                module.checksums(root)
            for name in module.names("0.1.0"):
                (dist / name).write_bytes(b"fixture")
            module.checksums(root)
            self.assertEqual(len((dist / "SHA256SUMS").read_text().splitlines()), 12)
            (dist / "unexpected").touch()
            with self.assertRaises(ValueError):
                module.checksums(root)
