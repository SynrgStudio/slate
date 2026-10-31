# Slate

Slate is a small, fast, local-first text editor and Markdown workspace.

It starts as a normal editor: open a file, type, save, quit. Then, when you want more, Slate gives you a terminal-inspired command line, fuzzy pickers, scratch capture, Markdown affordances, project search, marked files, daily logs, agenda views, backlinks, and local-first knowledge workflows — without turning your notes into a database or surrounding your text with dashboards.

> A quiet terminal-like editor for writing, thinking, capturing, and navigating plain text.

Slate is **not** trying to be Vim, Emacs, Obsidian, Logseq, VS Code, or a full IDE. It borrows the good parts — command palettes, minibuffers, quick key layers, plain files, fuzzy navigation, and durable local workflows — and leaves the rest behind.

For the product/design charter, see [`PHILOSOPHY.md`](PHILOSOPHY.md). For the knowledge-work roadmap and file model, see [`SLATE_FLOW.md`](SLATE_FLOW.md).

---

## Why you might want Slate

Use Slate if you want:

- a normal text editor first, not a lifestyle change
- local Markdown files as the source of truth
- commandline-first workflows without living in Vim
- quick capture that gets out of your way
- fuzzy file/open/recent/mark navigation without sidebars
- inline Markdown help while keeping raw text editable
- a small agenda/task layer over plain Markdown checkboxes
- backlinks, mentions, daily logs, and refile workflows without a graph dashboard
- minimal UI: no menubar, no toolbar, no permanent project sidebar

Slate should feel like opening a clean terminal and writing.

---

## Current feature snapshot

### Editor core

- Native Rust GUI editor using `eframe`/`egui`.
- Slate-owned main editor; it does **not** rely on `egui::TextEdit` for the document.
- Plain text and Markdown editing.
- New/open/save/save-as/quit.
- Dirty-state tracking.
- Per-file cursor position persistence.
- Mouse text selection.
- System clipboard copy/paste.
- `Tab` indents by four spaces; `Shift+Tab` dedents by up to four leading spaces.
- Multiline selections can be indented/dedented as a block with `Tab` / `Shift+Tab`.
- Word wrap toggle.
- Absolute and relative line numbers.
- Goto line/column, including relative jumps.
- Find mode with highlights and next/previous navigation.
- Rich status bar with file state, active mode, line/column, line count, word count, char count, wrap state, and shortcut hints.
- Bottom command line/minibuffer instead of popover-heavy UI.

### Commands and discovery

- `Ctrl+.` opens Slate's command line.
- `Ctrl+P` opens the command palette.
- Commands are stored in a real command registry shared by command line, palette, fuzzy suggestions, usage ranking, and help.
- Live terminal-style command completion while typing.
- Fuzzy command suggestions below the command line.
- Command history and command usage persistence.
- Vim-like aliases where useful: `:w`, `:q`, `:wq`, `:x`, `:e`, `:g`, etc.

### File navigation

- Slate-owned file open modal; no dependency on desktop-native file dialogs for the main workflow.
- Save-as modal.
- Recent files picker.
- Open last file command.
- Project-root file opening.
- Shared Slate Picker surface for open/recent/open-buffer/Lance workflows.
- Periscope: fzf-style project/home file search.
- Lance: marked-file quick navigation inspired by Harpoon.
- Editable modal buffer workflow for opening another file on top of the current file.

### Markdown editing and preview

- Markdown preview split.
- Optional live inline Markdown render layer in the editor.
- Preview split forces source on the left and rendered Markdown on the right.
- Headings, lists, blockquotes, inline code, fenced code, links, separators, and checkboxes receive lightweight visual affordances.
- Markdown tables render in preview.
- Markdown links can be opened contextually.
- `Ctrl+LeftClick` opens web links in the browser and local Markdown links in Slate.
- Compact checkbox input: typing `[] ` expands to `[ ] `.
- Checkbox states: `[ ]`, `[/]`, `[x]`.
- `Ctrl+Enter` cycles checkbox state on the current line.
- `:doc-tasks` browses current-document checkboxes and can cycle them in-place.

### Capture and scratch

- `slate --scratch` launches a quick scratch window that appends to `~/.local/share/slate/scratch.md` on exit.
- `:scratch` opens an in-app scratch modal.
- Scratch modal supports multiline editing with Slate's native editor.
- `Ctrl+S` archives scratch content.
- `Ctrl+E` opens scratch entries.
- `:capture` captures the current selection, or the current line if there is no selection, with optional context/title.
- `:scratch-entries` reviews and deletes archived scratch entries.

### Slate Flow knowledge-work lane

Slate Flow is the local-first knowledge layer over ordinary Markdown files.

Current commands and workflows include:

- `:agenda` — task agenda over Markdown tasks.
- `:quickadd` / `:qa` / `:inbox` — append a task to the current Flow inbox.
- `:daily` — open monthly daily reflection log; supports today, yesterday, explicit dates, and refresh.
- `:refile` — copy the current Flow line to another Markdown file and write bidirectional links.
- `:flow-rescan` — rescan the current Flow scope into `.slate` and agenda cache.
- `:flow-rebuild` — rebuild the current Flow scope index/cache.
- `:backlinks` — show Markdown links pointing to the current file.
- `:mentions` — show textual mentions of the current file.
- `:find-note` — find Markdown notes by file, heading, or content.

Flow remains plain-text first: Markdown files are authoritative, `.slate` metadata is rebuildable, and cache/index files are derived.

---

## Install and run

From source:

```bash
cargo run --release
cargo run --release -- ~/notes.md
```

After installing a local `slate` binary:

```bash
slate
slate ~/notes.md
```

If Slate is already running, `slate file.md` opens the file in that window
instantly instead of starting a new one. Use `slate --new-window file.md`
(`-n`) for a separate window, or set `single_instance = false` in
`config.toml`. See [`docs/speed-work.md`](docs/speed-work.md).

Files open in tabs: opening a file that is already open switches to its tab,
and `slate file.md` adds a tab to the running window. Drag tabs to reorder
them; files with the same name show their folder (`README.md · notas`). Tabs survive closing the
window and restarting Slate.

Closing the window keeps Slate running in the tray, so opening it again is
instant (~25 ms instead of ~300 ms). Quit for real from the tray icon or with
`:quit-all`. In Settings you can turn this off, or start Slate hidden with
Windows so the first open of the day is instant too (`slate --background`).

Slate renders with wgpu (Direct3D 12 flip model on Windows) for the lowest
keystroke latency, and falls back to OpenGL if that fails. Set
`SLATE_RENDERER=glow` to force OpenGL.

Scratch mode:

```bash
slate --scratch
```

---

## First five minutes

1. Open Slate:

   ```bash
   slate ~/notes.md
   ```

2. Type normally.
3. Save with `Ctrl+S` or `:w`.
4. Open the command line with `Ctrl+.` and try:

   ```text
   :open
   :recent
   :scratch
   :periscope
   :settings
   ```

5. Press `Ctrl+H` for the in-app shortcut and command reference.

---

## Main shortcuts

| Shortcut | Action |
| --- | --- |
| `Ctrl+P` | Command palette |
| `Ctrl+.` | Command line / minibuffer |
| `Ctrl+H` | Shortcut and command help |
| `Ctrl+S` | Save |
| `Ctrl+Alt+S` | Save As |
| `Ctrl+O` | Open file |
| `Ctrl+N` | New buffer |
| `Ctrl+M` | Toggle Markdown preview |
| `Ctrl+F` | Find |
| `Ctrl+Enter` | Cycle checkbox state on current line/task |
| `Tab` | Indent current line or selected lines by 4 spaces |
| `Shift+Tab` | Dedent current line or selected lines by up to 4 spaces |
| `Ctrl+LeftClick` | Open Markdown/web link under pointer |
| `F12` | Timing overlay (frame, keystroke, find, startup) |
| `Ctrl+T` | New tab |
| `Ctrl+W` | Close tab (asks first when it has unsaved changes) |
| `Ctrl+Tab` / `Ctrl+Shift+Tab` | Next / previous tab |
| `Ctrl+wheel` | Next (up) / previous (down) tab |
| `Ctrl+1` … `Ctrl+9` | Go to tab 1–8, `Ctrl+9` = last tab |
| `Ctrl+Shift+T` | Reopen the last closed tab |
| `Ctrl+Q` | Close the window (tabs kept; Slate stays in the tray) |
| `Alt+1` … `Alt+9`, `Alt+0` | Open Lance slots 1 through 10 |
| `Alt+Up` / `Alt+Down` | Move current line up/down |

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

## Command line examples

Open the command line with `Ctrl+.`. Commands may be typed with or without the leading `:` in most contexts, but the examples use `:` for clarity.

### File/session commands

```text
:w                         save
:q                         close the window (Slate stays in the tray)
:wq / :x                   save and close the window
:close / :bd               close the current tab
:quit-all / :exit-all      quit Slate completely
:new / :enew               new buffer
:open / :e                 open file picker
:open ~/notes/todo.md      open explicit path
:save-as                   save current buffer to a new path
:open-last / :last / :ol   open last file
:recent / :rec             recent files picker
:recent notes              recent files filtered by query
```

### View/settings commands

```text
:preview / :md             toggle Markdown preview
:preview on|off            set preview state
:preview-on / :preview-off explicit preview commands
:wrap                      toggle word wrap
:wrap on|off               set wrap state
:wrap-on / :wrap-off       explicit wrap commands
:line-numbers absolute     absolute line numbers
:line-numbers relative     relative line numbers
:ln absolute|relative      short form
:settings                  settings modal
:perf / :hud               toggle the timing overlay (also F12)
```

### Navigation and editing commands

```text
:find text                 find text
:g 10                      go to line 10
:g 10:4                    go to line 10, column 4
:g +5                      move five lines down
:g -5                      move five lines up
:top / :gt                 top of file
:bottom / :gb              bottom of file
:select-word / :sw         select word under cursor
:select-line / :sl         select current line
:delete-word / :dw         delete word under cursor
:delete-line / :dl         delete current line
:duplicate-line / :dup     duplicate current line
:duplicate-place / :dupp   duplicate current line, then place it interactively
:move-line-up / :mlu       move current line up
:move-line-down / :mld     move current line down
:move-line-to-paragraph-start / :mlps
:move-line-to-paragraph-end / :mlpe
```

### Picker/navigation commands

```text
:open-buffer / :ob         open a file in editable modal buffer
:periscope / :ps / :fzf    project/home file search
:lance / :lc / :marks      marked files picker
:lance-add / :la           mark current file
:lance-add ~/notes/ref.md  add file manually
:lance 1                   jump to Lance slot 1
:lance-next / :lnext       next Lance mark
:lance-prev / :lpv         previous Lance mark
```

### Scratch/capture commands

```text
:scratch / :sc             in-app scratch modal
:capture / :cap            capture selection/current line to scratch
:scratch-entries           review scratch archive
:scratch-log / :scl        aliases
```

### Markdown/knowledge commands

```text
:open-link / :olink / :follow-link    open Markdown link under cursor
:doc-tasks / :tasks                   current-document task picker
:agenda / :ag                         Slate Flow agenda
:agenda #tag                          agenda filtered by tag
:agenda @project                      agenda filtered by project
:agenda overdue                       overdue agenda items
:quickadd comprar pan #casa ALTA      append task to inbox
:qa revisar README @slate             quickadd alias
:daily                                open today's monthly daily log
:daily yesterday                      open yesterday
:daily 2026-06-11                     open explicit date
:daily refresh                        refresh daily view/log
:refile                               refile current Flow line
:backlinks / :bl                      links pointing to current file
:mentions / :refs                     textual mentions of current file
:find-note / :fn query                find Markdown notes
:flow-rescan / :flow-index            rescan Flow scope
:flow-rebuild / :flow-reindex         rebuild Flow scope
```

---

## Slate Picker

`:open`, `:open-buffer`, `:recent`, and `:lance` share the same picker surface:

```text
type to filter
↑ / ↓ select
Enter open selected item
Esc close
→ enter folder
← parent folder
```

Additional Lance actions:

```text
Ctrl+Enter   open selected mark as modal buffer
Ctrl+A       add current file
Delete       remove selected mark
Alt+1..0     jump directly to mark slots 1..10
```

---

## Periscope file search

Periscope is Slate's fzf-style file search.

```text
:periscope
:ps
:fzf
```

Behavior:

- Project mode searches the nearest `.git` root.
- If no `.git` root exists, Slate can choose a project folder or jump to Home mode.
- Home mode indexes `$HOME` in the background and searches the local file list in memory.
- Path-aware queries match components in order:

  ```text
  descargas rmenu main
  lua periscope readme
  etc/ssh config
  ```

- Folder results can expand inline.
- `Tab` toggles project/home search.
- `Enter` opens or expands.
- `Ctrl+Enter` opens files as modal buffers.

---

## Ctrl-hold command layer

Slate has a transient command layer for fast commands without entering a permanent modal mode:

1. hold `Ctrl`
2. type a short sequence
3. release `Ctrl`

Examples:

| Sequence | Action |
| --- | --- |
| hold `Ctrl`, `s`, release | Save |
| hold `Ctrl`, `o`, release | Open |
| hold `Ctrl`, `o l`, release | Open last file |
| hold `Ctrl`, `d l`, release | Delete line |
| hold `Ctrl`, `d w`, release | Delete word |
| hold `Ctrl`, `d u p`, release | Duplicate line |
| hold `Ctrl`, `d u p p`, release | Duplicate placement mode |
| hold `Ctrl`, `s w`, release | Select word |
| hold `Ctrl`, `s l`, release | Select line |
| hold `Ctrl`, `g t`, release | Go to top |
| hold `Ctrl`, `g b`, release | Go to bottom |
| hold `Ctrl`, `h`, release | Shortcut help |

This gives Slate some of the speed of modal editors while keeping normal typing as the default.

---

## Duplicate placement

Use:

```text
:duplicate-place
:dupp
```

Slate duplicates the current line and enters a transient placement mode:

```text
Alt movement or Ctrl+Shift movement   move duplicate
Enter / Space                         place
Esc                                   cancel
```

---

## Alt structural editing

`Alt` is reserved for structural text operations and quick Lance slot jumps.

| Action | Slate movement mode | Vim movement mode |
| --- | --- | --- |
| Move line up | `Alt+i` / `Alt+Up` | `Alt+k` / `Alt+Up` |
| Move line down | `Alt+k` / `Alt+Down` | `Alt+j` / `Alt+Down` |
| Move line to paragraph start | repeat up key, e.g. `Alt+i i` | repeat up key, e.g. `Alt+k k` |
| Move line to paragraph end | repeat down key, e.g. `Alt+k k` | repeat down key, e.g. `Alt+j j` |
| Extend word selection left | `Alt+j` repeatedly | `Alt+h` repeatedly |
| Extend word selection right | `Alt+l` repeatedly | `Alt+l` repeatedly |
| Open Lance slot | `Alt+1` … `Alt+9`, `Alt+0` | same |

A paragraph is a contiguous block of non-empty lines separated by blank lines.

---

## Ctrl+Shift live movement

For movement that should happen immediately, Slate supports a live movement layer while holding `Ctrl+Shift`.

Vim movement:

```text
h left
j down
k up
l right
```

Slate movement:

```text
i up
j left
k down
l right
```

Fast structural cursor jumps use the same movement preference with `Shift+Alt` double-key sequences:

```text
Slate mode: Shift+Alt+i i paragraph start, k k paragraph end, j j word start, l l word end, j l line end, l j line start
Vim mode:   Shift+Alt+k k paragraph start, j j paragraph end, h h word start, l l word end, h l line end, l h line start
```

The movement mode is persisted in `~/.config/slate/config.toml`.

---

## Markdown conventions

Slate keeps Markdown plain. It adds conveniences, not a new format.

Supported lightweight affordances include:

- headings: `#`, `##`, `###`, etc.
- unordered and ordered lists
- blockquotes: `> quote`
- fenced code blocks: <code>```rust</code>
- inline code: `` `code` ``
- Markdown links: `[text](target)`
- horizontal separators: `---` alone on a line
- preview-only simple tables
- checkboxes:

  ```md
  [ ] todo
  [/] doing
  [x] done
  - [ ] subtask
  -- [/] subsubtask
  ```

Compact checkbox input:

```text
[] task
```

is normalized to:

```md
[ ] task
```

---

## Slate Flow task grammar

Slate Flow understands simple Markdown tasks:

```md
- [ ] Task text #tag @project ALTA
```

States:

```text
[ ] todo
[/] doing
[x] done
```

Inline metadata:

```text
#tag
@project
ALTA / MEDIA / BAJA
```

Nearby child metadata can be used for dates and related context:

```md
- [ ] Ship README @slate ALTA
  - due: 2026-06-12
  - scheduled: 2026-06-11
  - related: [Slate Flow](SLATE_FLOW.md)
  - note: keep the file readable and local-first
```

Agenda/cache data is derived from Markdown. Editing or completing items should update the source Markdown where possible.

---

## Scratch and capture details

Scratch archive path:

```text
~/.local/share/slate/scratch.md
```

In-app scratch:

```text
:scratch
:sc
```

Controls:

```text
Ctrl+S   archive scratch content
Ctrl+E   open scratch entries
Esc      hide scratch modal
```

Capture selection/current line:

```text
:capture
:cap
```

Scratch archive review:

```text
:scratch-entries
:scratch-log
:scl
```

Controls:

```text
↑ / ↓             select archived entry
Ctrl+D / Delete   delete selected entry
Esc               close
```

---

## Settings and persistence

Open settings:

```text
:settings
:set
:prefs
:preferences
```

Slate persists lightweight human-readable configuration in:

```text
~/.config/slate/config.toml
```

Current persisted state includes:

- command history limit
- command history
- recent/frequent command usage
- line number mode
- word wrap
- Markdown preview mode
- last opened file
- recent files
- per-file cursor positions
- Ctrl+Shift movement mode
- reopen-last-file preference

Project/local derived Flow metadata may live under `.slate/`. Cache/index files are rebuildable and are not the source of truth.

---

## Architecture

Slate is written in Rust using `eframe`/`egui`, but the main document editor is Slate-owned:

- `EditorBuffer` owns text, cursor, selection, line index, and edit primitives.
- `EditorView` paints text, cursor, selections, line numbers, wrapping, highlights, Markdown affordances, and link affordances.
- Search, goto, selection, task cycling, commands, and navigation operate on Slate primitives.
- `egui::TextEdit` is reserved for small inputs such as command fields and settings fields.

Important source modules:

| Path | Purpose |
| --- | --- |
| `src/main.rs` | Application shell, UI state, command routing, modals |
| `src/editor_buffer.rs` | Text model, cursor/selection/edit primitives |
| `src/editor_view.rs` | Native editor rendering and input handling |
| `src/commands.rs` | Command registry, aliases, commandline parsing, fuzzy command matching |
| `src/markdown.rs` | Lightweight line-based Markdown parsing |
| `src/file_dialogs.rs` | Slate-owned open/save picker behavior |
| `src/periscope.rs` | Project/home file search and path scoring |
| `src/scratch.rs` | Scratch/capture/archive helpers |
| `src/flow.rs` | Flow task/index data model |
| `src/flow_runtime.rs` | Flow runtime paths/cache helpers |
| `src/flow_edit.rs` | Flow source-edit helpers |
| `src/agenda.rs` | Agenda query/cache helpers |
| `src/daily.rs` | Daily log/reflection helpers |
| `src/link_ui.rs` | Link/backlink/mention/find-note UI helpers |

---

## Design principles

Slate should remain:

- local-first
- Markdown-first
- keyboard-friendly
- command-oriented
- visually minimal
- normal-editor-first
- capture-first
- progressively organized

Slate should avoid:

- mandatory databases
- heavy permanent sidebars
- graph-view-first UX
- plugin complexity before the core is excellent
- infinite settings
- cloning Vim/Emacs/Obsidian wholesale
- project-management cosplay

---

## Roadmap

### Done / active foundation

- Native editor architecture.
- Native buffer and renderer.
- Native input, selection, cursor jumps, scroll-to-position.
- Search/find on native editor primitives.
- Status bar and command line/minibuffer layout.
- Command registry, fuzzy command suggestions, and palette integration.
- Persistent config basics.
- Recent files workflow.
- Slate-owned open/save-as pickers.
- Ctrl-hold command layer.
- Ctrl+Shift movement layer.
- Scratch, capture, and scratch archive review.
- Markdown affordance layer and preview split.
- Periscope file search.
- Lance marked files.
- Slate Flow task syntax/index/agenda/daily/refile/link lanes.

### Next likely work

- Buffer/multi-file workflow polish.
- Optional auto-save.
- Theme system.
- Richer Flow metadata without bloat.
- Global notes search language.
- Wiki-link parser/resolver/follow-link navigation.
- Textual result buffers for search/backlinks/tasks.
- Optional sync/checkpoint lane.
- Literate Markdown configuration.

---

## Credits and inspiration

Slate's `Lance` marked-buffer / quick-file workflow is inspired by [Harpoon 2](https://github.com/ThePrimeagen/harpoon/tree/harpoon2). Full credit to [ThePrimeagen](https://github.com/ThePrimeagen) for the original idea.

Slate also takes broad inspiration from terminal editors, command palettes, minibuffers, Org-style agendas, local-first note systems, and tools that make text feel fast without making the interface loud.

---

## Current vibe

No dashboard. No ceremony. No permanent sidebar yelling for attention.

Just text, commands, capture, and enough structure to make thinking easier.
