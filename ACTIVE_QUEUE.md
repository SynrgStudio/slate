# ACTIVE_QUEUE.md

## Queue

### T001 — Review ThreadSuite generated context

Status: pending
Scope:
- Review inferred Rust/Cargo project context and confirm or edit generated facts.

### T002 — Command palette as Slate's primary interface

Status: pending
Scope:
- Add fuzzy command search instead of substring-only matching.
- Add command aliases such as `w`, `q`, `wa`, `md`, `wrap`, `:w`, `:q`, and `:wq`.
- Add recent/frequent commands.
- Explore command arguments such as `open ~/notes/todo.md`.
- Keep Slate command-palette-first: commands before menus/toolbars.

### T003 — Persistent configuration

Status: pending
Scope:
- Persist editor preferences: theme, word wrap, preview mode, last file, selected vault/notes folder, scratch behavior, wiki-link insert style, and link resolver behavior.
- Keep configuration lightweight and human-readable.
- Add an optional configurable vault/root notes directory for knowledge-management workflows.
- Support toggleable wiki-link styles: compact deep link (`file.md:line:column`) and full Markdown-friendly display link (`file.md:line:column|visible text`).
- Support a future literate Markdown config as the human-facing configuration source.

### T004 — Recent files workflow

Status: pending
Scope:
- Track recently opened/saved files.
- Track the last opened file and support `open-last` / `ol` plus Ctrl-layer `ol` as an early lightweight workflow.
- Add `Open recent` command in the command palette.
- Optionally reopen the last file at startup.

### T005 — Scratch buffer and quick capture

Status: pending
Scope:
- Improve scratch mode as a fast capture inbox.
- Support daily archive sections and quick append behavior.
- Consider command palette entries for `scratch`, `capture`, and `daily note`.
- Treat scratch as the central mental inbox: capture now, organize later.
- Support sending selected text/current line to scratch.

### T006 — Daily notes

Status: pending
Scope:
- Add command to create/open today's note.
- Use a configurable notes directory and simple Markdown template.
- Preserve the terminal-like, minimal interaction style.
- Consider commands for `daily`, `yesterday`, `tomorrow`, and `append daily`.

### T007 — Core editing improvements

Status: in_progress
Scope:
- Track and display current line/column.
- Add absolute and relative line-number modes, configurable in Settings.
- Add `:goto` / `:g` / `:line` / `:l` commands for absolute jumps and explicit `+/-` relative jumps.
- Add duplicate line, move line up/down, and delete line commands.
- Add `Alt+Up` / `Alt+Down` as primary shortcuts to move the current line or selected lines.
- Add useful Vim-inspired text operations without requiring a full Vim mode.

### T008 — Lightweight Markdown improvements

Status: pending
Scope:
- Improve preview rendering without making Slate heavy.
- Support checklists, blockquotes, separators, local links, better code blocks, and simple tables where practical.
- Keep Markdown as plain files plus lightweight conventions, not a custom database format.

### T009 — Theme system

Status: pending
Scope:
- Formalize current theme as terminal-dark.
- Add alternate themes such as nord, amber-terminal, green-phosphor, and blueprint.
- Add theme selection via command palette and persist the choice.

### T010 — Minimal file explorer / project file opening

Status: pending
Scope:
- Add a command-palette-first file opener for a directory/project.
- Search files by name without adding a heavy sidebar.
- Keep the UI minimal: no menubar, no toolbar.

### T011 — Buffers / multi-file workflow

Status: pending
Scope:
- Support multiple open buffers.
- Add switch-buffer and close-buffer commands.
- Prefer command-palette-driven buffers before visible tabs.
- Borrow the useful Emacs concept of buffers without adopting Emacs complexity.

### T012 — Optional auto-save

Status: pending
Scope:
- Add configurable auto-save for suitable workflows.
- Consider save-on-focus-loss or timed auto-save.
- Keep auto-save especially useful for scratch and daily notes.

### T013 — Terminal/Vim-like command language

Status: pending
Scope:
- Support command palette commands like `save`, `quit`, `save quit`, `new`, `open`, `wrap`, and `preview`.
- Support common aliases such as `:w`, `:q`, and `:wq`.
- Allow knowledge commands like `:daily`, `:scratch`, `:recent`, and `:theme amber`.

### T014 — Templates

Status: pending
Scope:
- Add templates for daily notes, meetings, projects, ideas, and journaling.
- Add `New from template` through the command palette.
- Keep templates as editable plain text/Markdown files where possible.

### T015 — Task/checklist commands

Status: pending
Scope:
- Detect Markdown task lines like `- [ ]` and `- [x]`.
- Add command to toggle the current task.
- Later consider listing tasks from the current file or notes folder.
- Consider archiving completed tasks without creating a full productivity system.

### T016 — Richer status bar and command line layout

Status: pending
Scope:
- Show file path/name, modified state, status message, line/column, mode, wrap state, word count, and shortcut hint.
- Move status bar to the penultimate line instead of the final line.
- Reserve the final line as a dedicated command line / minibuffer.
- Let transient UI modes such as Ctrl-hold command layer, command entry, search, link resolver, and future swipe-like actions use the command line.
- When the command line is active, status remains visible above it instead of being replaced.
- Keep the visual style terminal-like: status line plus command line, similar to Vim/Emacs minibuffer concepts.

### T017 — Slate knowledge-work philosophy

Status: pending
Scope:
- Define Slate as a personal text workspace / terminal-like knowledge editor, not an Obsidian/Logseq clone.
- Prioritize: local files, capture first, commands before UI, progressive organization, and simple Markdown.
- Explicitly avoid early graph views, mandatory databases, heavy sidebars, plugin complexity, sync systems, and Emacs-level configurability.

### T018 — Optional vault / local knowledge base

Status: pending
Scope:
- Add a `Select vault` command that picks a normal folder as Slate's optional knowledge workspace.
- Keep Slate usable as a regular text editor even when no vault is selected.
- Treat the configured vault as the root for scratch, daily notes, ideas, projects, wiki links, and search.
- Use normal folders/files such as `daily/`, `scratch.md`, `ideas/`, and `projects/`.
- Add commands for creating, opening, and searching notes within that root.

### T019 — Wiki links and note navigation

Status: pending
Scope:
- Support simple wiki links like `[[Slate roadmap]]` in Markdown text.
- Support deep wiki links to exact positions: `[[path/file.md:line:column]]`.
- Support full display links for readability: `[[path/file.md:line:column|visible text]]`.
- Add a toggle/preference for compact links vs full display links.
- Add command to open/create the note under cursor or selected wiki link.
- Add shortcut to follow link under cursor and jump to target file/line/column.
- Later support backlinks without requiring a graph view or database-heavy model.

### T020 — Global notes search

Status: pending
Scope:
- Add text search across the configured notes directory.
- Search by note/file name, content, tags, and later backlinks.
- Prefer search results in the command palette or a minimal modal.
- Reuse the same search/index primitives for the `[[` link resolver.

### T021 — Progressive organization commands

Status: pending
Scope:
- Add commands to move selection/current line to a new note, append to daily note, append to scratch, convert line to task, and archive completed tasks.
- Support commands that both edit the current text and append/capture a copy elsewhere.
- Support the workflow: write first, organize later.

### T022 — Tags and lightweight metadata

Status: pending
Scope:
- Support simple inline tags like `#idea`, `#project`, and `#todo`.
- Search/filter notes by tags.
- Avoid complex frontmatter requirements unless clearly useful.

### T023 — Vault index architecture: Markdown source plus rebuildable SQLite cache

Status: pending
Scope:
- Keep Markdown files as the source of truth.
- Use SQLite only as an optional/rebuildable index/cache for search, links, backlinks, tags, tasks, and recent/frequent results.
- Evaluate SQLite FTS5 for fast content search and snippets.
- Ensure Slate can delete/rebuild the index from the vault at any time.
- Do not store canonical note content only inside SQLite.

### T024 — Link resolver trigger for `[[`

Status: pending
Scope:
- Detect when the user types `[[` in the editor.
- Open an inline fzf-like resolver/modal without disrupting normal writing.
- Keep filtering live as the user types inside the unfinished link.
- Support Escape to cancel and Enter to insert the selected result.
- Make the resolver available as a command too, not only through automatic trigger.

### T025 — Link resolver ranking and result groups

Status: pending
Scope:
- Merge multiple result types into one selector: most-linked notes, recent notes, filename/title matches, content matches, tag matches, and create-new-note action.
- Rank filename/title matches above content matches when both are strong.
- Show content matches with snippet, file path, line, and column.
- Highlight matched query text in results where practical.
- Prefer deterministic ordering when scores tie.

### T026 — Content-match deep link insertion

Status: pending
Scope:
- When selecting a filename/title result, insert a normal wiki link such as `[[path/file.md]]` or `[[Note title]]`.
- When selecting a content result, insert a deep link to exact position.
- Support compact style: `[[path/file.md:line:column]]`.
- Support full style: `[[path/file.md:line:column|visible text]]`.
- Choose visible text from matched word, selected snippet, note title, or user-edited label depending on context.

### T027 — Wiki-link parser and target resolver

Status: pending
Scope:
- Parse wiki links robustly: `[[Note]]`, `[[path/file.md]]`, `[[path/file.md:line]]`, `[[path/file.md:line:column]]`, and `[[path/file.md:line:column|label]]`.
- Resolve note titles and relative paths inside the selected vault.
- Safely handle spaces, punctuation, duplicate titles, missing files, and renamed/moved notes.
- Define behavior for links outside the vault or when no vault is selected.

### T028 — Follow-link navigation and cursor jump

Status: pending
Scope:
- Add shortcut such as Ctrl+Enter to follow the wiki link under cursor.
- Open the target file and jump to the target line/column when present.
- If target file does not exist, offer to create it in the vault.
- Preserve current buffer state and dirty checks when navigating.
- Later integrate with buffer history/back navigation.

### T029 — Link display ergonomics and visual affordances

Status: pending
Scope:
- Keep plain Markdown text editable and readable.
- Optionally provide subtle highlighting for wiki links in the editor/preview.
- Avoid making long deep links annoying in prose by supporting full display links.
- Add commands/toggles to convert compact links to full links and full links to compact links.
- Consider a statusbar/tooltip preview of the real target when cursor is on a link.

### T030 — Backlinks and most-linked notes

Status: pending
Scope:
- Track wiki-link references across the vault through the index.
- Use backlink counts to power the resolver's `most linked` group.
- Add command to show backlinks for the current note.
- Avoid graph-view-first UX; backlinks should be textual/searchable first.

### T031 — Link resolver performance and indexing lifecycle

Status: pending
Scope:
- Start with simple filesystem scanning if needed, then move to SQLite/FTS when vault size demands it.
- Incrementally update the index when files are opened/saved/changed.
- Provide manual `Rebuild vault index` command.
- Keep resolver responsive for large vaults by limiting result counts and doing expensive work off the UI path where possible.

### T032 — Soft Vim-inspired editing grammar

Status: pending
Scope:
- Explore Vim's useful editing grammar without adding a permanent Vim mode.
- Support temporary/operator-style commands such as delete word, change word, delete line, change inside quotes/parens, and jump by word/paragraph.
- Avoid requiring constant mode toggling; prefer transient command capture, command palette actions, or leader/chord-style interactions.
- Keep normal typing as the default behavior.
- Make commands discoverable and configurable before expanding too far.

### T033 — Ctrl-hold command layer

Status: in_progress
Scope:
- Design a low-friction way to execute short editing commands like `dw` without becoming a full modal editor.
- Use Ctrl as a physical temporary command layer: collect key sequence while Ctrl is held, then confirm when Ctrl is released.
- Preserve normal shortcuts by interpreting single-key sequences on Ctrl release: Ctrl+C then release copies, Ctrl+V then release pastes, Ctrl+S then release saves.
- Treat single-key Ctrl chords and multi-key Ctrl-layer commands as distinct by waiting until Ctrl release before dispatching: hold Ctrl, press `s`, release Ctrl means save; hold Ctrl, press `s`, press `w`, release Ctrl means parse the multi-key sequence `sw`.
- Support multi-key sequences while Ctrl remains held: Ctrl+C then W then release can mean change word; Ctrl+D then W then release can mean delete word; Ctrl+D then L then release can mean delete line.
- Prefer Ctrl-release as the primary commit signal instead of post-key timeout recognition.
- Use timeout only as a safety/cancel mechanism for stuck keys or abandoned sequences, not as the normal recognition path.
- Avoid awkward multi-key modifier combos that require “pulpo manotas”.
- Show the pending command in the dedicated command line while Ctrl is held, e.g. `ctrl:c` then `ctrl:cw`, while the status bar remains visible above it.
- Provide Escape/cancel and safe fallback so accidental command entry is harmless.
- Avoid conflicts with essential OS/window-manager shortcuts; decide which Ctrl sequences Slate owns and which stay native.
- First implementation supports existing single-key Ctrl commands on release (`s`, `o`, `n`, `p`, `q`, `m`, `.`, `f`) plus `dl` for delete current line and `ol` for open last file.
- Additional early Ctrl-layer commands: `sw` select word, `sl` select line, `dw` delete word, `gt` go to top, `gb` go to bottom.
- Ctrl+Shift live navigation: while both modifiers are held, the selected movement mode moves immediately without waiting for release.
- Movement mode is configurable in settings: Vim `h/j/k/l` or Slate `i/j/k/l` (`i` up, `j` left, `k` down, `l` right).

### T034 — Repeatable edits and lightweight macros

Status: pending
Scope:
- Add repeat-last-edit behavior inspired by Vim's `.` command.
- Consider recording/replaying a short sequence of editor operations later.
- Keep this lightweight and optional; do not implement a full macro language early.
- Ensure repeated commands work with Slate-native operations such as append-to-scratch/daily and task toggles where sensible.

### T035 — Append/capture side effects from normal editing

Status: pending
Scope:
- Add commands that insert or transform text in the current buffer while also appending/capturing related content elsewhere.
- Examples: insert a note link and append context to scratch; create task here and append to daily; mark an idea inline and append it to `ideas/inbox.md`; create a new note from selection while replacing selection with a wiki link.
- Make side effects explicit, previewable, and undo-safe where possible.
- Prefer vault-relative plain Markdown targets.

### T036 — Textual result buffers

Status: pending
Scope:
- Borrow Emacs' useful concept that tool outputs can be buffers.
- Show search results, backlinks, task lists, command output, and index diagnostics as navigable text-like buffers or minimal panels.
- Allow opening/jumping from result entries with keyboard shortcuts.
- Keep these buffers optional and command-driven, not permanent UI clutter.

### T037 — Curated defaults instead of infinite configurability

Status: pending
Scope:
- Borrow Doom Emacs' concept of curated power: strong defaults, coherent aesthetics, and discoverability.
- Avoid exposing every internal behavior as a setting too early.
- Add configuration only when it protects distinct workflows, such as compact/full links or vault location.
- Keep Slate powerful but not life-consuming.

### T038 — Literate Markdown configuration

Status: pending
Scope:
- Support a Markdown configuration document where prose explains the config and fenced config blocks define the actual settings.
- Prefer TOML blocks inside Markdown, such as code fences tagged `toml slate-config` or `slate-config`.
- Treat the Markdown config as the human-facing source and optionally generate/cache a machine-readable `config.toml` from it.
- Add commands such as `Open Slate config`, `Reload Slate config`, `Validate Slate config`, and later `Tangle Slate config`.
- Show validation/tangle errors in a textual result buffer.
- Keep config documentation close to the actual settings so the config file becomes self-documenting.
- Ensure normal plain config still works for users who do not want literate configuration.

### T039 — Native editor architecture: leave `egui::TextEdit` behind

Status: completed
Scope:
- Replace the main editing surface architecture from `String + egui::TextEdit` to Slate-owned editor primitives.
- Keep `egui::TextEdit` only for small inputs such as command line, palette, and settings fields.
- Do not attempt search/highlight/scroll features on top of `TextEdit`; build them on Slate-owned buffer/view state.
- Preserve current normal-editor behavior during migration: typing, saving, opening, wrapping, preview toggle, command line, status bar, and scratch behavior.
- Treat this as prerequisite infrastructure for reliable find, selections, cursor jumps, link following, and future editing grammar.

### T040 — EditorBuffer text model

Status: completed
Scope:
- Introduce an `EditorBuffer` that owns text, dirty state inputs, cursor byte/char position, optional selection, revision counter, and line index.
- Maintain `line_starts` or equivalent for efficient byte offset ↔ line/column conversion.
- Implement minimal mutation primitives: insert text, newline, backspace, delete, replace selection, set cursor, set selection, clear selection.
- Rebuild/update indexes safely after edits and file open/new buffer.
- Add focused unit tests for line indexing, Unicode-safe cursor movement where practical, insertion, deletion, and offset conversions.
- Initially allow Slate to still render through `TextEdit` only if useful for a staged migration, but keep mutation logic in the buffer.

### T041 — Native EditorView renderer

Status: completed
Scope:
- Build an egui-based custom editor view that paints visible lines with `Painter` instead of delegating the document to `TextEdit`.
- Track viewport/scroll state, line height, first visible line, visible line count, cursor rectangle, and selection rectangles.
- Render only visible lines for responsiveness on large files.
- Paint cursor, current line, selection, and later search/link highlights from explicit ranges.
- Support word wrap later; start with reliable no-wrap or simple wrap if needed.
- Keep the status bar and command line layout unchanged: editor area should shrink upward when bottom panels are visible.

### T042 — Native editor input handling

Status: completed
Scope:
- Route text input and keyboard events into `EditorBuffer` when the editor has focus.
- Implement minimum viable normal editing: character input, Enter, Backspace, Delete, Arrow keys, Home/End, PageUp/PageDown, mouse click to place cursor, and scroll wheel.
- Preserve existing global shortcuts such as Ctrl+S, Ctrl+O, Ctrl+N, Ctrl+P, Ctrl+Q, Ctrl+M, and Ctrl+.
- Avoid stealing OS/window-manager shortcuts such as Ctrl+Shift+S.
- Make focus transitions explicit between editor, command line, palette, settings, and future search mode.
- Add tests for buffer operations; manually validate GUI event behavior after changes.

### T043 — Native selection, cursor jump, and scroll-to-position

Status: completed
Scope:
- Support explicit cursor movement to byte offset or line/column.
- Support programmatic selection ranges independent of command line focus.
- Implement `scroll_to_cursor` / `scroll_to_line` so commands can jump to results reliably.
- Keep selection visible even when command line or a panel has focus where appropriate.
- Use this for future find results, wiki-link follow, backlinks, task result buffers, and command outputs.

### T044 — Search/find on native editor primitives

Status: completed
Scope:
- Reintroduce find only after `EditorBuffer`, `EditorView`, cursor jump, selection, and scroll-to-position exist.
- Use command line/minibuffer as the search input: Ctrl+F opens `find `; `:find query` and `:f query` are supported.
- Keep live search as `SearchState` over `EditorBuffer`, cache by buffer revision and query, and never recompute unnecessarily per frame.
- Start with exact case-insensitive search; later evaluate a real matcher such as `nucleo-matcher` for fzf-like ranking.
- Render highlights only through the native editor view and only for visible ranges.
- Support `f`/`b` to move selected result, Enter to accept, Escape to cancel, and status text such as `match 2/10`.
- Support `Ctrl+F` / `Ctrl+B` while find is active to place the cursor after/before the current match without reopening find.
- Add regression tests for search matching behavior, including one-line-many-matches and single-letter queries.

### T045 — Find fixture and regression harness

Status: in_progress
Scope:
- Keep `test-fixtures/lorem-find.md` as a manual and automated fixture for search behavior.
- Cover single-letter queries, repeated words on one line, case-insensitive matches, no-match query, and large-ish text behavior.
- Add unit tests for search result offsets and line/column mapping.
- Add a lightweight performance guard where feasible so common one-letter queries do not regress into UI freezes.

### T046 — Pragmatic command backlog: useful basics without becoming Vim

Status: pending
Scope:
- Treat this as a curated backlog, not an instruction to implement every command immediately.
- Prefer commands that map to clear editor operations and can be reused later by command palette, command line, and optional Ctrl-hold grammar.
- Keep normal typing as the default; avoid a permanent Vim mode.

Command groups worth considering:

Editing lines:
- `:delete-line` / `:dl` — delete current line.
- `:duplicate-line` / `:dup` — duplicate current line below.
- `:move-line-up` / `:mlu` — move current line or selected lines up; primary shortcut should be `Alt+Up`.
- `:move-line-down` / `:mld` — move current line or selected lines down; primary shortcut should be `Alt+Down`.
- `:join-line` / `:join` — join current line with next line.
- `:sort-lines` — sort selected lines or current paragraph.
- `:trim-line` — trim trailing whitespace on current line.
- `:trim-buffer` — trim trailing whitespace in the whole file.

Editing words/selections:
- `:select-word` / `:sw` — select the word under/near the cursor; Ctrl-layer `sw`.
- `:select-line` / `:sl` — select the current line; Ctrl-layer `sl`.
- `:delete-word` / `:dw` — delete word under/near cursor; Ctrl-layer `dw`.
- `:delete-prev-word` — delete word before cursor.
- `:change-word` / `:cw` — select/delete word and enter normal typing.
- `:uppercase` / `:upper` — uppercase selection or word.
- `:lowercase` / `:lower` — lowercase selection or word.
- `:titlecase` — titlecase selection or heading text.
- `:wrap-selection` — wrap selection with quotes, parentheses, brackets, backticks, or Markdown markers.
- `:unwrap-selection` — remove matching wrapping delimiters when safe.

Navigation:
- Already implemented: `:goto` / `:g` / `:line` / `:l` with absolute and explicit relative targets.
- `:top` / `:bottom` — jump to start/end of file; Ctrl-layer `gt` / `gb`.
- `:next-heading` / `:nh` — jump to next Markdown heading.
- `:prev-heading` / `:ph` — jump to previous Markdown heading.
- `:next-blank` / `:prev-blank` — jump between paragraph boundaries.
- `:center-cursor` — scroll current cursor line to the middle of the view.
- `:back` / `:forward` — navigation history after goto/find/follow-link jumps.

Search and replacement:
- Already implemented: `:find` / `:f` with highlighted matches and `f`/`b` navigation while active.
- `:replace old new` — replace next/current match with confirmation later.
- `:replace-all old new` — whole-buffer replace, with confirmation/status count.
- `:clear-search` — clear search highlights explicitly.
- `:find-selection` — search for current selection.
- `:grep` / `:search-files` — future vault/project text search.

Markdown and notes:
- `:toggle-task` / `:task` — toggle `- [ ]` / `- [x]` on current line.
- `:make-task` — turn current line into an unchecked task.
- `:heading 1..6` — convert current line to Markdown heading level.
- `:promote-heading` / `:demote-heading` — adjust Markdown heading level.
- `:insert-link` — insert Markdown link around selection or at cursor.
- `:insert-wikilink` — insert `[[...]]` link later integrated with vault resolver.
- `:format-table` — eventually align simple Markdown tables if practical.

Files and buffers:
- Already implemented basics: `:w`, `:q`, `:wq`, `:open`, `:new`.
- `:recent` — open recent files picker/result list.
- `:reopen` — reopen current file from disk after confirmation.
- `:rename-file` — rename current file and update path.
- `:copy-path` — copy full path or vault-relative path.
- `:buffer-next` / `:bn` and `:buffer-prev` / `:bp` — future multi-buffer navigation.
- `:buffer-close` / `:bd` — close current buffer when buffers exist.

Capture and knowledge workflow:
- `:scratch` — open scratch buffer.
- `:capture` — append selection/current line to scratch.
- `:daily` — open today's note.
- `:yesterday` / `:tomorrow` — open adjacent daily notes.
- `:append-daily` — append selection/current line to daily note.
- `:new-note` — create note in selected vault.
- `:backlinks` — show backlinks for current note once indexing exists.

View and UI:
- Already implemented: `:wrap`, `:preview`, `:settings`.
- `:line-numbers absolute|relative` — command-line toggle for the setting.
- `:theme name` — switch theme once theme system exists.
- `:zoom-in` / `:zoom-out` / `:zoom-reset` — adjust editor font size.
- `:status` — show current file/editor diagnostics in a textual result buffer.

Safety and workflow:
- `:undo` / `:redo` — proper editor history once implemented.
- `:reload-config` — reload settings from disk.
- `:validate-config` — validate future literate/plain config.
- `:help command` — lightweight command help/discoverability.

## Suggested implementation order

1. Native editor architecture: leave `egui::TextEdit` behind for the main document.
2. EditorBuffer text model with tests.
3. Native EditorView renderer for visible lines.
4. Native editor input handling for minimum viable editing.
5. Native selection, cursor jump, and scroll-to-position.
6. Search/find on native editor primitives, using the command line as minibuffer.
7. Find fixture and regression harness.
8. Define/preserve Slate knowledge-work philosophy.
9. Persistent configuration, literate Markdown config, and optional vault selection.
10. Vault index architecture: Markdown source plus rebuildable SQLite cache.
11. Command palette improvements and terminal/Vim-like command language.
12. Scratch/inbox and quick capture.
13. Daily notes.
14. Recent files.
15. Link resolver trigger for `[[`.
16. Link resolver ranking/result groups.
17. Wiki-link parser and target resolver.
18. Content-match deep link insertion with compact/full style toggle.
19. Follow-link navigation and cursor jump.
20. Global vault search.
21. Backlinks and most-linked notes.
22. Tasks, tags, and progressive organization commands.
23. Append/capture side effects from normal editing.
24. Status bar plus dedicated command line layout.
25. Soft Vim-inspired editing grammar and Ctrl-hold command layer.
26. Repeatable edits and lightweight macros.
27. Textual result buffers.
28. Templates.
29. Core editing improvements.
30. Pragmatic command backlog triage: pick the smallest useful command batch before implementing.
31. Theme system.
32. Buffers.
33. Curated defaults / Doom-like ergonomics.
34. Literate Markdown configuration polish.
35. Markdown preview improvements.

<!-- THREADSUITE:START -->
# ACTIVE_QUEUE.md

## Queue

### T001 — Review ThreadSuite generated context

Status: pending
Scope:
- Review inferred Rust/Cargo project context and confirm or edit generated facts.
<!-- THREADSUITE:END -->
