# Slate modules

Slate is a core editor plus optional modules. The core (editor, tabs,
windows, files, preview, search, pickers, Periscope, Find note, Link assist,
Scratch, settings) must work on its own. Modules add features on top and
reach the core only through its hooks.

On 2026-11-01 the knowledge-work features were cut out of the core, to be
reconnected one at a time through hooks, each with a short design first.
The complete code before the cut is at the git tag **`pre-module-cut`**.

## What is here

| Path | State | What |
|---|---|---|
| `flow/` | compiled, tests run, **not connected** | Task grammar parser, SQLite cache, batch indexing, background writer, request files, `index.tsv` export, organization helpers |
| `agenda/` | compiled, **not connected** | Agenda model: items from the cache, filters, week rows, live-buffer overlay |
| `daily/` | compiled, tests run, **not connected** | Monthly daily log: day sections in date order, events |
| `references/` | **connected** | `:backlinks` / `:mentions` over the file's vault, results in the core's result list |
| `legacy/` | **not compiled** (reference only) | The glue that wired them into the editor: `SlateApp` methods, dialogs, key handling, editor aids, CLI |

`legacy/` holds the old code verbatim, grouped by feature. It refers to
fields and methods the core no longer has; it is there to read while
rebuilding, not to be switched back on.

## What the core no longer does

- **Flow indexing.** Opening a file no longer scans its folder; saving and
  typing no longer update a Flow index; no `.slate/index.tsv` is written.
- **Commands:** `:agenda`, `:daily`, `:quickadd`/`:qa`/`:inbox`,
  `:refile`, `:flow-rescan`, `:flow-rebuild`, `:backlinks`, `:mentions`.
- **CLI:** `slate --agenda` (standalone agenda window), `slate flow-api`.
- **Editor aids for the task grammar:** `#task`/`#note`… added on Enter,
  `due:`/`scheduled:` helpers and date suggestions (Shift+Enter is now a
  plain line break), `#`/`@` autocomplete, `#tag`/`@project` colouring in
  task lines, `:note`/`:idea` markers and gutter bars.
- **Scratch:** `:qa` and checkbox lines are archived as text like the rest
  (they no longer become inbox tasks).
- **Settings:** the "Virtual backlinks/mentions" row (that feature was
  never connected: the editor never received the references section).

Module settings in `config.toml` (for example `virtual_references_*`) are
kept: the core writes back every line it does not know.

## Where the core used to call them (the cut points)

These are the places a hook has to cover when a module comes back.

| Core moment | Was calling | Module |
|---|---|---|
| Every frame (`ui_frame`) | heartbeat file, 4 request-file polls, background scan results, writer results, debounced agenda sync | Flow |
| Every frame | agenda dialog, Flow error dialog, inline `#`/`@` suggestion popup | Agenda, Flow |
| File opened | background scan of the whole scope (+ live index of the file) | Flow |
| File saved (foreground and background) | reindex the file | Flow |
| Text edited near a task | schedule agenda sync (`take_flow_agenda_update_request`) | Flow |
| Text typed | inline `#`/`@` autocomplete state | Flow |
| Editor keys: Enter, Shift+Enter, Tab, Backspace/Delete, ↑↓ | default tags, metadata lines, date suggestion, sync requests | Flow |
| Editor painting of a line | `:note`/`:idea` markers, tag colouring, metadata date hint | Flow |
| Keyboard routing (`shortcuts`) | agenda keys, org autocomplete keys, Esc closes agenda | Agenda |
| Commands and command line | the commands listed above | all |
| Open destinations | `Daily { date }`, `Agenda(line)` jump after loading | Daily, Agenda |
| File picker / heading picker modes | refile target and heading | Refile |
| Pending actions (unsaved prompt) | open Daily | Daily |
| Tabs | unsaved tasks of inactive tabs shown in the agenda | Agenda |
| Recovery | agenda sync after recovering text | Flow |
| Scratch archive | `:qa`/checkbox lines sent to the inbox | Quickadd |
| Settings load/save, settings dialog | virtual references keys and row | References |
| `main()` | `flow-api` subcommand, `--agenda` window, export flush on exit | Flow, Agenda |
| Flow worker → daily log | captures/completions logged on agenda sync | Flow → Daily |

## Bringing a module back

1. Read its `legacy/` code and the audit notes; write a short design
   (what it does, which hooks, what runs where, what it stores).
2. Add or extend the core hooks it needs (generic, never module-specific).
3. Connect the module through them; move what it reuses out of `legacy/`.
4. Tests, measurements, docs; then it is on.
