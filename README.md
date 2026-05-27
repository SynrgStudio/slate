# Slate

Slate is a small, fast, local-first text editor for writing, editing, and eventually organizing plain Markdown notes.

It is not trying to be Vim, Emacs, Obsidian, Logseq, or a full IDE. Slate borrows the parts that feel good — command palettes, minibuffers, transient key layers, plain text, fast navigation — and leaves the rest behind.

The goal is simple:

> A normal text editor first. A powerful personal knowledge workspace when you want it.

For the project philosophy and feature decision rules, see [`PHILOSOPHY.md`](PHILOSOPHY.md).

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
- modal shortcut and command reference

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

In-app scratch capture:

```text
:scratch
:sc
```

The in-app scratch modal archives with `Ctrl+S`, opens archived entries with `Ctrl+E`, hides with `Esc`, and resumes with `:scratch` if it still has pending text.

Scratch entries review:

```text
:scratch-entries
:scratch-log
:scl
```

Use `↑↓` to select an archived entry and `Ctrl+D` or `Delete` to delete it from `~/.local/share/slate/scratch.md`.

---

## Core shortcuts

| Shortcut | Action |
| --- | --- |
| `Ctrl+P` | Command palette |
| `Ctrl+H` | Modal shortcut and command reference, including Ctrl-layer, Alt layer, Shift+Alt jumps, duplicate placement, and all registered commands |
| `Ctrl+S` | Save |
| `Ctrl+Alt+S` | Save As |
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

The command line is Slate's fastest primary interface. While typing the command token, Slate shows a live terminal-style completion inline in dim text and a small fuzzy suggestion list sourced from the real command registry. Press `Tab` to accept the current completion, or `Enter` to run.

Slate persists command history and recent/frequent command usage in `~/.config/slate/config.toml`. Usage acts as a conservative ranking boost for commandline suggestions and the command palette. Fuzzy match quality still comes first, so this should improve discovery without making the commandline feel noisy or heavy.

Examples:

```text
:w
:q
:wq
:x
:open              # Slate-owned file modal
:open ~/notes/todo.md
:e ~/notes/todo.md
:save-as           # Slate-owned save-as modal
:scratch           # In-app quick capture modal
:scratch-entries   # Review and clean archived scratch entries
:new
:preview
:preview on
:preview off
:wrap
:wrap on
:wrap off
:line-numbers relative
:line-numbers absolute
:settings
:recent
:recent notes
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
:duplicate-line :dup
:duplicate-place :dupp
:move-line-up   :mlu
:move-line-down :mld
:move-line-to-paragraph-start :mlps
:move-line-to-paragraph-end   :mlpe
:top            :gt
:bottom         :gb
:open-last      :ol
:recent         :rec
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
| `Ctrl` hold → `d u p` → release | Duplicate line |
| `Ctrl` hold → `d u p p` → release | Duplicate placement mode |
| `Ctrl` hold → `d w` → release | Delete word |
| `Ctrl` hold → `s w` → release | Select word |
| `Ctrl` hold → `s l` → release | Select line |
| `Ctrl` hold → `g t` → release | Go to top |
| `Ctrl` hold → `g b` → release | Go to bottom |
| `Ctrl` hold → `h` → release | Shortcut help |

This gives Slate some of the speed of modal editors without requiring a permanent mode switch.

---

## Duplicate placement

For a movable duplicate, use:

```text
:duplicate-place
:dupp
```

Slate duplicates the current line and enters a transient placement mode:

```text
Alt movement or Ctrl+Shift movement = move duplicate
Enter / Space                    = place
Esc                              = cancel
```

---

## Alt structural editing

`Alt` is reserved for structural text manipulation. It does not move the cursor directly; `Ctrl+Shift` already covers live cursor movement.

Current batch:

| Shortcut | Slate movement mode | Vim movement mode |
| --- | --- | --- |
| Move current line up | `Alt+i` / `Alt+Up` | `Alt+k` / `Alt+Up` |
| Move current line down | `Alt+k` / `Alt+Down` | `Alt+j` / `Alt+Down` |
| Move line to paragraph start | repeat up key, e.g. `Alt+i i` | repeat up key, e.g. `Alt+k k` |
| Move line to paragraph end | repeat down key, e.g. `Alt+k k` | repeat down key, e.g. `Alt+j j` |
| Extend word selection left | `Alt+j` repeatedly | `Alt+h` repeatedly |
| Extend word selection right | `Alt+l` repeatedly | `Alt+l` repeatedly |

A paragraph is a contiguous block of non-empty lines separated by blank lines.

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

Fast structural cursor jumps use the same movement mode with `Shift+Alt` double-key sequences:

```text
Slate mode: Shift+Alt+i i paragraph start, k k paragraph end, j j word start, l l word end, j l line end, l j line start
Vim mode:   Shift+Alt+k k paragraph start, j j paragraph end, h h word start, l l word end, h l line end, l h line start
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
- Slate-owned Open and Save As modals; no native Linux/Desktop file dialogs
- Open modal includes fuzzy filtering, folder navigation, and size/modified metadata
- Persist more editor preferences such as theme and vault location
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
