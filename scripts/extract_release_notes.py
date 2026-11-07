#!/usr/bin/env python3
"""Extract only the release-note section matching a Git tag."""
import argparse
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parent.parent
TAG_PATTERN = re.compile(r"v\\d+\\.\\d+\\.\\d+(?:-[0-9A-Za-z.-]+)?")
HEADING_PATTERN = re.compile(r"^# Termune (v\\d+\\.\\d+\\.\\d+(?:-[0-9A-Za-z.-]+)?)$")


def extract_release_notes(tag: str, source: Path) -> str:
    if not TAG_PATTERN.fullmatch(tag):
        raise ValueError(f"Invalid release tag: {tag}")

    lines = source.read_text(encoding="utf-8").splitlines()
    matches = [
        index for index, line in enumerate(lines)
        if (match := HEADING_PATTERN.fullmatch(line.strip())) and match.group(1) == tag
    ]
    if len(matches) != 1:
        raise ValueError(
            f"Expected exactly one '# Termune {tag}' section in {source}, found {len(matches)}"
        )

    start = matches[0]
    end = len(lines)
    for index in range(start + 1, len(lines)):
        if HEADING_PATTERN.fullmatch(lines[index].strip()):
            end = index
            break

    section = lines[start:end]
    while section and (not section[-1].strip() or section[-1].strip() == "---"):
        section.pop()

    if len(section) < 2 or not any(line.strip() for line in section[1:]):
        raise ValueError(f"Release notes section for {tag} is empty")

    return "\\n".join(section).strip() + "\\n"


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tag", required=True, help="Release tag, for example v0.1.9")
    parser.add_argument(
        "--source", type=Path, default=ROOT / "RELEASE_NOTES.md",
        help="Markdown changelog to extract from",
    )
    parser.add_argument("--output", type=Path, help="Output file; stdout when omitted")
    args = parser.parse_args()

    try:
        result = extract_release_notes(args.tag, args.source)
    except (OSError, ValueError) as error:
        parser.error(str(error))

    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(result, encoding="utf-8")
    else:
        print(result, end="")


if __name__ == "__main__":
    main()
