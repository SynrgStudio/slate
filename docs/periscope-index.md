# Periscope: file search with Seek

Periscope (`:periscope`, `:ps`, `:fzf`) finds files by name. The names come from [Seek](https://github.com/SynrgStudio/seek). Seek runs as a background process and keeps a live index of every file name on every fixed drive, kept current by drive watchers. It also serves agents and other programs from the same index. Slate talks to it over its pipe with version 1 of Seek's [protocol for programs](https://github.com/SynrgStudio/seek/blob/main/PROTOCOL.md) (`src/seek.rs`), on Periscope's worker thread.

## What a search does

- **Global:** asks Seek for names on every drive, ranked:
  1. exact names;
  2. what the user changed or opened lately;
  3. the folders they use;
  4. what is close to the project, or to the current document's folder.
- **Project:** only below the project folder. What `.gitignore` leaves out there (build output, dependencies, `.git`) stays out.
- **Query:** reaches Seek as typed, so its syntax works: `.md` or `ext:md,txt`, `!word`, `src/name` (in a folder), `!target/` (not through a folder), `"exact words"`, `*` and `?`.
- **Typos:** when nothing matches as typed, Seek answers with close names, and Periscope says so ("no exact match · close names").
- **The worker:**
  - answers only the newest query, after 40 ms of quiet;
  - asks again every 5 s while Periscope is open;
  - asks Seek for 500 rows and shows the best 100 files, so matching folders can be grouped.
- **First index:** on a machine indexing a drive for the first time, the status line says which drive, and how many names it has so far. Seek can't know the total in advance, so there is no percentage.
- **Opened files:** each file Slate opens is reported to Seek (`opened`), so it ranks higher everywhere.

## When Seek isn't running

If the pipe isn't there, Slate starts the installed `seek.exe --daemon` (`%LOCALAPPDATA%\Programs\Seek`) and waits for it. It runs detached, outside Slate's job, and keeps running after Slate closes. If Seek isn't installed, Periscope says so, with where to get it.

Shipping Seek inside Slate is planned through Seek's own Rust crate and its `bundled` option (SynrgStudio/seek#21, #22, #23). An installed Seek will be preferred. See issue #1.

## History

Periscope used to search Everything over `WM_COPYDATA`. Before it opened the first time, it also copied every path on every drive into `%LOCALAPPDATA%\slate\periscope\paths.sqlite3`: about 3 GB, used only when Everything wasn't running, and rescanned every 5 minutes in that case. Seek replaced both. Slate deletes that folder once, on start, if it is still there.
