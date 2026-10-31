"""Create a deterministic Markdown fixture; never overwrite an existing file."""
import argparse
import hashlib
import json
from pathlib import Path

CHARACTERS = 5_000_000
MARKERS = (
    "SLATE_BASELINE_NEEDLE_START",
    "SLATE_BASELINE_NEEDLE_MIDDLE",
    "SLATE_BASELINE_NEEDLE_END",
)


def section(number: int) -> str:
    lines = [f"## Section {number:06d}", ""]
    paragraph = (
        "Slate baseline: escribir, buscar y desplazarse sin perder texto. "
        "Texto con acentos: canción, árbol, pingüino; Unicode: 雪, λ, →. "
        "This paragraph mixes **bold**, *italic*, `inline_code` and ordinary words."
    )
    for row in range(32):
        lines.append(f"Paragraph {number:06d}.{row:02d}: {paragraph}" + (" " + paragraph if row % 4 == 0 else ""))
        if row % 8 == 0:
            lines.append("")
    lines.extend([
        "", "- Plain list item, deliberately not a Flow task.",
        "> A quoted paragraph for Markdown layout and wrapping.", "",
        "```python", "def baseline_example(value):", "    return value + 1  # sample text only; not executed", "```", "",
        "| Column | Value |", "| --- | --- |", f"| section | {number:06d} |", "",
    ])
    if number % 25 == 0:
        lines.extend(["Long wrapped line: " + ("wrapping αβγ café — keep the cursor visible. " * 180), ""])
    return "\n".join(lines) + "\n"


def segment(length: int, first: int) -> str:
    parts = []
    size = 0
    number = first
    while True:
        block = section(number)
        if size + len(block) > length:
            break
        parts.append(block)
        size += len(block)
        number += 1
    # Never cut a fenced code block; the final padding is ordinary paragraph text.
    padding = "Baseline padding: ordinary text, spaces and accented café.\n"
    left = length - size
    parts.append((padding * (left // len(padding) + 1))[:left])
    return "".join(parts)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    path = args.output
    metadata_path = path.with_suffix(".metadata.json")
    if path.exists() or metadata_path.exists():
        parser.error("Fixture or metadata already exists; choose a new path. No files overwritten.")
    prefix = (
        "# Slate large-document baseline\n\n"
        "Generated test document: exactly 5,000,000 Unicode characters.\n"
        "Use Save As for edits; keep this original unchanged for repeatable comparisons.\n"
        "No Flow checkboxes, external URLs or executable actions.\n\n"
        + MARKERS[0] + "\n\n"
    )
    middle = "\n" + MARKERS[1] + "\n\n"
    suffix = "\n" + MARKERS[2] + "\n"
    first_length = CHARACTERS // 2 - len(prefix)
    second_length = CHARACTERS - CHARACTERS // 2 - len(middle) - len(suffix)
    text = prefix + segment(first_length, 1) + middle + segment(second_length, 100_001) + suffix
    assert len(text) == CHARACTERS
    assert text.count("```") % 2 == 0
    assert all(text.count(marker) == 1 for marker in MARKERS)
    data = text.encode("utf-8")
    positions = {}
    for marker in MARKERS:
        offset = text.index(marker)
        positions[marker] = {
            "character_offset_zero_based": offset,
            "line_one_based": text.count("\n", 0, offset) + 1,
        }
    metadata = {
        "schema_version": 1,
        "generator": "tools/generate_large_document_baseline.py",
        "characters_unicode_codepoints": len(text),
        "bytes_utf8": len(data),
        "lines": text.count("\n"),
        "longest_line_characters": max(map(len, text.splitlines())),
        "sha256": hashlib.sha256(data).hexdigest(),
        "encoding": "UTF-8 without BOM; LF newlines; trailing newline",
        "search_markers": positions,
    }
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("xb") as file:
        file.write(data)
    with metadata_path.open("x", encoding="utf-8", newline="\n") as file:
        json.dump(metadata, file, ensure_ascii=False, indent=2)
        file.write("\n")
    print(path)
    print(json.dumps(metadata, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
