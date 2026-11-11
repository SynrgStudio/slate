# Periscope: file search with Seek

Periscope (`:periscope`, `:ps`, `:fzf`) finds files by name. The names come from [Seek](https://github.com/SynrgStudio/seek). Seek runs as a background process and keeps a live index of every file name on every fixed drive, kept current by drive watchers. It also serves agents and other programs from the same index. Slate talks to it through Seek's own Rust crate, `seek-client`, which speaks version 1 of Seek's [protocol for programs](https://github.com/SynrgStudio/seek/blob/main/PROTOCOL.md) over its pipe. `src/seek.rs` adds Slate's messages and the types Periscope reads, and the calls run on Periscope's worker thread.

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

Slate carries Seek. The crate's `bundled` feature builds Seek 0.5.1's `seek.exe` into `slate.exe` (about 1 MB), after checking it against the release's SHA-256. When the pipe isn't there, the crate starts the newest Seek it knows of:

- the installed one (`%LOCALAPPDATA%\Programs\Seek\seek.exe`), which wins at the same version;
- or the one Slate carries, written to `%LOCALAPPDATA%\slate\seek\<version>\seek.exe` only when it is the one started, so its process is named `seek`, as an installed one's. Older versions there are deleted when nothing runs them.

Either way it runs as `--daemon`, detached, outside Slate's job, inheriting none of Slate's handles, and keeps running after Slate closes. If a Seek older than one of those runs, it is replaced. Seek's data lives in `%LOCALAPPDATA%\Seek`, shared with an installed Seek and with agents.

Every call has the time limit PROTOCOL.md gives (5 s, or 45 s for searches, which wait for the index right after Seek starts). Past it, Periscope says Seek may be stuck, and the next search opens a new connection. Slate never kills Seek.

**Building:** the first build downloads that `seek.exe` from Seek's release. Without network, set `SEEK_CLIENT_EXE` to it; it is checked against the same hash.

## History

Periscope used to search Everything over `WM_COPYDATA`. Before it opened the first time, it also copied every path on every drive into `%LOCALAPPDATA%\slate\periscope\paths.sqlite3`: about 3 GB, used only when Everything wasn't running, and rescanned every 5 minutes in that case. Seek replaced both. Slate deletes that folder once, on start, if it is still there.
