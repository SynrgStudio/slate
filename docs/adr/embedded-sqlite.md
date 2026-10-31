# Embedded SQLite for Flow

## Decision

Use `rusqlite` with bundled SQLite instead of spawning its command-line executable.
Preserve existing cache locations/schema and the public Flow API. No user CLI
installation, uninstallation, or environment-variable changes are performed.

## Execution model

- Reuse up to four writer connections per UI/API thread; never share a connection
  across threads. Background scans own a separate writer and initialize it once.
- Use parameterized cached statements and WAL transactions of at most 32 files.
- Stream document reads/parsing into batches; do not retain a whole folder's text.
- Automatic scan, manual rescan and rebuild all execute on a worker thread.
- Keep writer lock waits short (UI 25 ms; workers 250 ms) and defer busy live-buffer
  updates rather than waiting multiple seconds on the UI thread.
- Export the derived TSV once after scanning. Single-buffer updates enqueue
  coalesced background exports; the UI never waits on the global manifest mutex.
  Serialize exports and use unique temp names. Bound API/shutdown flush waits to
  2 seconds; SQLite commits, not TSV delivery, are authoritative.
- Skip the current editor path and recent active-buffer heartbeat registrations;
  rebuild preserves their existing derived rows. Keep idle Markdown heartbeats
  fresh with a repaint scheduled every 3 seconds.

## Compatibility

The `tasks.kind` legacy migration is transactional and idempotent. Readers support
older caches lacking that column before migration. Existing rows, unrelated tables,
file formats, API JSON and task IDs are preserved. Rebuild deletes derived rows
transactionally rather than unlinking an open database with WAL sidecars.

## Validation and limits

Regression coverage includes migration, batch rollback, UTF-8/quotes/tabs/NULLs,
WAL readers and writer contention, live-buffer scanning and rebuild. The suite and
API read/rescan/complete integration trials pass with no SQLite CLI on PATH.

The 100-document isolated scan improved from 14.16 s (external CLI) to 0.206 s
(embedded, fresh DB); a warm existing-cache scan took 0.125 s. Single-run measurements
include process startup and do not establish typing or graphical startup latency.
A first Git-nested trial was rejected: the API normalizes scope to its project root.

Remaining work is deliberately separate: heartbeat/runtime file polling still uses
filesystem I/O; active registrations retain their existing 10-second freshness
semantics; a batch is bounded by file count, not maximum document bytes. Corrupt or
locked caches are reported, not deleted or rebuilt without an explicit request.
