# Slate Flow

Slate Flow is Slate's local-first knowledge-work lane: capture, tasks, agenda, daily log, refile, links, backlinks, and search over normal Markdown files.

Slate Flow should make Slate feel like:

> Write normally, capture instantly, let Slate surface what matters, and organize progressively without leaving plain text.

It is inspired by Org/Agenda/Roam-style workflows and useful ideas from tools like `nb`, but it should remain Slate-native: editor-first, commandline-first, textual, local, and minimal.

## Non-negotiable principles

1. **Markdown is the source of truth.**
   Tasks, notes, links, captures, daily logs, and refiles live in plain files. Any SQLite database, `.slate` index, search cache, or agenda cache is derived and rebuildable.

2. **Slate remains a normal editor first.**
   Flow features must not make typing, opening, saving, selecting, or navigating files weird.

3. **No mandatory vault.**
   Slate may use project roots, home roots, inbox files, daily files, and a readable `.slate` index, but it must remain useful on any normal Markdown file.

4. **Tasks can live anywhere Slate touches.**
   A task in `README.md`, `ACTIVE_QUEUE.md`, a daily log, an inbox, or a project note can appear in the agenda once Slate knows about that file.

5. **Commandline-first, textual UI.**
   `:agenda`, `:capture`, `:daily`, `:refile`, `:backlinks`, and `:search` should use Slate commands, pickers, modals, and textual result buffers. No permanent sidebars and no web UI.

6. **Progressive organization.**
   Capture first. Add metadata if useful. Refile later. Link later. Search later. Structure should help writing, not block it.

7. **No graph-first PKM.**
   Backlinks, mentions, and reliable navigation matter. Canvas/graph views are not part of the current direction.

8. **Integrations are downstream.**
   `rtasks`, `rmenu`, Telegram/thread-daemon workflows, Git, rclone, and Syncthing can integrate later, but they must not become Slate's source of truth.

## What Slate Flow is not

Slate Flow is not:

- an Obsidian clone
- a Logseq clone
- an Emacs/Org clone
- a web UI
- a plugin system
- a mandatory notebook/vault format
- a project-management database
- a graph UI
- a sync service

## Core file model

Slate Flow has three layers:

```text
Markdown files       source of truth
.slate index         readable/rebuildable project-local knowledge map
SQLite cache         fast derived agenda/search/index data
```

### Markdown files

Markdown files remain authoritative. Flow reads and writes normal Markdown syntax where possible.

Examples:

```md
- [ ] Arreglar selección con mouse #editor #input @slate ALTA
  - due: 2026-06-10
  - scheduled: 2026-06-08
  - related: [Periscope](README.md#periscope)
  - note: revisar Unicode
```

### Readable `.slate` index

A project may have a readable index such as:

```text
.slate/index.toml
.slate/index.md
```

This index documents files Slate has touched or believes contain useful structured information.

It can track hints like:

```text
path
last_seen
has_tasks
has_links
has_tags
task_count
link_count
project/root metadata
```

The `.slate` index is **not** the source of truth. It is an auditable, rebuildable map that helps scope agenda/search/backlink work.

### SQLite cache

A scope-local SQLite cache, for example:

```text
~/.local/share/slate/flow/scope-<hash>/agenda.db
```

stores fast derived data for the current project/folder:

```text
path
line/byte range
task text
checkbox state
due date
scheduled date
tags
@project
priority
parent heading/document context
related links/source references
mtime/hash/revision
source snippet
```

If the database is deleted, Slate should be able to rebuild it from Markdown plus the readable `.slate` index or from known/touched files.

## Task grammar

Slate Flow should support lightweight Markdown tasks by default:

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

Nearby child metadata:

```md
- [ ] Task text #editor @slate ALTA
  - due: 2026-06-10
  - scheduled: 2026-06-08
  - related: [README](README.md)
  - note: revisar comportamiento con Unicode
```

Checkbox metadata lines may also be accepted for compatibility, but Slate should treat known metadata keys as metadata for the parent task rather than independent agenda tasks by default:

```md
- [ ] Task text #editor @slate ALTA
  - [ ] due: 2026-06-10
  - [ ] scheduled: 2026-06-08
```

For larger tasks, Slate may optionally understand section-style task notes:

```md
# [ ] Preparar release @slate ALTA

## Due
2026-06-10

## Related
- [README](README.md)

## Notes
Revisar Periscope y buffers antes.
```

The default should remain inline task + nearby metadata. Section-style task notes are optional, not required.

## Agenda

`:agenda` is the main task view over Markdown.

It should group tasks into useful sections such as:

```text
Today
Scheduled
Due soon
Overdue
Inbox / no date
By project
```

Rows should be compact but contextual:

```text
[ ] Arreglar mouse selection   @slate ALTA   due today   src/editor_view.rs:220
```

Core interactions:

```text
Enter        jump to source file/line
Ctrl+Enter   cycle source checkbox state
/ or command  filter/search within agenda
Esc          close agenda/result buffer
```

Agenda is a view, not storage. Editing or completing an agenda item updates the original Markdown file when possible.

## Capture, inbox, and quickadd

Slate Flow should extend existing scratch/capture rather than compete with it.

Commands to consider:

```text
:capture task
:capture idea
:capture note
:capture link
:quickadd
```

Destinations:

```text
inbox
current file
daily log
choose file
project note
```

The inbox should be a plain Markdown file, likely configurable, with a sensible default such as:

```text
~/.local/share/slate/inbox.md
```

Captured tasks should use the task grammar and appear in `:agenda` after indexing.

## Daily log

Daily is the chronological trail, not the agenda.

Prefer monthly daily-log files over one tiny file per day:

```text
~/.local/share/slate/daily/2026-06.md
```

Candidate format:

```md
# Slate Daily Log — 2026-06

## 2026-06-08

- 09:30 idea: mejorar agenda
- 09:42 task: [Arreglar selección](../../slate/ACTIVE_QUEUE.md)
- 10:12 refile: "Periscope thoughts" -> [periscope.md](...)
```

Commands:

```text
:daily                 open today's daily section/file
:daily "texto"         append timestamped entry
:daily-log "texto"     append timestamped entry
:quickappend daily ... append timestamped entry
```

Daily should preserve history. Refile should be able to leave links/references in daily rather than erasing chronology.

## Refile

`:refile` organizes selected text, the current task, current line, or capture entries.

Destinations:

```text
inbox
daily log
current file heading
another heading in current file
chosen file
chosen file heading
project note
```

First-pass behavior should prefer non-destructive or link-preserving refile:

```text
source daily/inbox entry -> destination note/task
destination note/task -> original source/context
```

Example source after refile:

```md
- 10:12 idea: Periscope should stay Project/Home
  - refiled: [docs/periscope.md#ideas](docs/periscope.md#ideas)
```

Example destination:

```md
## Ideas

- Periscope should stay Project/Home
  - source: [Daily 2026-06-08](../daily/2026-06.md#2026-06-08)
```

Future modes can support move/delete/archive, but the first version should avoid losing context.

## Links and backlinks

Slate Flow keeps standard Markdown links as final document syntax.

`[[` remains an assist trigger, not persistent wiki-link syntax.

Commands to consider:

```text
:backlinks
:mentions
:find-note
:notes
:insert-note-link
```

Backlinks should be textual result buffers or pickers, not a graph UI.

## Search language

A future Slate search/filter language should work across notes search, agenda, backlinks, mentions, and maybe Periscope if Periscope becomes formalized beyond file search.

Initial operators:

```text
foo bar                  AND
foo --or bar             OR
foo --not target         NOT
#editor @slate ALTA      tag/project/priority filters
due:today
due:overdue
scheduled:this-week
type:task
path:src
```

This should feed pickers or textual result buffers, not a web UI.

## Universal selector/resolver

A universal selector is a nice-to-have, not an early requirement. It is powerful but sensitive because it can become ambiguous or over-complex.

Possible future selectors:

```text
README
README.md
src/main.rs
project:README
home:notes/todo
recent:3
lance:1
buffer:2
daily:2026-06-08
inbox:
@slate
#editor
```

Use it only if it improves existing workflows like `:open`, `:buffer`, `:agenda`, `:insert-link`, `:capture`, `:refile`, and `:backlinks`.

## External integrations

Future integrations may include:

```text
rtasks
rmenu
Telegram/thread daemon
git checkpoints
rclone
Syncthing-friendly folder layouts
```

These are optional downstream lanes. Slate Markdown remains the source of truth.

A future dream loop:

```text
write TODO in any Slate-touched file
-> Slate agenda indexes it
-> rtasks/calendar sees it
-> rmenu / Telegram / thread daemon can show or complete it
-> completion syncs back to original Markdown checkbox when safe
```

## Implementation order

Recommended order:

1. **T051/T059 — task parser and metadata grammar**
   - Parse Markdown checkbox tasks.
   - Parse `#tags`, `@project`, `ALTA/MEDIA/BAJA`.
   - Parse nearby `due:`, `scheduled:`, `related:`, and `note:` metadata.

2. **T052 — minimal index/cache**
   - Track known/touched files.
   - Add readable `.slate` index shape.
   - Add SQLite cache after parser shape is stable.

3. **T053 — basic agenda**
   - Show tasks grouped by due/scheduled/no-date.
   - Jump to source.
   - Cycle checkbox state from agenda.

4. **T054/T055/T062 — capture, inbox, daily, quickappend**
   - Extend scratch/capture.
   - Add daily quick append.
   - Ensure captured tasks are indexed.

5. **T056 — refile with bidirectional links**
   - Move/copy/link selected text or current task.
   - Preserve source context.

6. **T057 — backlinks and mentions**
   - Textual backlink/result buffers over Markdown links.

7. **T061 — search language**
   - Reuse across agenda/search/backlinks.

8. **T060 — universal selector/resolver**
   - Discuss carefully and add only if it clarifies workflows.

9. **T058/T063 — external integrations and sync/checkpoints**
   - Future only, after local Flow works well.

## First implementation slice

The first real implementation should be small:

```text
Task parser only, no UI first.
```

Deliverables:

```text
src/flow.rs or src/flow/task_parser.rs
unit tests for task parsing
parse state, text, tags, project, priority
parse child metadata due/scheduled/related/note
return source line and context hints
```

After that, Slate can build a tiny in-memory agenda before committing to SQLite/index details.
