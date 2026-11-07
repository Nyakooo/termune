import importlib.util
from pathlib import Path
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / "scripts/extract_release_notes.py"
spec = importlib.util.spec_from_file_location("extract_release_notes", SCRIPT)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class ReleaseNotesExtraction(unittest.TestCase):
    def test_extracts_only_requested_version(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "RELEASE_NOTES.md"
            source.write_text(
                "# Termune v0.1.9\n\n- Current feature.\n\n---\n\n"
                "# Termune v0.1.8\n\n- Previous feature.\n",
                encoding="utf-8",
            )
            result = module.extract_release_notes("v0.1.9", source)
            self.assertEqual(result, "# Termune v0.1.9\n\n- Current feature.\n")
            self.assertNotIn("v0.1.8", result)
            self.assertNotIn("Previous feature", result)

    def test_extracts_previous_version_when_requested(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "RELEASE_NOTES.md"
            source.write_text(
                "# Termune v0.1.9\n\n- Current feature.\n\n---\n\n"
                "# Termune v0.1.8\n\n- Previous feature.\n",
                encoding="utf-8",
            )
            result = module.extract_release_notes("v0.1.8", source)
            self.assertIn("- Previous feature.", result)
            self.assertNotIn("Current feature", result)

    def test_missing_or_duplicate_section_fails_closed(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "RELEASE_NOTES.md"
            source.write_text("# Termune v0.1.8\n\n- Only this version.\n", encoding="utf-8")
            with self.assertRaises(ValueError):
                module.extract_release_notes("v0.1.9", source)
            source.write_text(
                "# Termune v0.1.9\n\n- First.\n\n# Termune v0.1.9\n\n- Second.\n",
                encoding="utf-8",
            )
            with self.assertRaises(ValueError):
                module.extract_release_notes("v0.1.9", source)

    def test_invalid_tag_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            source = Path(directory) / "RELEASE_NOTES.md"
            source.write_text("# Termune v0.1.9\n\n- Current feature.\n", encoding="utf-8")
            with self.assertRaises(ValueError):
                module.extract_release_notes("0.1.9", source)


if __name__ == "__main__":
    unittest.main()
