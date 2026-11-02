# Module API

How optional modules (Flow, Agenda, Daily, references, …) plug into the
core. The cut points it must cover are listed in `src/modules/README.md`.

## Status (2026-11-02)

Implemented: the `Module` trait, the host (`src/module_api.rs`), `Cx` with
status / open / result list / `wake_after` / waker / data folder / vaults,
the document hooks (opened, edited with the changed line range, idle after
a pause in typing, saved, closed), module commands in the palette, command line and help, the core
result list (`src/results_ui.rs`), vaults (`src/vaults.rs`), Settings →
Modules & vaults (on/off per module, `module.<id>` and `vault` keys in
`config.toml`), slow hooks (≥ 4 ms) in the performance log.

Connected modules: **References** (`:backlinks`, `:mentions`) and **Flow**
(the task index of declared vaults; `:flow` lists the open tasks,
`:flow-reindex` rebuilds it; see `src/modules/flow/module.rs`).

Not yet: panels, editor extensions (`decorate`, `complete`, `on_enter`),
module settings rows, the windows channel for modules. They come with the
modules that need them (Agenda, Flow's editing aids).

## Rules

1. **The core never knows a module.** No `if agenda_open` in the editor, no
   Flow field in `SlateApp`. The core calls hooks on a list of modules.
2. **Nothing per frame by default.** A module runs when an event concerns
   it (document opened/saved/edited, its command, its panel open, a message
   for it) or when it asked to be woken (`cx.wake_after`, a background
   thread's waker). Idle modules cost zero.
3. **Heavy work off the UI thread.** Hooks return fast; scans, SQLite and
   disk writes go to the module's own worker. The core times every hook
   (performance log, F12 HUD) so a slow module is visible.
4. **Markdown is the source of truth.** Module data lives in
   `%APPDATA%\slate\modules\<id>\`, never next to the user's files.
5. **Off means off.** A disabled module is not constructed: no thread, no
   file, no key binding, no command.

## Core concepts modules use

- **Vaults.** Folders the user declares as note collections (Settings →
  Vaults; `.git` roots as fallback). One answer to "which notes belong
  together", shared by Flow, Find note, Periscope and backlinks — instead
  of today's three different notions of "project".
- **Documents.** `DocumentRef { id, path, revision, text }` for the open
  documents (all tabs). Edits carry the changed line range (the buffer
  already tracks it), so a module can reparse only what changed.
- **Windows channel.** Each window's loopback listener (used today for
  dragging tabs between windows) carries `MODULE <id> <payload>` messages,
  so a module in one window can ask the window holding a document to change
  it. This replaces Flow's TSV request files and its heartbeat file.

## The hooks

```rust
trait Module {
    fn id(&self) -> &'static str;

    // Lifecycle and documents
    fn start(&mut self, cx: &mut Cx) {}
    fn document_opened(&mut self, cx: &mut Cx, doc: DocumentRef) {}
    fn document_edited(&mut self, cx: &mut Cx, doc: DocumentRef, lines: Range<usize>) {}
    fn document_idle(&mut self, cx: &mut Cx, doc: DocumentRef) {} // 800 ms after the last edit
    fn document_saved(&mut self, cx: &mut Cx, doc: DocumentRef) {}
    fn document_closed(&mut self, cx: &mut Cx, id: DocumentId) {}
    fn wake(&mut self, cx: &mut Cx) {}            // when it asked to be woken

    // Commands (registered in the palette, command line, help)
    fn commands(&self) -> &[CommandSpec] { &[] }
    fn run(&mut self, cx: &mut Cx, command: &str, args: &str) {}

    // UI: a panel while it is open; it gets the keyboard first
    fn panel(&mut self, cx: &mut Cx, ui: &egui::Context) -> PanelState { PanelState::Closed }

    // Editor extensions (all optional, all cheap and cached by the core)
    fn decorate(&self, line: &str) -> Option<LineDecor> { None }   // colours, gutter mark
    fn complete(&self, cx: &Cx, token: &str) -> Vec<Completion> { vec![] } // after # or @
    fn on_enter(&mut self, cx: &mut Cx, line: &str) -> Option<Edit> { None } // e.g. continue a task

    // Settings (rows in their own Settings section; keys `<id>.<name>`)
    fn settings(&self) -> &[SettingSpec] { &[] }

    // Messages from other windows
    fn message(&mut self, cx: &mut Cx, payload: &str) {}
}
```

`Cx` is the module's door into the core: open a file at a line or
heading, read/replace a line of an open document (undoable, marked
unsaved), status messages and toasts, `wake_after`, its data folder, the
vaults, the windows channel, a background job runner.

## How the old features map onto it

| Feature | Hooks |
|---|---|
| Flow index | declared vaults only; `document_opened` → incremental vault scan on its thread (size/mtime, at most once a minute); `document_idle` → the note's unsaved items shadow the index in memory; `document_saved` → the note is reindexed. Item identity by content (path, kind, title, repeat) |
| Agenda | `commands` (`:agenda`), `panel`, opens items with `cx.open_at(path, line)`, edits via `cx.replace_line` or the windows channel |
| Daily | `commands` (`:daily`), events from Flow on `document_saved` |
| Quickadd / inbox | `commands`; Scratch gets a generic "send to" hook later |
| Refile | `commands`, a picker from the core |
| Backlinks / mentions | `commands`, background search over the vault, results in a core picker |
| Task editing aids | `decorate`, `complete`, `on_enter` |

## Order

1. Core: the `Module` trait and registry, `Cx`, Settings → Modules
   (on/off) and Vaults, hook timing in the performance log.
2. **Backlinks/mentions first**: small, but it exercises commands, a
   background job, vaults and a result list. It proves the API.
3. Flow index (no UI), then Agenda on top of it, then Daily, Quickadd,
   Refile, and the editing aids.
