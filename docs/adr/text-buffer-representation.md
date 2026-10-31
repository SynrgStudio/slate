# ADR: keep `String` + line index as the text representation (for now)

Date: 2026-10-31 · Status: accepted

## Context

Editors like VS Code and Fred use a piece tree (or a rope) so edits never move
the rest of the document. Slate's `EditorBuffer` keeps one contiguous `String`
plus a `line_starts` index. Every caller (view, Markdown, Flow, search, save)
reads the text as `&str`; a piece tree would change all of them.

The real cost driver was not the `String`: undo copied the whole document on
every edit. That was replaced by delta undo (see `docs/speed-work.md`).

## Measurements after delta undo (release, 5 MB / 27,343 lines)

| operation incl. frame | CPU |
| --- | ---: |
| insert at start / middle / end | 0.8–1.4 ms |
| undo | 0.5–1.1 ms |
| paste multi-line in middle | 1.0 ms |
| scroll frame | 0.09 ms |

A `memmove` of 5 MB is well under a millisecond; at 240 Hz the frame budget is
~4 ms, so the contiguous buffer is not the bottleneck at this size.

## Decision

Keep `String` + incremental line index. Revisit a rope/piece tree only if the
overlay (`F12`) or the large-document benchmark shows single edits above ~4 ms,
e.g. for documents well beyond 5 MB, or if multi-cursor editing makes many
scattered edits per keystroke common.

## Consequences

- Everything keeps borrowing `&str`; no API churn.
- All edits go through `EditorBuffer::edit`, so a future representation change
  has a single mutation path to replace.
