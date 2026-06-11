# ACTIVE_QUEUE.md

## Queue

Planning snapshot:
- Completed foundation: native editor architecture/buffer/view/input/selection/find plus Ctrl-hold command layer.
- Current near-term lane: finish statusbar/minibuffer, find regression larness, small editing basics, command discovery, config/recent files.
- Later lane: scratch/search/wiki-link/tag workflows.

### T001 — Review ThreadSuite generated context

Status: completed
Scope:
- Review inferred Rust/Cargo project context and confirm or edit generated facts.

### T002 — Commandline-first command discovery, with palette as browser

Status: completed
Scope:
- Principle: the commandline is the fastest primary interface; the command palette is for browsing lists of files, commands, settings, and other discoverable collections.
- Done: command-line aliases for save/quit/new/open/preview/wrap/settings/find/goto/delete/select/navigation basics.
- Done: command arguments such as `open ~/notes/todo.md`.
- Done: add live commandline autocomplete: while typing after `Ctrl+.`, show the best matching command completion inline in dim/low-opacity text, terminal-style.
- Done: show a small fzf-like live suggestion list below the commandline when useful, sourced from the real command registry rather than static help text.
- Done: add fuzzy command search instead of substring-only matching for command registry results.
- Done: register command usage from commandline, command palette, shortcuts, and Ctrl-layer where practical.
- Done: persist command history plus recent/frequent command usage in the lightweight config, capped to keep it cheap.
- Done: rank command suggestions with a conservative frequency/recency boost while keeping fuzzy quality primary.
- Done: keep the empty commandline clean; use usage ranking when filtering and in the command palette/browser.
- Keep Slate commandline-first and command-palette-supported: commands before menus/toolbars.

### T003 — Persistent configuration

Status: in_progress
Scope:
- Done: lightweight config at `~/.config/slate/config.toml`.
- Done: persist command history limit, command history, line-number mode, word wrap, preview mode, last opened file, recent files, per-file cursor positions, command usage, and Ctrl+Shift movement mode.
- Done: expose persisted wrap/preview controls in Settings and commandline (`:wrap on|off`, `:preview on|off`).
- Done: expose line-number mode through commandline (`:line-numbers absolute|relative`, `:ln absolute|relative`).
- Pending: persist theme, scratch behavior, wiki-link insert style, and link resolver behavior.
- Keep configuration lightweight and human-readable.
- Support toggleable wiki-link styles: compact deep link (`file.md:line:column`) and full Markdown-friendly display link (`file.md:line:column|visible text`).
- Support a future literate Markdown config as the human-facing configuration source.

### T004 — Recent files workflow

Status: completed
Scope:
- Done: track the last opened file and support `open-last` / `last` / `ol` plus Ctrl-layer `ol`.
- Done: track a real recent opened/saved files list and persist it in config.
- Done: add `:recent` as a commandline-first workflow that opens a live recent-file picker in the minibuffer/palette area.
- Done: support `:recent <query>` and live typing/backspace filtering inside the recent picker.
- Done: add recent command discovery through the live command registry and command palette/minibuffer.
- Decision: automatic reopen-last is not necessary by default because `Ctrl+O L` / `open-last` covers the intentional workflow.
- Done: add user-controlled `reopen_last_file_on_startup` setting for users who want Slate to reopen with the last file from the previous session.

### T005 — Scratch buffer and quick capture

Status: in_progress
Scope:
- Done: preserve existing `slate --scratch` window behavior as a fast capture inbox that appends to `~/.local/share/slate/scratch.md` on exit.
- Done: add `:scratch` / `:sc` command and command-palette entry for an in-app Slate-owned scratch modal.
- Done: scratch modal uses Slate's native `EditorBuffer`/`EditorView`, supports multiline typing, hides with `Esc`, resumes with `:scratch`, and archives with `Ctrl+S`.
- Done: app exit archives pending scratch modal content.
- Done: add `:scratch-entries` / `:scratch-log` / `:scl` modal for reviewing `~/.local/share/slate/scratch.md` as entries.
- Done: from scratch modal, `Ctrl+E` opens scratch entries; in entries, `↑↓` selects and `Ctrl+D` / `Delete` deletes an entry from the archive.
- Done: add `:capture` / `:cap` modal for sending selected text, or current line when no selection exists, to scratch with an optional title/context field focused by default.
- Pending: support quick append behavior.
- Pending: later promote entries/captures to full notes.

### T007 — Core editing improvements

Status: in_progress
Scope:
- Done: absolute and relative line-number modes, configurable in Settings.
- Done: `:goto` / `:g` / `:line` / `:l` commands for absolute jumps and explicit `+/-` relative jumps.
- Done: delete line, select line, select word, delete word, top, and bottom commands.
- Done: track and display current line/column in the statusbar.
- Done: move current line up/down commands and `Alt+Up` / `Alt+Down` shortcuts.
- Done: Alt structural vertical batch for current-line movement and paragraph-boundary movement.
- Done: duplicate line command.
- Done: support mouse text selection in the native editor with left-button drag.
- Done: support copying the current native editor selection to the system clipboard with `Ctrl+C` / OS copy events.
- Done: support pasting text from the system clipboard into the native editor, replacing the selection when one exists.
- Done: normalize Slate cursor-size environment before eframe starts so X11/XWayland does not choose a giant client-side mouse cursor only inside Slate.
- Continue adding useful Vim-inspired text operations without requiring a full Vim mode.

### T008 — Lightweight Markdown improvements

Status: in_progress
Scope:
- Improve preview rendering without making Slate heavy.
- Keep Markdown as plain files plus lightweight conventions, not a custom database format.
- Done: support Slate checkbox markers (`[ ] `, `[/] `, `[x] `) as Markdown checkboxes, not as Slate task-management objects; `[] ` expands to `[ ] ` while typing.
- Done: render checkbox markers nicely in editor/preview while keeping raw text editable when the cursor enters the marker.
- Done: `Ctrl+Enter` cycles checkbox state on the current line without moving the cursor into the checkbox.
- Done: add Markdown render layer setting: plain source mode vs live inline Markdown rendering.
- Done: when preview split is open, force the left/editor side to plain source text and the right side to rendered Markdown regardless of live render setting.
- Done: add a small shared line-based Markdown parser module for editor and preview rendering; avoid a heavy Markdown AST unless there is a strong reason.
- Done: support checkbox subtasks and subsubtasks with `- [ ]` / `- [/]` / `- [x]` and `-- [ ]` / `-- [/]` / `-- [x]`.
- Done: add `:doc-tasks` / `:tasks` for current-document-only checkbox navigation; scan only the current buffer, no global task database.
- Done: in `:doc-tasks`, `Enter` jumps to the selected task line and `Ctrl+Enter` cycles that task state in-place without leaving the picker.
- Done: support `---` as the only horizontal separator syntax when it is alone on a line, with editor and preview rendering.
- Done: support blockquotes (`> quote`) with subtle editor affordance and preview rendering.
- Done: support fenced code blocks with subtle background/padding and optional language label; no syntax highlighting required yet.
- Nice-to-lave: future syntax/semantic highlighting for fenced code blocks, probably via tree-sitter or syntect; not relevant for the current milestone.
- Done: support inline code rendering for backtick spans where practical.
- Done: polish headings in editor/preview while preserving direct plain-text editing.
- Done: polish regular unordered/ordered lists in editor/preview while preserving direct plain-text editing.
- Done: support Markdown links like `[text](./file.md)` and `[text](https://example.com)` with editor/preview visual affordance.
- Done: support contextual open for Markdown links via `:open-link`.
- Done: support `Ctrl+LeftClick` on web links to open them in the default browser and on local links to open them in Slate.
- Done: support simple Markdown tables in preview only; no table editor or auto-alignment for now.

### T009 — Theme system

Status: pending
Scope:
- Formalize current theme as terminal-dark.
- Add alternate themes such as nord, amber-terminal, green-phosphor, and blueprint.
- Add theme selection via command palette and persist the choice.

### T010 — Minimal file explorer / project file opening

Status: completed
Scope:
- Add a command-palette-first file opener for a directory/project.
- Search files by name without adding a heavy sidebar.
- Keep the UI minimal: no menubar, no toolbar.
- Superseded by T047: the regular Slate-owned `Open` modal now covers project/repo file opening with focused fuzzy input, large result list, keyboard navigation, and project-root start.
- Removed the redundant `:files` / `:file` / `:open-file` / `:of` command path and `Ctrl+Alt+O` shortcut because `Ctrl+O` already opens the same project-root modal.
- Done: skips heavyweight/generated directories such as `.git`, `target`, `node_modules`, and `.threadwell`.
- Done: file results show lightweight metadata columns for size and relative modified time.

### T011 — Buffers / multi-file workflow

Status: pending
Scope:
- Support multiple open buffers.
- Reuse the existing large local-link preview modal as the first multi-file workflow surface.
- Done: `Ctrl+LeftClick` on a local Markdown link opens the target as an editable modal buffer.
- Add `:open-buffer` / `:ob` to manually open any file into that same editable modal buffer workflow, without requiring a link under cursor.
- When a local link/file is opened in the modal, keep the current file as the main buffer and load the target as an editable secondary buffer inside the modal.
- `Enter` promotes/opens the modal buffer as the main buffer, preserving the previous file as an open buffer rather than discarding it.
- `Esc` closes the modal/secondary buffer view without switching away from the main buffer.
- `Tab` swaps/promotes the main buffer and modal buffer positions, so the user can quickly flip which file is primary and which is in the large editable overlay.
- Unlike the current read-only-ish link preview, the modal buffer should support real editing, save state, cursor/scroll, and dirty protection.
- Add Slate Picker core inspired by Telescope: a single minimal modal pattern for prompt + fuzzy results + optional preview + contextual actions.
- Done: centralized `:open`, `:open-buffer`, `:recent`, and `:lance` on one shared Slate Picker modal surface while preserving contextual actions.
- Keep the visual direction Telescope-like: simple framed results pane, preview pane, compact prompt/status line; minimal but a little more Slate-polished.
- Use Slate Picker only where the workflow is a simple "pick one item and run an action" list. Do not force admin/editing workflows into it, and keep Periscope/search as a separate feature lane.
- Add Lance-inspired marked-buffer workflow: a small persistent per-project list of important files, editable through a modal, with direct slot shortcuts and next/previous navigation.
- Done: Lance is capped at 10 quick files, with `Alt+1` through `Alt+9` and `Alt+0` jumping directly to slots 1 through 10.
- Mark current/modal file into a numbered slot; selecting a slot opens/promotes that file quickly.
- Keep the Lance idea small and Slate-native: marked files are a fast local navigation set, not a full tab bar or project manager.
- Add switch-buffer and close-buffer commands.
- Prefer command-palette-driven buffers before heavy tab-driven navigation.
- Pending: add a tiny visual buffer strip at the top of the editor that looks tab-like but behaves as context/status, not as the primary workflow. It should show the current file plus a few open buffers, dirty markers, and support quick click/keyboard switching without becoming a full browser UI.
- Pending: support a two-focus buffer workflow: keep many buffers loaded in the background, but allow choosing one secondary buffer to open "on top" of the current main buffer, reusing the modal-buffer/Lance mental model.
- Pending: when a secondary buffer is active above the main buffer, `Tab` should quickly swap/jump between the main and secondary buffers so the user can compare/check two files without losing the rest of the buffer set.
- Pending: background buffers remain loaded and selectable, but the fast working set is the current pair: main buffer + secondary overlay buffer.
- Borrow the useful Emacs concept of buffers without adopting Emacs complexity.

### T012 — Optional auto-save

Status: pending
Scope:
- Add configurable auto-save for suitable workflows.
- Consider save-on-focus-loss or timed auto-save.
- Keep auto-save especially useful for scratch and capture workflows.

### T013 — Terminal/Vim-like command language

Status: in_progress
Scope:
- Done: command line supports `save`, `quit`, `wq`, `new`, `open`, `wrap`, `preview`, `find`, `goto`, `settings`, and early edit/navigation commands.
- Done: common aliases such as `:w`, `:q`, `:wq`, `:x`, `:e`, `:f`, `:g`, `:dl`, `:dw`, `:sw`, `:sl`, `:gt`, `:gb`.
- Done: make the command list a live registry used by commandline autocomplete, fzf-like suggestions, palette results, aliases, and command palette entries.
- Done: route command palette results through the shared command registry and fuzzy matching.
- Pending: add knowledge commands like `:scratch` and future wiki/link commands, plus `:theme amber`.

### T016 — Richer status bar and command line layout

Status: completed
Scope:
- Done: statusbar shows file path/name, modified state, status message, active mode, wrap state, cursor line/column, line count, word count, char count, and shortcut hint.
- Done: statusbar sits above a dedicated command line/minibuffer.
- Done: command history, Ctrl-layer, command entry, and search use the bottom/minibuffer region without overlaying the editor.
- Done: Ctrl+H shortcut help moved from the crowded minibuffer list into its own modal with shortcuts plus registered commands.
- Done: commandline inactive hint points to command entry, help, and palette.
- Future modes such as link resolver, recent picker, file opener, and textual result buffers should reuse this statusbar/minibuffer pattern.
- Keep the visual style terminal-like: status line plus command line, similar to Vim/Emacs minibuffer concepts.

### T017 — Slate knowledge-work philosophy

Status: completed
Scope:
- Done: added `PHILOSOPHY.md` as Slate's product/design charter.
- Done: defined Slate as a personal text workspace / terminal-like knowledge editor, not an Obsidian/Logseq clone.
- Done: prioritized local files, capture first, commands before UI, progressive organization, and simple Markdown.
- Done: explicitly documented anti-goals: early graph views, mandatory databases, heavy sidebars, plugin complexity, sync systems, and Emacs-level configurability.
- Done: added feature decision rules for future scratch/wiki-link/search/tag work.

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
- Add commands to move selection/current line to a related note/file and append to scratch.
- Support commands that both edit the current text and append/capture a copy elsewhere.
- Support the workflow: write first, organize later.

### T022 — Tags and lightweight metadata

Status: pending
Scope:
- Support simple inline tags like `#idea`, `#project`, and `#todo`.
- Search/filter notes by tags.
- Avoid complex frontmatter requirements unless clearly useful.

### T024 — Link resolver trigger for `[`

Status: in_progress
Scope:
- Done: detect when the user types `[` in the editor.
- Done: open an inline fzf-like file dropdown without disrupting normal writing.
- Done: keep filtering live as the user types inside the unfinished Markdown label.
- Done: support Escape to cancel and Enter to insert the selected result.
- Done: typing `]` cancels the dropdown so checkbox/manual Markdown stays natural.
- Pending: make the resolver available as a command too, not only through automatic trigger.
- Decision: prefer standard Markdown links from the start (`[label](target.md)`), not persistent `[[wiki]]` syntax.

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
- Resolve note titles and relative paths from the current file/project context.
- Safely handle spaces, punctuation, duplicate titles, missing files, and renamed/moved notes.
- Define behavior for links outside the current project or when no project root is obvious.

### T028 — Follow-link navigation and cursor jump

Status: pending
Scope:
- Add shortcut such as Ctrl+Enter to follow the wiki link under cursor.
- Open the target file and jump to the target line/column when present.
- If target file does not exist, offer to create it near the current file or in a chosen related-notes location.
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
- Track wiki-link references across relevant Markdown files through lightweight scanning/indexing.
- Use backlink counts to power the resolver's `most linked` group.
- Add command to show backlinks for the current note.
- Avoid graph-view-first UX; backlinks should be textual/searchable first.

### T031 — Link resolver performance and indexing lifecycle

Status: pending
Scope:
- Start with simple filesystem scanning over relevant Markdown files.
- Incrementally update resolver/search data when files are opened/saved/changed if needed.
- Provide manual `Rebuild link/search data` command if caching appears.
- Keep resolver responsive for large folders by limiting result counts and doing expensive work off the UI path where possible.

### T032 — Soft Vim-inspired editing grammar

Status: in_progress
Scope:
- Done: first transient grammar exists through Ctrl-hold release dispatch.
- Done: `dl`, `dw`, `sw`, `sl`, `gt`, `gb`, `ol`, and shortcut help are implemented.
- Done: Ctrl+Shift live navigation supports configurable Vim `h/j/k/l` or Slate `i/j/k/l` movement.
- Pending: future operator-style commands such as change word, change inside quotes/parens, and jump by word/paragraph.
- Avoid requiring constant mode toggling; prefer transient command capture, command palette actions, or leader/chord-style interactions.
- Keep normal typing as the default behavior.
- Make commands discoverable and configurable before expanding too far.

### T033 — Ctrl-hold command layer

Status: completed
Scope:
- Done: Ctrl is a physical temporary command layer that collects key sequences while held and dispatches on Ctrl release.
- Done: single-key Ctrl commands remain available on release: `s`, `o`, `n`, `p`, `q`, `m`, `.`, `f`, `b` in find context.
- Done: multi-key commands: `dl` delete line, `dw` delete word, `sw` select word, `sl` select line, `ol` open last, `gt` go top, `gb` go bottom.
- Done: `h` opens a dedicated shortcut/command help modal.
- Done: pending Ctrl sequence is shown in the dedicated command line while Ctrl is held.
- Done: Ctrl+Shift live navigation moves immediately without waiting for release.
- Done: movement mode is configurable in settings: Vim `h/j/k/l` or Slate `i/j/k/l` (`i` up, `j` left, `k` down, `l` right).
- Future expansion belongs in T032/T046 rather than keeping this task open.

### T034 — Repeatable edits and lightweight macros

Status: pending
Scope:
- Add repeat-last-edit behavior inspired by Vim's `.` command.
- Consider recording/replaying a short sequence of editor operations later.
- Keep this lightweight and optional; do not implement a full macro language early.
- Ensure repeated commands work with Slate-native operations such as append-to-scratch where sensible.

### T035 — Append/capture side effects from normal editing

Status: pending
Scope:
- Add commands that insert or transform text in the current buffer while also appending/capturing related content elsewhere.
- Examples: insert a note link and append context to scratch; create a new related file from selection while replacing selection with a wiki link.
- Make side effects explicit, previewable, and undo-safe where possible.
- Prefer plain Markdown targets relative to the current file/project when possible.

### T036 — Textual result buffers

Status: pending
Scope:
- Borrow Emacs' useful concept that tool outputs can be buffers.
- Show search results, backlinks, checkbox lists, command output, and diagnostics as navigable text-like buffers or minimal panels.
- Allow opening/jumping from result entries with keyboard shortcuts.
- Keep these buffers optional and command-driven, not permanent UI clutter.

### T037 — Curated defaults instead of infinite configurability

Status: pending
Scope:
- Borrow Doom Emacs' concept of curated power: strong defaults, coherent aesthetics, and discoverability.
- Avoid exposing every internal behavior as a setting too early.
- Add configuration only when it protects distinct workflows, such as compact/full links or scratch behavior.
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

### T042 — Native editor input landling

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
- Use this for future find results, wiki-link follow, backlinks, checkbox result buffers, and command outputs.

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

### T045 — Find fixture and regression larness

Status: completed
Scope:
- Done: `test-fixtures/lorem-find.md` is both a manual and automated fixture for search behavior.
- Done: fixture covers repeated words, case-insensitive matches, no-match query, single-letter queries, Unicode-adjacent matches, stable markers, and noisy/large-ish text.
- Done: unit tests validate fixture match counts, search result offsets, and line/column mapping through `EditorBuffer`.
- Done: lightweight performance smoke test guards common one-letter queries against obvious regressions.

### T046 — Pragmatic command backlog: useful basics without becoming Vim

Status: in_progress
Scope:
- Treat this as a curated backlog, not an instruction to implement every command immediately.
- Prefer commands that map to clear editor operations and can be reused later by command palette, command line, Ctrl-hold grammar, and Alt structural editing.
- Keep normal typing as the default; avoid a permanent Vim mode.
- Treat Ctrl+Shift as live cursor movement, Ctrl-hold as command dispatch, and Alt as a structural text/line manipulation layer.

### T047 — Slate-owned file dialogs for open/save/save-as

Status: completed
Scope:
- Done: replaced native Linux/desktop dialogs from `rfd` with Slate-owned egui modals so file workflows match the terminal-like UI.
- Done: evolved the T010 project file picker for `Open`: `Ctrl+O`, Ctrl-layer `o`, and bare `:open` open Slate's modal; `:open path` / `:e path` still open directly.
- Done: every Slate-owned filesystem modal supports consistent arrow navigation: `↑↓` selects entries, `→` enters the selected directory when selection is a folder, and `←` goes back to the parent directory.
- Done: Open/SaveAs filesystem modals show both directories and files, with clear folder/file affordances, while preserving fuzzy filtering and metadata where useful.
- Done: added a Slate `Save As` modal with a focused path input, clear title, `Enter` to save, and `Esc` to cancel.
- Done: added `Ctrl+Alt+S` as the Save As shortcut while preserving `Ctrl+Shift+S` for the OS/window manager.
- Done: `Save` remains direct when the buffer already has a path; scratch/untitled buffers open the Slate `Save As` modal.
- Done: preserved unsaved-change confirmation flows before opening/replacing buffers.
- Done: removed the `rfd` dependency from `Cargo.toml`.

### T048 — Assisted Markdown link insertion

Status: completed
Scope:
- Done: add an assisted Markdown link flow triggered by typing `[`, while preserving normal manual `[text](target)` editing.
- Done: single-`[` now opens a compact vertical IDE-style file dropdown, with recent files first for an empty query and fuzzy project-file matching as the user types.
- Done: the dropdown is navigable with `↑↓`/`Tab`, `Enter`, and `Esc`; typing `]` cancels it so checkbox/manual Markdown flows like `[]` and `[ ]` remain natural.
- Done: confirming a file inserts standard Markdown `[](relative/path.md)` and places the cursor inside the label.
- Done: when selected file is Markdown and has headings, show a second heading picker; first option always links the whole file, while selected headings insert `[](relative/path.md#heading)`.
- Done: local Markdown links with heading fragments open the file and jump to the matching heading.
- Done: for `Web`, show a focused mini input for the URL and insert standard Markdown `[](https://...)` on `Enter`.
- Done: after insertion, place the cursor inside the `[]` label so the user can immediately type the link text.
- Done: pressing `Enter` on an empty local Markdown-file label fills it with the file stem, e.g. `[](docs/periscope.md)` becomes `[periscope](docs/periscope.md)`; web links keep the old behavior of moving after the link with a space.
- Done: resolve file targets relative to the current buffer when possible; fall back to project/current directory when the buffer has no path.
- Done: keep the final document plain Markdown; `[[` is only the assist trigger, not a persistent custom link syntax.

### T049 — Periscope: real fzf-style file search

Status: in_progress
Scope:
- Done: build Periscope as Slate's dedicated fast file-search workflow, separate from the generic Slate Picker/list centralizer.
- Done: add `:periscope` / `:ps` / `:fzf` command registry entries.
- Done: default mode is Project Periscope: search files under the detected project root.
- Done: project root detection starts from the current file directory and walks upward until a `.git` directory is found; that directory is the project root.
- Done: if no `.git` project root exists, ask the user whether to define a project folder.
- Done: if the user chooses yes, open a folder picker to select the project folder and then run Project Periscope there.
- Done: if the user chooses no, skip project mode and open Home Periscope directly.
- Done: `Tab` toggles between Project Periscope and Home Periscope when a project root exists; if no project root is configured, `Tab` shows a short status hint.
- Replaced the experimental whole-system `plocate`/`locate` Global Periscope with Home Periscope because whole-system fuzzy was noisy and hit-or-miss in practice.
- Done: Home Periscope indexes files under `$HOME` in the background, using `fd` when available and a Rust recursive fallback otherwise, then fuzzy-filters that local list in memory.
- Done: Project mode starts with `fd` where available, with a Rust fallback over the project root; this is separate from the `$HOME` index.
- Done: keep Periscope controls minimal: type filter/query, `↑↓` select, `Enter` open as main buffer, `Ctrl+Enter` open as modal buffer, `Tab` switch project/home, `Esc` close.
- Done: Home Periscope displays up to 100 results.
- Done: Periscope supports path-aware ordered queries such as `lua periscope readme` or `etc/ssh config`, matching tokens in path order so users can narrow by directories without filtering out system paths.
- Done: add folder-group expansion inside Periscope results. When a query strongly matches a folder, Slate shows a folder row plus inline child rows, e.g. `/home/nd/slateamp/` followed by indented `---/README.md`, `---/Cargo.toml`, etc.
- Done: `Enter` or `→` on a folder row expands/collapses that folder in the same Periscope list instead of leaving search mode or opening a separate file browser.
- Done: child rows remain normal file results: `Enter` opens the file, `Ctrl+Enter` opens it as modal buffer.
- Done: folder expansion stays transient and scoped to the current Periscope query/results; query/mode changes clear expansion state.
- Pending optimization: debounce Project Periscope query updates, cancel stale backend processes where still applicable, and keep polishing indexing lifecycle/status.
- Future but not part of the first pass: content search, custom indexes, Everything-like daemon/indexer, project-specific persisted roots, and search result buffers.

### T050 — Slate Flow: Org-inspired Markdown agenda lane

Status: in_progress
Scope:
- Started: initial design contract documented in `SLATE_FLOW.md`.
- Build a new Slate-native lane inspired by Org/Org-roam/Agenda, without turning Slate into Emacs or a heavyweight PKM.
- Principle: plain Markdown files remain the source of truth. Slate may maintain caches/indexes for speed, but every cache must be rebuildable from local files.
- Principle: tasks may live in any Markdown file touched by Slate, not only in a special vault or agenda file.
- Principle: keep the workflow local-first and commandline-first: capture fast, write normally, agenda scans/indexes, refile/link later.
- Principle: do not require graph views or visual PKM surfaces; backlinks, mentions, link insertion, and reliable navigation matter more than graph cosmetics.
- Treat this lane as related to scratch/capture, Markdown links, document tasks, Lance/Periscope navigation, and future rtasks/rmenu/thread-daemon integration.

### T051 — Slate Flow task syntax and metadata grammar

Status: completed
Scope:
- Done: added `src/flow.rs` with a Slate Flow task parser and unit tests.
- Done: parse checkbox state, clean title, tags, project/context markers, Spanish priority markers, heading context, and child metadata (`due`, `scheduled`, `related`, `note`).
- Done: checkbox metadata child lines are treated as parent metadata instead of independent agenda tasks when the key is known.
- Prefer Markdown-compatible checkbox tasks with inline tags, project markers, and priority markers:
  ```md
  - [ ] Arreglar selección con mouse #editor #input @slate ALTA
    - [ ] due: 2026-06-10
    - [ ] scheduled: 2026-06-08
  ```
- Parse task state from Slate checkbox markers:
  ```text
  [ ] todo
  [/] doing
  [x] done
  ```
- Parse tags from `#tag` tokens on the task line; examples: `#editor`, `#input`.
- Parse project/context from `@project` tokens on the task line; examples: `@slate`, `@home`, `@work`. This should map cleanly to rtasks project/context concepts later.
- Parse priority from curated Spanish priority tokens at the end or near the end of the task line:
  ```text
  ALTA
  MEDIA
  BAJA
  ```
- Prefer curated priorities over arbitrary custom priority schemes for now.
- Parse child metadata lines for dates:
  ```md
  - [ ] due: 2026-06-10
  - [ ] scheduled: 2026-06-08
  ```
- Metadata child lines may use checkbox syntax so they can remain visible/compatible with rtasks-style tooling, but Slate should understand `due:` and `scheduled:` as metadata for the parent task rather than independent agenda tasks by default.
- Also accept non-checkbox child metadata for convenience and less visual bloat:
  ```md
  - due: 2026-06-10
  - scheduled: 2026-06-08
  - related: [README](README.md)
  - note: revisar con selección Unicode
  ```
- Support a middle-ground between inline-only tasks and heavy task objects: richer per-task metadata is useful, but should stay close to the task and readable as normal Markdown.
- Keep all syntax readable and useful without Slate; no hidden proprietary task objects.

### T052 — Slate Flow task index/cache

Status: completed
Scope:
- Done: Markdown files opened/saved by Slate are parsed with `src/flow.rs` and indexed when they have a Markdown-like extension.
- Done: write a readable project-local `.slate/index.tsv` known-files index with path, seen/mtime, task count, and task/link/tag hints.
- Done: write a derived SQLite agenda cache at the Slate data dir using the system `sqlite3` command, with `files` and `tasks` tables.
- Done: status line reports how many Flow tasks were indexed when opening/saving a Markdown file.
- Done: added `:flow-rescan` / `:flow-index` to rescan the current Flow scope into `.slate/index.tsv` and the SQLite cache.
- Done: added `:flow-rebuild` / `:flow-reindex` to remove the current scope's readable index and rebuild the SQLite cache from Markdown files in the current scope.
- Add a scope-local SQLite agenda/task cache for fast agenda views, e.g. under:
  ```text
  ~/.local/share/slate/flow/scope-<hash>/agenda.db
  ```
- The SQLite cache stores derived data only; it is never the source of truth.
- Also maintain a Slate-readable project/file index, inspired by `nb` but Slate-native, such as `.slate` or `.slate/index.md` / `.slate/index.toml`, for files touched by Slate that contain valuable structured information.
- The readable `.slate` index should be auditable and rebuildable, not a hidden proprietary source of truth. It can list known files, last-seen metadata, discovered task/link/tag presence, and cache hints.
- Agenda should primarily read fast derived data from SQLite, while the readable `.slate` index documents and scopes what Slate believes is worth indexing.
- Minimum cached fields:
  ```text
  path
  line number / byte range
  task text
  checkbox state
  due date
  scheduled date
  tags
  @project
  priority
  parent heading / document context
  related links / source references
  mtime/hash/revision
  source snippet
  ```
- Update the cache incrementally when Slate opens, edits, saves, or otherwise touches a file.
- Maintain a known-files set: any file opened/edited/saved by Slate can contribute tasks to the agenda.
- Provide rebuild/rescan commands for both SQLite cache and readable `.slate` index.
- First pass can index files touched by Slate instead of scanning the entire filesystem.
- Cache should be robust against line-number drift by refreshing on save/open; line numbers are navigation hints, not permanent identity.
- Future identity may use stable task IDs only if needed; avoid forcing IDs into Markdown in the first pass.

### T053 — Slate Flow agenda

Status: completed
Scope:
- Done: added `:agenda` / `:ag` command backed by the Flow SQLite cache.
- Done: agenda opens as a Slate-native modal with an Org-style 10-day agenda layout, overdue section, dated rows, later section, inbox/no-date section, task metadata, and keyboard/mouse navigation.
- Done: `Enter` on an agenda task jumps to the source file and line.
- Done: `Ctrl+Enter` cycles the selected task state in source Markdown and refreshes the Flow cache; Slate asks for a clean buffer first to avoid overwriting unsaved edits.
- Done: commandline filters via `:agenda #tag`, `:agenda @project`, `:agenda ALTA`, `:agenda overdue`, `:agenda today`, `:agenda due`, `:agenda scheduled`, `:agenda inbox`, `:agenda doing`, `:agenda done`, and text/path/heading terms.
- Done: agenda split into two keyboard segments: dated week tasks above and no-date tasks below; `Tab` moves focus between segments.
- Done: dated segment shows only the current week as day rows; `←`/`→` changes week, `Home` returns to current week, and each day shows a task-count marker.
- Done: `Enter` on a day expands/collapses only that day; `Enter` on a task jumps to source.
- Done: selected agenda rows scroll into view inside their own segment; moving through no-date tasks does not move the dated week segment.
- Done: `x` toggles visibility of done tasks in agenda.
- Done: opening a Markdown file now refreshes that file and starts a background Flow scan of the file's project/folder, so `:agenda` no longer depends on a manual `:flow-rescan` for the opened scope.
- Done: Flow task lines in the editor get a subtle parsed-task affordance, with checkbox state bar plus colored `#tags`, `@projects`, priority markers, and known metadata keys.
- Done: `Shift+Enter` on a Flow task starts metadata entry with an indented `scheduled:` child line; empty `scheduled:` Enter becomes `due:`, and empty `due:` Enter exits back to a plain blank line.
- Done: Flow date metadata entry has inline dark autocomplete for day/month shorthands; `Enter` accepts the suggested ISO date and moves through `scheduled:` → `due:` → plain line, while `↑`/`↓` adjusts the suggested month.
- Done: agenda hides stale/global cache rows whose resolved source file does not exist in the current Flow scope.
- Done: standalone `slate --agenda` / rmenu `Slate Agenda` opens Flow agenda as its own floating window; `Enter` on a task opens a normal Slate window at that task line and closes the agenda window.
- Done: standalone agenda renders as its own focused agenda surface, without the normal Slate editor/status/commandline chrome behind it.
- Future: expose a small local Flow agenda surface/API for Waybar calendar integration, backed by the same Markdown-derived index rather than a separate task database.
- Future: richer boolean agenda/search belongs in T061 Slate search language.
- Add `:agenda` as the main view for tasks discovered across files touched by Slate.
- Agenda should group tasks into useful sections such as:
  ```text
  Today
  Scheduled
  Due soon
  Overdue
  Inbox / no date
  By project
  ```
- Agenda rows should show enough context without becoming noisy:
  ```text
  [ ] Arreglar selección con mouse   @slate ALTA   due 2026-06-10   path:line
  ```
- Pending visual polish: Agenda, Daily, and related Flow views should share Slate's editor color language. Tags, projects, priorities, file paths, source references, and link-like values should be colorized consistently across editor, agenda, daily, scratch/review surfaces, and future link/backlink views.
- `Enter` on an agenda item jumps to the source file and line.
- `Ctrl+Enter` cycles task state in the source file, matching existing checkbox behavior.
- Agenda should support filtering by:
  ```text
  #tag
  @project
  ALTA/MEDIA/BAJA
  due/scheduled/today/overdue
  ```
- Agenda is a view over Markdown, not a separate task database. Editing/completing tasks should update the source file.
- Avoid graph/PKM UI here; agenda should be fast, textual, keyboard-friendly, and commandline-first.

### T054 — Slate Flow capture, inbox, and quickadd

Status: in_progress
Scope:
- Extend existing scratch/capture into Org-like capture templates without making a mandatory PKM/vault.
- Done: add `:quickadd` / `:qa` / `:inbox` command for appending `- [ ] ...` Flow tasks to the current Flow scope inbox.
- Done: default inbox is plain Markdown at `inbox.md` inside the current Flow scope/project.
- Done: quickadded tasks are indexed immediately and refresh `:agenda` when it is open.
- Done: scratch/quickadd task normalization now applies Flow default tags, so plain quickadd tasks become `#task #inbox` and show in tag-driven Tasks/Inbox views; contextual quickadds with `#tag`, `@project`, priority, `due:`, or `scheduled:` get `#task` only. Scratch also treats raw checkbox lines like `[] tarea` / `[ ] tarea` / `- [ ] tarea` as Flow quickadds instead of archiving them only.
- Done: `:quickadd` with no arguments opens scratch with quickadd guidance.
- Done: scratch modal and standalone `slate --scratch` support special Flow quickadd lines: any line starting with `:` becomes a quickadd task, with optional `:quickadd ...`, `:qa ...`, or `:inbox ...`; non-command scratch lines still archive normally.
- Done: standalone `slate --scratch` shows a quiet centered `Agenda updated` toast for about 0.4s before closing after a saved quickadd/archive, without writing that message to the command line.
- Pending: richer Flow item parsing for non-task agenda entries, without expanding `:capture` beyond selected-text capture.
- Decision: `:capture` keeps its current meaning: capture the selected text. Do not overload it into template commands for idea/note/task.
- Decision: tasks remain normal checkbox syntax (`[ ]`, `[/]`, `[x]`); no separate `:task` syntax is needed.
- Decision: notes and ideas are written directly in the document as plain text Flow items, without requiring hand-written bullets:
  ```md
  :note Esto es una nota #tag @project
  :idea Esto es una idea #tag @project
  ```
- Decision: avoid making bullets feel mandatory for Markdown workflows. Canonical note/idea syntax is non-bullet intradocument text.
- Done: parse and index `:note` and `:idea` lines as first-class Flow items with kind `note` / `idea`, title/body text, tags, project/context, source path, line, and heading.
- Done: render `:note` and `:idea` with Flow syntax coloring in the editor, similar to task affordances: the marker is visually parsed/recognized, and inline `#tags` / `@projects` keep their existing colored treatment.
- Done: include notes and ideas in global agenda/Flow views, because agenda is the global place where everything actionable/reference-like lands.
- Done: note/idea lines participate in live Flow agenda publishing when committed/edited near Flow context, so standalone/rmenu agenda can see them from the derived cache without requiring an explicit file save after pressing Enter/committing the line.
- Pending: add timestamps to note/idea metadata when available or when Daily log lands.
- Done: agenda tabs/sections in this order: `Agenda` (global) | `Inbox` | `Tasks` | `Ideas` | `Notes`, switching tabs with `Shift+Tab` so plain `Tab` remains available for existing in-modal focus/navigation.
- Done: non-global agenda tabs are flat lists without the weekly calendar; each tab remembers its cursor while the agenda is open and resets on reopen.
- Decision: Flow tabs are tag-driven. `:idea` gets `#idea`, `:note`/`:nota` gets `#note`, checkbox tasks get `#task`, plain `: text` quickadd gets `#inbox`, and plain tasks with no extra context get `#task #inbox`.
- Done: add inline agenda organization autocomplete for tagging/projecting items without a modal. While a Flow item is selected in agenda/inbox tabs, pressing `@` opens a non-blocking project autocomplete/dropdown and pressing `#` opens a non-blocking tag autocomplete/dropdown; typing filters suggestions and confirming appends the selected `@project` or `#tag` to the source line. Plain `Enter` must keep opening/jumping to the source line, and `Ctrl+Enter` remains task state cycle.
- Done: reusable Flow item edit mini-modal and editor inline organization autocomplete. In agenda, `Shift+Enter` on the selected item opens a compact editor prepopulated with the full source item line; `Ctrl+Enter` saves, `Esc` cancels, and `#`/`@` open autocomplete using the shared project/tag suggestion engine. Standalone agenda can send edit requests to an already-open Slate buffer via `edit-requests.tsv`; otherwise it edits disk and reindexes. Modal styling is compact/Slate-like. Token replacement is cursor-aware inside the mini editor. Normal Slate editing reuses Flow `#tag` / `@project` autocomplete on Flow lines with a compact vertical IDE-style suggestion list/dropdown. Final validated behavior: `#` and `@` explicitly call the popup; known complete tokens do not reopen it; known incomplete tokens show matching suggestions; unknown tokens show no suggestions. Manual GUI validation passed. Design decision: inside the normal Slate editor, suggestions render vertically, not as horizontal chips; horizontal/wrapped chips remain acceptable in Agenda/modal contexts.
- Pending: define inbox as a first-class agenda view for uncategorized/no-date Flow items, including tasks, notes, and ideas.
- Pending: alternate destinations later: current file, daily log, choose file, project note.
- Pending: configurable/default local-data inbox option if project-local inbox is not desired.
- Capture should preserve plain Markdown output and use the task metadata grammar from T051 when capturing tasks.
- Future: allow rmenu/thread-daemon quickadd to append into the same inbox/daily/task paths.

### T055 — Slate Flow daily log

Status: completed
Scope:
- Daily is not another inbox and not another capture syntax. `:note`, `:idea`, plain `:` inbox items, checkbox tasks, Scratch, and Agenda remain the live capture/organization surface.
- Daily is a monthly chronological reflection/backup of what changed in Slate Flow: captures, edits, completions, refile events, organization changes, and notable agenda state transitions.
- Prefer one monthly text file over one tiny note per day, so a whole month is searchable/readable as one chronological artifact.
- Candidate monthly path:
  ```text
  ~/.local/share/slate/daily/2026-06.md
  ```
- Candidate format:
  ```md
  # Slate Daily Log — 2026-06

  ## 2026-06-07

  ### Captured
  - 09:30 idea — mejorar Periscope #idea @slate
  - 10:12 task — revisar selección con mouse #task @slate

  ### Completed
  - 10:45 done — Arreglar selección con mouse #task @slate

  ### Organized
  - 11:05 refile — "revisar selección" -> project/slate.md#editor

  ## 2026-06-08
  ...
  ```
- The source of truth remains the original Markdown item/source file. Daily entries are an append-only/monthly audit trail and human-readable backup, not the canonical task/note storage.
- Daily should be updated by agenda/Flow events rather than freeform daily capture. If a user wants to record a thought, they should use existing Flow syntax (`:note`, `:idea`, `:`, or `- [ ]`) and the daily can reflect that event.
- Refile should write a daily event: when something is sent to a new home, the daily entry keeps a link/reference to where it went.
- Daily `source:` references should eventually be real navigable links back to the original file and line, not inert text. The current textual `path:line` format is acceptable as an intermediate representation, but link/navigation behavior belongs in the Daily/linking integration.
- Do not erase daily history when organizing; daily is the chronological trail of what happened.
- Support command(s):
  ```text
  :daily
  :daily-log
  :daily refresh
  ```
- `:daily` / `:daily-log` should open the current monthly daily at today's section. `:daily refresh` can rebuild/repair the monthly reflection from available Flow cache/history where possible.
- Explicitly avoid adding `:log` or `:daily text` as new capture surfaces for now, because `:note` already covers that role and duplicate capture paths create confusion.
Progress:
- First implementation pass added `src/daily.rs` as the Daily reflection backend. It creates monthly files under `~/.local/share/slate/daily/YYYY-MM.md`, ensures day/section structure, appends deduplicated events, and has focused tests for document creation, section grouping, and duplicate prevention.
- Normal Flow agenda publishing now records Daily events from current-buffer agenda diffs: newly indexed Flow items become `Captured`, and tasks transitioning to done become `Completed`. `Organized` is reserved for explicit refile/organization flows so normal typing/tagging does not spam the Daily. Source Markdown remains canonical; Daily is append-only reflection/audit trail.
- Validation after first pass: `cargo fmt --all`, `cargo check`, `cargo test` (118 passed), and `cargo build --release`.
- Daily polish pass started: new Daily `source:` references are now emitted as standard Markdown links like `[slatetest.md:24](/home/nd/slate/slatetest.md#L24)`, so they inherit Slate's link colors in the editor/preview surfaces. Slate Markdown link fragments now understand `#L123` / `#line-123` and jump to that line when promoted/opened, while existing heading-anchor fragments keep working. Current local Daily file was migrated to the new source-link shape. Validation: `cargo fmt --all`, `cargo check`, `cargo test` (120 passed), and `cargo build --release`.

### T056 — Slate Flow refile with bidirectional links

Status: in_progress
Scope:
- Add `:refile` to organize selected text, current task, current line, or capture entries.
- Refile destinations:
  ```text
  inbox
  daily log
  current file heading
  another heading in current file
  chosen file
  chosen file heading
  project note
  ```
- Use a picker for destination file/heading selection.
- Default behavior should be non-destructive or link-preserving: never lose the original source without explicit user intent.
- When refiling from daily/inbox to a destination, keep the daily/inbox entry as history and add a link to the new home.
- Destination content should include a backlink/reference to the source when useful.
- This creates bidirectional navigation:
  ```text
  source daily/inbox entry -> destination note/task
  destination note/task -> original source/context
  ```
- Future optional modes may include move/delete/archive, but first pass should prefer copy/link/refile over destructive moves.
- Decision: default refile is explicitly non-destructive. It must never delete the original item by default; instead the original gets a `refiled to:` link and the destination copy gets a `source:` backlink.
Progress:
- First implementation pass added `:refile` / `:rf`. It takes the current non-empty line from a saved clean buffer, opens the normal Slate file picker in refile mode, appends a copy to the chosen destination Markdown file with `source: [file:line](source#Lline)`, and inserts `refiled to: [target](target)` under the original line. The original remains in place and the current buffer becomes dirty so the user controls saving the backlink.
- Refile writes an `Organized` Daily event with a source link, so Daily remains the audit trail for organization actions without treating normal typing/tagging as organized events.
- Refile heading pass added: after choosing a Markdown destination with headings, Slate now opens the shared heading picker in `refile heading` mode. First option appends to the end of the file; selecting a heading inserts the copied item under that section and writes the original `refiled to:` link with the heading fragment. The destination copy still gets a `source:` backlink, and Daily `Organized` remains the audit trail.
- Agenda refile pass added: pressing `r` on a selected Agenda/Inbox/Tasks/Ideas/Notes item starts the same non-destructive refile flow directly from Agenda. It reads the source line, opens the destination file picker, optionally opens the heading picker, writes the destination copy/backlink, and adds `refiled to:` to the original source. If the source is the current dirty buffer, Slate asks the user to save first to avoid losing in-memory edits. File/heading pickers take keyboard focus over Agenda while active.
- Daily Organized polish added: refile events now render as explicit audit entries with `refile — ...`, `from: ...`, and `to: ...` Markdown links instead of folding the target into the title. Destination heading anchors are preserved in the Daily `to:` link.
- Interactive single-`[` Markdown link autocomplete added: typing `[` opens a compact file dropdown with recent files first and fuzzy project-file matching; files with headings show a `▸` affordance. `Enter` inserts `[](target)` for the whole file with the cursor inside the label; `Tab` or `Shift+Enter` opens the heading picker so the link can target a heading directly; `]` cancels for checkbox/manual Markdown; pressing `Enter` on an empty local Markdown-file label fills the file stem.
- Flow metadata start now applies default task tags before adding `scheduled:`. This fixes `[] task` + metadata skip flow so the parent becomes `[] task #task #inbox` even when the user presses through empty scheduled/due fields.
- First-pass `:backlinks` / `:mentions` implemented for the current file. Results are textual Markdown modal buffers with clickable file:line links.
- Virtual end-of-document backlink/mention sections implemented for saved Markdown files. They render after the real buffer as non-saved Markdown (`---`, `## Backlinks`, `## Mentions`, `<!-- virtual: not saved to file -->`), cap inline results to 8 per section, and their result links are clickable.
- Virtual references are now configurable in Settings with a persisted `Virtual backlinks/mentions` show/hide toggle.
- Virtual references are clickable/collapsible: clicking the virtual header collapses/expands the whole section; clicking `Backlinks` or `Mentions` collapses/expands that subsection. Collapse state is persisted in settings.
- Virtual end-of-document references were removed from the editor surface after visual review; `:backlinks` / `:mentions` modal commands remain the supported reference views for now. Revisit the virtual section later with a cleaner Slate-native design.
- Validation after latest pass: `cargo fmt --all`, `cargo check`, `cargo test` (122 passed), and `cargo build --release`.

### T057 — Slate Flow links, backlinks, and roam-ish navigation

Status: in_progress
Scope:
- Keep using standard Markdown links as the final document syntax.
- Decision update: single `[` is the primary inline Markdown link resolver trigger; `[[wiki]]` is not persistent syntax for now.
- Done: add first-pass backlink/mention commands for the current file:
  ```text
  :backlinks
  :bl
  :mentions
  :refs
  ```
- Done: `:backlinks` scans project Markdown links and shows external links that resolve to the current file, including heading/line fragments.
- Done: `:mentions` scans project Markdown text for current file name/stem/path mentions, including unlinked mentions.
- Done: results render as a textual modal Markdown buffer with clickable `[path:line](absolute#Lline)` result links and preview snippets.
- Done: add virtual end-of-document references section, not physically written to the source file:
  ```md
  ---
  ## Backlinks
  ...
  ## Mentions
  ...
  ```
- Done: add Settings toggle for showing/hiding virtual Backlinks/Mentions sections.
- Done: make the virtual references section and each Backlinks/Mentions subsection click-to-collapse/click-to-expand.
- Done: `:find-note` / `:fn` / `:note-find` command.
  - Searches Markdown notes by file name, heading, and content.
  - Results are textual/keyboard-first in a modal Markdown buffer with clickable `path:line` links.
- Deferred/removed for now: `:notes` and `:insert-note-link`; user does not see enough value while normal text insertion and `[` autocomplete already exist.
- Done: bidirectional automatic linking inline command:
  ```text
  :link 
  ```
  - This is written in the file like `:idea` / `:note`, not run from the commandline.
  - Typing the space after `:link` turns the line into the existing `[` autocomplete/file+heading picker flow.
  - Slate uses that line as the source location.
  - User chooses target file and optional heading.
  - Slate inserts a normal Markdown link at the source line.
  - Slate also writes a reciprocal Markdown backlink into the target file at the chosen heading or file end:
    ```md
    - linked from: [source:50](source.md#L50)
    ```
  - Keep source of truth plain Markdown; no hidden database-only relationships.
- Support unlinked mentions later, but keep it textual and practical.
- No graph view for now; user explicitly does not want graph/canvas PKM UI.
- Link resolver should work across daily logs, inbox, project notes, task sources, and normal Markdown files.
- Daily `source:` entries should become first-class link targets: clicking/activating a source reference should open the original Markdown file and jump to the original line.
- The goal is a useful network of links: agenda items, refile events, captures, daily entries, and notes should be able to reference each other reliably.

### T058 — Slate Flow integration with rtasks, rmenu, Waybar calendar, and thread daemon

Status: in_progress
Scope:
- Future integration goal: tasks written anywhere in Slate appear in Slate agenda and can also appear in rtasks/calendar workflows.
- Current Waybar calendar evidence:
  ```text
  ~/.config/waybar/config.jsonc -> custom/date -> ~/.local/bin/waybar-date-thread
  ~/.local/bin/thread-calendar-gtk-popup -> socket wrapper
  ~/.local/bin/thread-calendar-gtk-popupd -> GTK popup
  calendar events source -> ~/.threadwell/calendar/collections/collection-root/thread/Tareas/*.ics
  thread tasks source -> ~/.threadwell/tasks/tasks.jsonl
  rtasks quick add -> ~/.local/bin/rtasks-quick-add -> rtasks quick-add
  ```
- Calendar popup currently has two task lanes: ICS calendar/habits/medication on the left and actionable Thread tasks from `tasks.jsonl` on the right.
- Integration design decision: do **not** copy Slate Flow tasks into `tasks.jsonl` as the primary sync mechanism. Instead expose a small local Slate Flow API/surface that the calendar can read alongside rtasks, with completion routed back to the original Markdown source.
- First-pass API should be CLI/file based, not a long-running web service:
  ```text
  slate flow-api list --json --days 45 [--scope PATH]
  slate flow-api complete <id>
  slate flow-api open <id>
  slate flow-api rescan [--scope PATH]
  ```
- JSON task shape should be stable enough for GTK/Waybar:
  ```json
  {
    "id": "slate:<scope-hash>:<path>:<line-number>:<title-hash>",
    "source": "slate",
    "title": "Revisar agenda",
    "status": "todo|doing|done",
    "priority": "high|medium|low|null",
    "project": "slate",
    "tags": ["flow"],
    "dueIso": "2026-06-10",
    "scheduledAtIso": "2026-06-09",
    "path": "inbox.md",
    "lineNumber": 4,
    "heading": "Inbox"
  }
  ```
- Field mapping:
  ```text
  Slate title -> calendar/rtasks title
  due: YYYY-MM-DD -> dueIso
  scheduled: YYYY-MM-DD -> scheduledAtIso
  @project -> project
  #tags -> tags
  ALTA/MEDIA/BAJA -> high/medium/low
  [] / [/] / [x] -> todo/doing/done
  path:line -> source reference for open/complete
  ```
- Calendar should aggregate sources, not own them:
  ```text
  ICS events/habits/medication
  + Thread/rtasks tasks.jsonl
  + Slate Flow API tasks
  -> one popup list
  ```
- Completion from calendar/thread daemon should call back into the source owner:
  ```text
  rtasks item -> update tasks.jsonl
  Slate item -> slate flow-api complete <id> -> edit Markdown checkbox
  ICS event -> update ICS STATUS/EXDATE
  ```
- Conflict rule: Slate completion only updates Markdown if the source path and expected task line/snippet still match; otherwise refuse with a clear conflict status and require opening Slate.
- Later pass can add a Unix socket daemon for faster refresh, but the contract should remain local and scriptable.
- Desired dream loop:
  ```text
  write []task in any Slate-touched file
  -> Slate agenda indexes it
  -> Waybar calendar sees it through Slate Flow API
  -> rmenu / Telegram / thread daemon can show or complete it
  -> completion syncs back to the original Markdown checkbox when safe
  ```
- Keep this integration as the bridge between tools while preserving Markdown as Slate's source of truth.
Progress:
- Added first-pass local CLI Flow API:
  ```bash
  slate flow-api list --json --days 45 --scope /home/nd/slate
  slate flow-api complete <id> --scope /home/nd/slate
  slate flow-api open <id> --scope /home/nd/slate
  slate flow-api rescan --scope /home/nd/slate
  ```
- Flow API emits JSON tasks from the derived Slate agenda cache and auto-rescans when the cache is missing.
- Flow API task IDs are generated from scope/path/line/title hash and keep `source: "slate"` so calendar/rmenu/thread can route actions back to Slate.
- `flow-api complete` edits the original Markdown checkbox to `[x]`, then refreshes the derived cache.
- Completion has a conflict guard: it refuses if the cached source snippet no longer matches the Markdown line.
- Updated `thread-calendar-gtk-popupd` to aggregate Slate tasks with Thread/rtasks tasks using:
  ```text
  SLATE_BIN=/home/nd/slate/target/release/slate
  SLATE_FLOW_SCOPE=/home/nd/slate
  ```
- Calendar task completion now routes by source:
  ```text
  source=thread -> tasks.jsonl
  source=slate  -> slate flow-api complete <id>
  ```
- Waybar calendar now treats Slate `#task` as first-class calendar tasks: the GTK popup Tasks lane includes all pending Slate Flow items tagged `#task`, unprojected Slate tasks are grouped under `#task` instead of `Inbox`, dated Slate `#task` items are merged into day/event views, `waybar-date-thread` lights the task dot for today's dated Slate `#task`, and the terminal `thread-calendar-view` includes dated Slate tasks.
- Flow parser now skips fenced code blocks so documentation examples do not flood the calendar/agenda as real tasks.
- Added live Slate-buffer agenda publishing: when a task block is committed from the normal editor with Enter/Shift+Enter/Ctrl+Enter, Slate refreshes the derived Flow agenda cache from the current in-memory buffer even before the file is saved.
- Normal Slate editor now shows the same centered `Agenda updated` toast as Scratch for about 0.4s after task/metadata/checkbox commits.
- This lets Waybar calendar see tasks/checkmarks quickly from Slate without waiting for an explicit save; source Markdown is still authoritative.
- Added bilateral completion requests for calendar -> Slate:
  ```text
  flow-api complete <id>
  -> writes Markdown/cache when disk source still matches
  -> always appends a local completion request under the Slate Flow scope cache
  -> any open Slate editor polls that request and applies `[x]` to the matching in-memory buffer line
  -> Slate republishes the in-memory buffer to agenda/cache and shows `Agenda updated`
  ```
- Reliability rule: external completion no longer overwrites mismatched disk text. If the task is currently dirty/unsaved in Slate, the running Slate window owns the in-memory edit; if Slate is closed and disk does not match, the request remains harmlessly pending instead of corrupting Markdown.
- Added direct Threadwell agenda hook:
  ```bash
  /home/nd/.local/bin/slate-agenda-context
  ```
  It prints live Markdown-ish context from Slate Flow with `Today`, `Dated / upcoming`, and `No-date / inbox` sections, backed by `slate flow-api list --json --days 45 --scope /home/nd/slate`.
- Added hook note for agents:
  ```text
  /home/nd/.threadwell/slate-agenda-hook.md
  ```
  When the user asks about agenda/tareas de hoy/cosas pendientes, Threadwell should run/read this hook first instead of rediscovering what Slate agenda means.
- Promoted the hook direction into local Threadwell core at `/home/nd/threadwell/packages/daemon`: added first-party daemon tools `slate_agenda_list` and `slate_agenda_complete`, included `slate_agenda_list` in readonly daemon policy, included `slate_agenda_complete` in calendar/local mutation policies, and updated the daemon prompt so agenda questions use Slate agenda directly.

### T059 — Slate Flow rich task metadata without bloat

Status: pending
Scope:
- Do not limit Slate Flow to inline-only tasks, but also avoid heavyweight task objects.
- Support a middle-ground task shape: concise inline tasks for normal writing, plus optional nearby metadata lines for richer task detail.
- Keep the canonical lightweight form:
  ```md
  - [ ] Arreglar selección con mouse #editor #input @slate ALTA
    - due: 2026-06-10
    - scheduled: 2026-06-08
    - related: [Periscope](README.md#periscope)
    - note: revisar comportamiento con selección Unicode
  ```
- Consider accepting section-style task notes for larger tasks, inspired by `nb` todos, but only as an optional expanded form:
  ```md
  # [ ] Preparar release @slate ALTA

  ## Due
  2026-06-10

  ## Related
  - [README](README.md)

  ## Notes
  Revisar Periscope y buffers antes.
  ```
- Agenda/indexing should understand both forms where practical, but inline Markdown tasks remain the normal/default writing flow.
- Rich metadata should add information to each task without making documents feel bloated.

### T060 — Slate selector/resolver grammar, nice-to-have

Status: pending
Scope:
- Nice-to-have lane inspired by `nb` selectors, but Slate-native and intentionally cautious because universal selectors can become sensitive/complex.
- Explore a common resolver for paths, titles, recent files, buffers, Lance slots, headings, daily/inbox targets, tags, projects, and future agenda items.
- Potential selector examples to discuss before implementing:
  ```text
  README
  README.md
  src/main.rs
  project:README
  home:notes/todo
  recent:3
  lance:1
  buffer:2
  daily:2026-06-07
  inbox:
  @slate
  #editor
  ```
- Use it only if it improves existing Slate workflows such as `:open`, `:buffer`, `:agenda`, `:insert-link`, `:capture`, `:refile`, and `:backlinks`.
- Do not introduce rigid `nb`-style notebooks or replace Slate's project/home model.

### T061 — Slate search language

Status: pending
Scope:
- Add a Slate-native search/filter language for future notes search, agenda, tags, backlinks, and textual result buffers.
- Start with practical operators inspired by `nb`, adapted to Slate:
  ```text
  foo bar                  # AND
  foo --or bar             # OR
  foo --not target         # NOT
  #editor @slate ALTA      # tag/project/priority filters
  due:today
  due:overdue
  scheduled:this-week
  type:task
  path:src
  ```
- Reuse the language in `:search`, `:agenda`, `:backlinks`, `:mentions`, and maybe Periscope if Periscope becomes formalized beyond an idea.
- Keep Periscope primarily as fast path/file search for now; do not over-formalize it before real usage proves it belongs.
- Results should flow into Slate-native pickers or textual result buffers, not a web UI.

### T062 — Slate Daily navigation and monthly reflection workflow

Status: completed
Scope:
- Keep Slate's preferred monthly daily log design rather than adopting `nb`'s one-file-per-day default.
- Do not add quick append as a second capture surface. Daily should reflect agenda/Flow changes; capture remains `:note`, `:idea`, plain `:`, checkbox tasks, Scratch, and Agenda.
- Add commands/workflows for navigating and maintaining the monthly reflection:
  ```text
  :daily                 # open current monthly daily at today's section
  :daily-log             # alias/open current monthly daily
  :daily yesterday       # jump to previous day's section
  :daily YYYY-MM-DD      # jump to a specific day in the monthly log
  :daily refresh         # rebuild/repair daily entries from Flow cache/history where possible
  ```
- Add a simple previous-day/month picker or command for browsing recent daily entries.
- Daily should remain the chronological backup/trail for agenda changes, completions, captures, and refile history, not a replacement for agenda, inbox, or project notes.
Progress:
- First implementation pass added command catalog/parser support for `:daily`, `:daily-log`, and `:journal` with targets `today`/empty, `yesterday`/`ayer`, `YYYY-MM-DD`, and `refresh`/`rebuild`.
- Command execution now ensures the monthly Daily file exists, opens it in Slate, and jumps the cursor to the requested day heading. Dirty buffers are protected: Slate asks the user to save/discard before opening Daily.
- `:daily refresh` currently opens/ensures today's monthly Daily and is reserved as the UX entry point for deeper rebuild/repair once more Flow history is persisted.
- Daily source links now use standard Markdown link syntax and line fragments, so Daily navigation is already aligned with the future linking/backlink lane.
- Validation after first pass: `cargo fmt --all`, `cargo check`, `cargo test` (120 passed), and `cargo build --release`.

### T063 — Optional sync/checkpoint lane

Status: pending
Scope:
- Future lane only; do not implement until Slate Flow/index/refile behavior is useful locally.
- Explore optional history/checkpoint/sync workflows inspired by `nb`, but Slate should not require Git or remote sync.
- Possible options:
  ```text
  git checkpoints
  rclone sync
  Syncthing-friendly folder layout
  manual export/import
  status/history textual buffers
  ```
- Keep Markdown/local files as source of truth and avoid surprise auto-sync by default.
- Any conflict handling must be explicit, textual, and safe.
- This lane is for user-chosen backup/sync/history, not plugin infrastructure.

### T064 — Main.rs modular refactor

Status: completed
Scope:
- Reduce `src/main.rs` from a 13k+ line mixed app file into focused modules without a disruptive rewrite.
- Keep behavior unchanged while moving code; prefer compileable incremental batches over a big-bang refactor.
- Preserve local-first/plain-text architecture and avoid introducing framework or plugin abstractions.

Plan:
1. Stabilize seams first:
   - add/keep small public methods on `SlateApp` only where modules need to coordinate;
   - avoid changing command names, shortcuts, Flow cache formats, or runtime request files during extraction;
   - run `cargo fmt --all`, `cargo check`, and focused tests after every batch.
2. Create reusable Flow edit/autocomplete module:
   - add `src/flow_edit.rs` for the upcoming reusable Flow item edit mini-modal;
   - centralize `#tag` / `@project` suggestion state and rendering there;
   - reuse it from agenda first, then from normal editor Flow-line typing.
3. Extract Flow runtime plumbing:
   - add `src/flow_runtime.rs` for active-buffer heartbeat, open requests, completion requests, organization requests, and runtime path helpers;
   - keep request file formats compatible:
     `active-buffers.tsv`, `open-requests.tsv`, `completion-requests.tsv`, `organization-requests.tsv`.
4. Extract agenda UI/actions:
   - add `src/agenda.rs` for agenda tabs, row rendering, selection movement, open/cycle/tag/project actions, and standalone agenda behavior;
   - keep `Enter`, `Ctrl+Enter`, `Shift+Tab`, `#`, `@`, and `Esc` behavior unchanged.
5. Extract scratch/capture:
   - add `src/scratch.rs` for scratch modal, scratch archive, quick capture, and scratch standalone close behavior.
6. Extract command surfaces:
   - add `src/commands.rs` for command definitions, command palette metadata, command execution, command history/suggestions, and command-line dispatch.
7. Extract picker/dialog families only after agenda is stable:
   - `src/file_dialogs.rs` for open/save/save-as/recent;
   - `src/periscope.rs` for project/global file search;
   - `src/link_ui.rs` for link assist, heading picker, and link preview.
8. Leave `src/main.rs` as orchestration:
   - app bootstrap;
   - `SlateApp` construction/wiring if still needed;
   - `eframe::App` update flow;
   - cross-module state fields until a later, smaller state split is obviously safe.
9. Optional final cleanup:
   - split `SlateApp` state into nested structs only when module boundaries are stable, e.g. `AgendaState`, `ScratchState`, `PickerState`;
   - update README/SLATE_FLOW docs only for user-visible behavior, not internal file movement.

Suggested batch order:
- Batch A: create `flow_edit.rs` with the new mini-modal/autocomplete; no existing behavior removed. In progress: extracted reusable Flow organization mode, suggestion filtering, token creation, token append helper, and `FlowItemEditState` into `src/flow_edit.rs`; agenda now uses this for `#`/`@` autocomplete and the `Shift+Enter` edit mini-modal.
- Batch B: move runtime request helpers to `flow_runtime.rs`. In progress: added `src/flow_runtime.rs`; moved reusable runtime path/request append helpers for completion, organization, edit, and open requests; moved active-buffer heartbeat/active-buffer lookup helpers; moved shared relative path formatting; moved request parsing/draining for open, completion, organization, and edit requests. `main.rs` still owns applying requests because that mutates editor/UI state.
- Batch C: move agenda rendering/actions to `agenda.rs`. Completed first extraction pass: added `src/agenda.rs`; moved core agenda types (`AgendaItem`, `AgendaFocus`, `AgendaTab`, `AgendaDatedRow`), cache loading/cache-row parsing, Flow-task-to-agenda-item conversion, live-buffer overlay, agenda sorting, week/date row helpers, selection clamp/move/selected-index helpers, task-line/state-marker formatting, row rendering helpers, and pure visibility/filter/tab matching helpers out of `main.rs`. Remaining agenda code in `main.rs` is mostly app-state orchestration and editor/standalone side effects.
- Batch D: move scratch/capture to `scratch.rs`. Completed first extraction pass after Batch C compiled and tests passed: added `src/scratch.rs`; moved `ScratchEntry`, scratch archive path/append helper, quickadd normalization/splitting, capture-entry formatting, scratch archive parse/serialize, scratch feedback toast rendering, and focused scratch module tests. Remaining scratch code in `main.rs` is app-state orchestration, editor-buffer interaction, and modal wiring.
- Batch E: move command palette/command line to `commands.rs`. Completed: added `src/commands.rs`; moved `Command`, `CommandSpec`, `COMMAND_SPECS`, command-name/canonical-alias lookup, command prefix parsing, fuzzy command scoring, best completion token selection, ranked command suggestion helpers, pure `CommandLineAction` / `CommandLineNumberMode` parsing for `:command args`, command history limit/trim/push helpers, and command usage parse/merge/record/ranking/boost helpers with focused module tests. `main.rs` now keeps only command execution and app-state/UI side effects. Validated with `cargo fmt --all`, `cargo check`, `cargo test` (107 passed), and `cargo build --release`.
- Batch F: move pickers/link/periscope modules. Completed: added `src/file_dialogs.rs`, `src/link_ui.rs`, and `src/periscope.rs`; moved file picker modes, picker/ranking/window helpers, file metadata labels, directory scanning skip policy, link assist choice/heading options, Markdown link target/path/heading-anchor helpers, web-target detection, Periscope modes/index rows, project/home indexing, fuzzy path scoring, cached result ranking, folder grouping, row selection, and focused tests. `main.rs` keeps picker/link/periscope UI orchestration, modal wiring, editor-buffer mutations, and dirty/open side effects. Validated with `cargo fmt --all`, `cargo check`, `cargo test` (113 passed), and `cargo build --release`.
- Completion note: first-stage modular refactor is done. Flow edit/autocomplete polish was validated manually, including cursor-aware `#tag` / `@project` replacement and normal-editor vertical suggestions. Optional future cleanup remains separate work: nested state structs such as `AgendaState`, `ScratchState`, `PickerState`, and module docs/manual UI checklist.

Validation:
- After every batch: `cargo fmt --all && cargo check`.
- After behavior-moving batches: `cargo test` plus focused Flow/agenda/editor buffer tests.
- After UI/shortcut-moving batches: manually verify agenda, standalone agenda, scratch, commandline, and editor navigation.

Command groups worth considering:

Editing lines:
- `:delete-line` / `:dl` — delete current line.
- `:duplicate-line` / `:dup` — duplicate current line below.
- `:move-line-up` / `:mlu` — move current line up one line; shortcuts: `Alt+Up`, plus Alt structural up key for the selected movement mode.
- `:move-line-down` / `:mld` — move current line down one line; shortcuts: `Alt+Down`, plus Alt structural down key for the selected movement mode.
- `:move-line-to-paragraph-start` / `:mlps` — move current line to the start of its paragraph; shortcut: repeated Alt structural up key.
- `:move-line-to-paragraph-end` / `:mlpe` — move current line to the end of its paragraph; shortcut: repeated Alt structural down key.
- `:join-line` / `:join` — join current line with next line.
- `:sort-lines` — sort selected lines or current paragraph.
- `:trim-line` — trim trailing whitespace on current line.
- `:trim-buffer` — trim trailing whitespace in the whole file.

Editing words/selections:
- `:select-word` / `:sw` — select the word under/near the cursor; Ctrl-layer `sw`.
- `:select-line` / `:sl` — select the current line; Ctrl-layer `sl`.
- `:delete-word` / `:dw` — delete word under/near cursor; Ctrl-layer `dw`.
- Alt structural left key extends selection by one word to the left; repeated taps extend by more words.
- Alt structural right key extends selection by one word to the right; repeated taps extend by more words.
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
- `:grep` / `:search-files` — future project/text search.

Markdown and notes:
- Checkbox parsing/rendering should support `[ ]`, `[/]`, and `[x]` as Markdown checkboxes, not Slate task objects.
- `:heading 1..6` — convert current line to Markdown heading level.
- `:promote-heading` / `:demote-heading` — adjust Markdown heading level.
- `:insert-link` — insert Markdown link around selection or at cursor.
- `:insert-wikilink` — insert `[[...]]` link later integrated with the link resolver.
- `:format-table` — eventually align simple Markdown tables if practical.

Files and buffers:
- Already implemented basics: `:w`, `:q`, `:wq`, `:open`, `:new`.
- `:recent` — open recent files picker/result list.
- `:reopen` — reopen current file from disk after confirmation.
- `:rename-file` — rename current file and update path.
- `:copy-path` — copy full path or project-relative path.
- `:buffer-next` / `:bn` and `:buffer-prev` / `:bp` — future multi-buffer navigation.
- `:buffer-close` / `:bd` — close current buffer when buffers exist.

Capture and knowledge workflow:
- `:scratch` — open scratch buffer.
- `:capture` — append selection/current line to scratch.
- `:new-note` — create a related Markdown note/file.
- `:backlinks` — show backlinks for current note once link scanning exists.

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

### Three-batch Alt structural editing plan

Batch 1 — Vertical line movement: done.
- Done: add `EditorBuffer` primitives to move the current line up/down one line.
- Done: add paragraph-boundary primitives to move the current line to the start/end of its paragraph, where a paragraph is a contiguous block of non-empty lines separated by blank lines.
- Done: add commandline commands: `:move-line-up` / `:mlu`, `:move-line-down` / `:mld`, `:move-line-to-paragraph-start` / `:mlps`, `:move-line-to-paragraph-end` / `:mlpe`.
- Done: add shortcuts: `Alt+Up` and `Alt+Down` for one-line movement.
- Done: add Alt structural mode mappings:
  - Slate mode: `Alt+i` move line up, repeated `Alt+ii` move line to paragraph start; `Alt+k` move line down, repeated `Alt+kk` move line to paragraph end.
  - Vim mode: `Alt+k` move line up, repeated `Alt+kk` move line to paragraph start; `Alt+j` move line down, repeated `Alt+jj` move line to paragraph end.
- Done: show the pending Alt sequence in the minibuffer as `alt:i`, `alt:ii`, etc.
- Done: keep this current-line only; selected-line ranges are a future extension.

Batch 2 — Horizontal word selection: done.
- Done: add `EditorBuffer` primitives to select/extend by words to the left and right.
- Done: Alt structural left key selects the word under/near the cursor and leaves the cursor at the left edge; repeated taps extend selection one more word left each time.
- Done: Alt structural right key selects the word under/near the cursor and leaves the cursor at the right edge; repeated taps extend selection one more word right each time.
- Done: Slate mode: `Alt+j` extends word selection left; `Alt+l` extends word selection right.
- Done: Vim mode: `Alt+h` extends word selection left; `Alt+l` extends word selection right.
- Done: preserve normal typing and avoid permanent modes.

Batch 3 — Duplicate line and polish: done.
- Done: add `EditorBuffer::duplicate_current_line()` with tests.
- Done: add `:duplicate-line` / `:dup`.
- Done: keep duplicate line out of Alt; expose it through commandline and Ctrl-layer `dup` so Alt stays structural movement/selection.
- Done: update shortcut help, README, and queue docs with the final Alt structural layer.
- Done: add regression tests for line movement at first/last line, paragraph boundaries, Unicode lines, selection extension, and duplicate-line behavior.

Follow-up — Duplicate placement transient mode: done.
- Done: keep simple `:dup` stable.
- Done: add transient duplicate-placement mode as `:duplicate-place` / `:dupp`.
- Done: duplicate current line, mark the duplicate as active, move it up/down while the transient mode is active, then confirm with `Enter`/`Space` or cancel with `Esc`.
- Done: reuse Slate/Vim `Ctrl+Shift` movement mapping plus arrow movement while placement is active.
- Done: show status/minibuffer help such as `duplicate placement: move · Enter/Space place · Esc cancel`.

## Suggested implementation order

### Completed foundation

1. T001 — Review generated context.
2. T039 — Native editor architecture: leave `egui::TextEdit` behind for the main document.
3. T040 — EditorBuffer text model with tests.
4. T041 — Native EditorView renderer for visible lines.
5. T042 — Native editor input landling for minimum viable editing.
6. T043 — Native selection, cursor jump, and scroll-to-position.
7. T044 — Search/find on native editor primitives, using the command line as minibuffer.
8. T033 — Ctrl-hold command layer and Ctrl+Shift live movement.
9. T016 — Richer statusbar/minibuffer layout.

### Next practical sequence

1. T007/T046 — Alt structural editing, batch 1: move current line up/down and to paragraph start/end. Done.
2. T007/T046 — Alt structural editing, batch 2: extend word selection left/right. Done.
3. T007/T046 — Alt structural editing, batch 3: duplicate line and shortcut/help/docs polish. Done.
4. T046 — Duplicate placement transient mode: duplicate a line into a movable pending placement, confirm with Enter/Space, cancel with Esc. Done.
5. T002/T013 — Improve command discovery: fuzzy command matching, palette/command-line consistency, and recent/frequent commands. Done for T002; future command additions continue under T013.
6. T003 — Expand persistent config for wrap/preview/theme/scratch behavior while keeping the plain config simple.
7. T004 — Build real recent-files list and `:recent` picker.
8. T010 — Minimal project file opener with fuzzy-ish file matching plus lightweight size/modified metadata. Done.
9. T047 — Replace native Open/Save/SaveAs dialogs with Slate-owned modals. Done.
10. T017 — Write down Slate's knowledge-work philosophy so future features do not drift into Obsidian/Emacs sprawl. Done.
11. T005 — Scratch buffer and quick capture workflow.
15. T020 — Global notes search, starting simple and later backed by lightweight scanning/cache only if needed.
16. T024 — Link resolver trigger for `[[`.
17. T025 — Link resolver ranking/result groups.
18. T027 — Wiki-link parser and target resolver.
19. T026 — Content-match deep link insertion with compact/full style toggle.
20. T028 — Follow-link navigation and cursor jump.
21. T029 — Link display ergonomics and visual affordances.
22. T031 — Link resolver performance and indexing lifecycle.
23. T030 — Backlinks and most-linked notes.
24. T021 — Progressive organization commands.
26. T022 — Tags and lightweight metadata.
27. T035 — Append/capture side effects from normal editing.
28. T036 — Textual result buffers.
29. T050–T059 — Slate Flow agenda, rich task metadata, SQLite cache, readable `.slate` index, daily/refile/link workflows.
30. T060 — Nice-to-have universal selector/resolver grammar; discuss carefully before implementing.
31. T061 — Slate search language for notes, agenda, tags, backlinks, and maybe future Periscope formalization.
32. T062 — Slate Daily quick append and previous-day workflow.
33. T063 — Optional future sync/checkpoint lane with git/rclone/Syncthing possibilities.
34. T009 — Theme system.
35. T008 — Lightweight Markdown preview improvements.
36. T011 — Buffers / multi-file workflow.
37. T012 — Optional auto-save.
38. T034 — Repeatable edits and lightweight macros.
39. T037 — Curated defaults instead of infinite configurability.
40. T038 — Literate Markdown configuration polish.

<!-- THREADSUITE:START -->
# ACTIVE_QUEUE.md

## Queue

### T001 — Review ThreadSuite generated context

Status: completed
Scope:
- Review inferred Rust/Cargo project context and confirm or edit generated facts.
<!-- THREADSUITE:END -->
