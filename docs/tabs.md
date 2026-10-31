# Tabs — plan

Agreed with the user on 2026-10-31.

## Keys and mouse

| Input | Action |
|---|---|
| `Ctrl+T` | New tab |
| `Ctrl+W` | Close tab (asks first when it has unsaved changes); closing the last tab leaves one empty tab |
| `Ctrl+Tab` / `Ctrl+Shift+Tab` | Next / previous tab (`Shift+Tab` keeps dedenting) |
| `Ctrl+wheel` | Next (up) / previous (down) tab |
| `Ctrl+1` … `Ctrl+9` | Go to tab N (`Alt+1`…`Alt+0` stay Lance's quick switch) |
| `Ctrl+Shift+T` | Reopen the last closed tab |
| `Ctrl+Q` | Close (hide) the window; tabs are kept |
| `:quit-all`, tray "Quit Slate" | Quit for real (asks for each unsaved tab) |
| Click / middle click / `×` | Activate / close / close |
| Drag a tab | Reorder; dropped outside the window: opens it in a new Slate window |
| Double click a tab | Rename it: an alias shown only inside Slate |

## Behaviour

- Opening a file that is already open activates its tab. Opening a file from
  anywhere (pickers, Periscope, `slate file.md`, links) opens a tab; an empty,
  unchanged untitled tab is reused.
- Each tab keeps its text, undo/redo, cursor, scroll, selection, search and
  crash-recovery checkpoint.
- Unsaved tabs show `●`; `×` (or a middle click) closes a tab and `+` opens one.
- Equal file names show their folder (`README.md · notas`).
- Hiding the window keeps the tabs. Starting Slate without a file restores the
  last session's tabs (replaces "reopen last file").
- Aliases are stored per absolute path; Periscope matches them and shows
  `alias · real-name.md`. Renaming/moving a file outside Slate drops its alias.

## Design

`SlateApp` keeps working on "the active document" (its existing fields), so the
10k lines that use `self.buffer`, `self.path`, … stay unchanged. Inactive tabs
store a `DocumentState` (buffer, view, path, dirty, disk base, preview sync,
search, …); switching swaps them. Crash recovery gets one slot per tab
(`Slot::Tab(id)`) and switching uses the existing `swap_roles`, so every
document keeps its own durable checkpoint.

## Stages

1. Core (done 2026-10-31): tab strip in the title bar, keys above, open-in-tab and dedupe,
   per-tab state and recovery, close/quit flows, session restore, forwarding
   from `slate file.md` into a tab.
2. Comfort (done 2026-10-31): drag to reorder, `Ctrl+Shift+T`, name
   disambiguation, scrolling strip (tabs never narrower than 72 pt), path
   tooltips. External changes to an inactive tab's file are flagged when the
   tab is shown again (switching re-checks the file at once), and the agenda
   overlays unsaved tasks from every tab.
3. Aliases (done 2026-10-31): double click a tab (or `:alias NAME`) to
   rename it inline; `aliases.json` next to the settings; shown in tabs,
   the window title, Lance and recent files; Periscope matches every word
   of the query against aliases and lists those files first. App shortcuts
   pause while an alias is being typed. An untitled tab keeps its alias on
   the tab and hands it to the file on the first save.
4. Drag out (done 2026-10-31): dropping a tab outside the window opens
   `slate --new-window --position X,Y file` under the pointer and closes the
   tab here. Unsaved or untitled tabs travel as a handoff file
   (`%LOCALAPPDATA%/slate/handoff/*.json`: text, cursor, path, disk base,
   alias) passed with `--handoff`; the new window loads it, makes its own
   recovery checkpoint durable, then deletes the file. Only the primary
   window (single-instance owner) reads/writes `session.json`, so extra
   windows never clobber the session. Dropping a tab into another Slate
   window is not supported yet.
