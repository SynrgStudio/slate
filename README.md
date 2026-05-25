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
- `:g 10`, `:g 10:4`, `:g +5`, `:g -5` jump to line/column
- `:delete-line` / `:dl` delete current line
- Ctrl-hold layer: hold `Ctrl`, type `d` then `l`, release `Ctrl` to delete line
- Ctrl-hold layer: hold `Ctrl`, type `o` then `l`, release `Ctrl` to open last file
- `:settings` configure history length and absolute/relative line numbers
- `Ctrl+Q` quit

## Current vibe

- no menubar
- no toolbar
- dark blue/gray theme
- command palette driven
- simple text editing
- optional lightweight Markdown preview
