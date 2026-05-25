# Slate

Slate is a small, fast, local-first text editor for writing, editing, and eventually organizing plain Markdown notes.

It is not trying to be Vim, Emacs, Obsidian, Logseq, or a full IDE. Slate borrows the parts that feel good — command palettes, minibuffers, transient key layers, plain text, fast navigation — and leaves the rest behind.

The goal is simple:

> A normal text editor first. A powerful personal knowledge workspace when you want it.

---

## What Slate does today

Slate currently provides a minimal native GUI editor with a terminal-inspired interface:

- plain text / Markdown editing
- native editor buffer and renderer, not `egui::TextEdit` for the main document
- save, open, new, quit
- simple Markdown preview
- command palette
- command line / minibuffer
- persistent settings
- absolute and relative line numbers
- goto line/column commands
- native find with highlights and match navigation
- richer statusbar with file state, active mode, cursor position, line/word/char counts
- Ctrl-hold command layer for short editing commands
- configurable Ctrl+Shift live movement using either Vim-style `h/j/k/l` or Slate-style `i/j/k/l`
- shortcut help inside the minibuffer

Slate keeps the UI intentionally sparse: no menubar, no toolbar, no permanent sidebars.

---

## Why Slate exists

Most writing tools eventually become either:

1. too modal,
2. too mouse-heavy,
3. too database-heavy,
4. too configurable,
5. or too visually busy.

Slate is an experiment in a different direction:

- **normal typing is always the default**
- commands are discoverable but not intrusive
- Markdown files remain the source of truth
- power features should feel temporary, not like switching personalities
- organization should be progressive: capture first, structure later
- the interface should feel closer to a terminal editor than a productivity dashboard

Slate wants the useful ideas from Vim and Emacs without requiring you to live inside Vim or Emacs.

---

## How Slate works

Slate is written in Rust using `eframe`/`egui`, but the main editor is Slate-owned:

- `EditorBuffer` owns the text, cursor, selection, line index, and edit primitives.
- `EditorView` paints the document, cursor, selections, line numbers, wrapping, and highlights.
- Search, goto, selection, and command actions operate on Slate's own editor primitives.
- `egui::TextEdit` is kept only for small inputs such as the command line and settings fields.

This matters because Slate can reliably implement things like:

- find highlights
- scroll-to-cursor
- line/column jumps
- custom selections
- future wiki-link navigation
- result buffers
- editor commands

without fighting a black-box text widget.

---

## Run

From source:

```bash
cargo run --release
cargo run --release -- ~/notes.md
```

Or after local install:

```bash
slate
slate ~/notes.md
```

Scratch mode:

```bash
slate --scratch
```

---

## Core shortcuts

| Shortcut | Action |
| --- | --- |
| `Ctrl+P` | Command palette |
| `Ctrl+H` | Shortcut help |
| `Ctrl+S` | Save |
| `Ctrl+O` | Open file |
| `Ctrl+N` | New buffer |
| `Ctrl+M` | Toggle Markdown preview |
| `Ctrl+F` | Find |
| `Ctrl+.` | Command line |
| `Ctrl+Q` | Quit |

Find mode:

| Key | Action |
| --- | --- |
| `f` | Next match |
| `b` | Previous match |
| `Enter` | Accept match |
| `Esc` | Cancel find |
| `Ctrl+F` | Place cursor after current match |
| `Ctrl+B` | Place cursor before current match |

---

## Command line

Open the command line with:

```text
Ctrl+.
```

Examples:

```text
:w
:q
:wq
:x
:open ~/notes/todo.md
:e ~/notes/todo.md
:new
:preview
:wrap
:settings
```

Goto:

```text
:g 10       # absolute line 10
:g 10:4     # line 10, column 4
:g +5       # five lines down
:g -5       # five lines up
```

Editing/navigation commands:

```text
:select-word    :sw
:select-line    :sl
:delete-word    :dw
:delete-line    :dl
:top            :gt
:bottom         :gb
:open-last      :ol
```

---

## Ctrl-hold command layer

Slate has an experimental transient command layer:

1. hold `Ctrl`
2. type a short sequence
3. release `Ctrl`

Examples:

| Sequence | Action |
| --- | --- |
| `Ctrl` hold → `s` → release | Save |
| `Ctrl` hold → `o` → release | Open |
| `Ctrl` hold → `o l` → release | Open last file |
| `Ctrl` hold → `d l` → release | Delete line |
| `Ctrl` hold → `d w` → release | Delete word |
| `Ctrl` hold → `s w` → release | Select word |
| `Ctrl` hold → `s l` → release | Select line |
| `Ctrl` hold → `g t` → release | Go to top |
| `Ctrl` hold → `g b` → release | Go to bottom |
| `Ctrl` hold → `h` → release | Shortcut help |

This gives Slate some of the speed of modal editors without requiring a permanent mode switch.

---

## Ctrl+Shift live movement

For movement that should happen immediately, Slate supports a live movement layer while holding `Ctrl+Shift`.

Two modes are available in Settings:

### Vim movement

```text
h left
j down
k up
l right
```

### Slate movement

```text
i up
j left
k down
l right
```

The selected mode is persisted in:

```text
~/.config/slate/config.toml
```

---

## Settings

Open settings with:

```text
:settings
```

Current settings include:

- command history length
- line number mode: absolute / relative
- Ctrl+Shift movement mode: Vim / Slate

Config lives at:

```text
~/.config/slate/config.toml
```

Current config is intentionally small and human-readable.

---

## Design principles

Slate should remain:

- local-first
- Markdown-first
- keyboard-friendly
- command-oriented
- visually minimal
- normal-editor-first
- optional-knowledge-workspace-later

Slate should avoid, especially early:

- mandatory databases
- heavy sidebars
- graph-view-first UX
- plugin complexity
- infinite settings
- cloning Vim/Emacs/Obsidian wholesale

---

## Roadmap

Slate's roadmap is intentionally staged. The idea is to build stable editor foundations first, then layer knowledge workflows on top.

### Completed foundation

- Native editor architecture
- Native text buffer
- Native renderer
- Native input handling
- Selection, cursor jumps, and scroll-to-position
- Native find/highlight flow
- Statusbar + minibuffer layout
- Command line basics
- Persistent config basics
- Ctrl-hold command layer
- Ctrl+Shift live movement modes

### Near-term

- Finish find regression/performance harness
- Duplicate line and move line up/down
- `Alt+Up` / `Alt+Down` for moving lines
- Better command discovery and fuzzy command matching
- Real recent-files list and `:recent`
- Persist more editor preferences such as wrap and preview mode
- Minimal project/vault file opener

### Knowledge-work layer

- Scratch buffer and quick capture inbox
- Daily notes
- Optional vault/root notes folder
- Global notes search
- Wiki links like `[[Note]]`
- Link resolver triggered by `[[`
- Follow-link navigation
- Deep links to exact file/line/column
- Backlinks and most-linked notes
- Tags and lightweight metadata
- Task/checklist commands

### Later experiments

- Rebuildable SQLite/FTS index for vault search and links
- Textual result buffers for search, backlinks, tasks, and diagnostics
- Templates
- Theme system
- Optional auto-save
- Repeatable edits / lightweight macros
- Literate Markdown config

---

## Current vibe

Slate should feel like opening a quiet terminal and writing.

No dashboard. No ceremony. No project management cosplay.

Just text, commands, and enough structure to make thinking easier.
