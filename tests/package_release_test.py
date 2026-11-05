import importlib.util
import tempfile
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("package", Path(__file__).resolve().parents[1] / "scripts/package_release.py")
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class ReleaseContract(unittest.TestCase):
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
            self.assertEqual(len((dist / "SHA256SUMS").read_text().splitlines()), 8)
            (dist / "unexpected").touch()
            with self.assertRaises(ValueError):
                module.checksums(root)
