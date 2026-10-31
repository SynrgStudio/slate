# SQLite in Slate

SQLite is now embedded through `rusqlite` with its `bundled` feature. Flow, agenda,
cache writes and the Flow API no longer invoke `sqlite3`, require WinGet, or depend
on PATH. The optional CLI installer and its prompt have been removed from the app.
An already-installed SQLite CLI or its PATH entries are left untouched.

## Compatibility and migration

Existing `agenda.db` locations, table/column names, task IDs and API JSON remain
unchanged. Writer initialization creates missing tables and adds the legacy
`tasks.kind` column transactionally, without replacing existing databases. Readers
can also read caches without `kind` before the first writer performs the migration.
No source Markdown files are rewritten by indexing. Corrupt/inaccessible caches
are reported in a small scrollable error dialog, never deleted automatically.

## Responsiveness

- UI/save updates reuse a bounded, thread-local connection cache. Its SQLite lock
  wait is capped at 25 ms; a busy live-buffer update is retried on a later frame.
- Automatic scans and explicit rescan/rebuild run on background workers.
- Workers reuse one connection/schema setup and prepared statements, committing
  at most 32 documents per transaction. No giant all-folder transaction is used.
- WAL readers see committed data without waiting for ordinary background writers.
- The rebuildable TSV index is exported once per scan, not once per scanned file.
  Single-buffer exports use a coalescing background queue, so the UI does not wait
  on the global TSV export mutex or a slow manifest write. API exit/shutdown flush
  pending exports with a bounded wait; SQLite commits remain authoritative.
- The current editor path and freshly registered live buffers are skipped by disk
  scans. Rebuild keeps their committed cache rows instead of unlinking an open DB.
- Markdown editors schedule a lightweight repaint every 3 seconds to keep the
  existing 10-second live-buffer heartbeat fresh while idle.

WAL database files must stay with their `-wal`/`-shm` sidecars while processes are
using them. Rebuild clears derived rows transactionally, not by deleting those
files. Background writer lock waits are bounded at 250 ms and errors are logged.

## Validation

Tests cover existing cache migration, typed Unicode/quotes/tabs/NULL values,
atomic batch rollback, concurrent WAL reads, bounded lock waits, coalesced exports,
active-buffer
protection, rebuilding without losing live cache rows, and background indexing.
The compiled suite was also run with a PATH containing only Windows System32;
all tests passed without the installed SQLite CLI being discoverable.

The old and new API returned identical data and IDs for 100 fixture tasks. The new
API completed a fixture task and updated both its Markdown and cached state with
no SQLite executable on PATH.

One isolated 100-document CLI scan took 14.16 s with external processes versus
0.206 s with embedded SQLite and a fresh database (0.125 s reusing an existing
cache). These are single-run indexing measurements, including process launch;
they are not first-window latency or typing-latency benchmarks. Temporary scopes
were used for valid measurements; a preliminary Git-nested scope was excluded
because the API deliberately normalized it to the enclosing project root.
