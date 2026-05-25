# Slate

Minimal GUI notepad inspired by a clean terminal look.

## Run

```bash
cargo run --release
cargo run --release -- ~/notes.md
```

Or after local install:

```bash
slate
slate ~/notes.md
```

## Shortcuts

- `Ctrl+P` command palette
- `Ctrl+S` save
- `Ctrl+O` open
- `Ctrl+N` new buffer
- `Ctrl+M` toggle simple Markdown preview
- `Ctrl+F` find (`f`/`b` next/previous, `Ctrl+F`/`Ctrl+B` place cursor after/before match)
- `Ctrl+.` command line
- `Ctrl+Q` quit

## Current vibe

- no menubar
- no toolbar
- dark blue/gray theme
- command palette driven
- simple text editing
- optional lightweight Markdown preview
