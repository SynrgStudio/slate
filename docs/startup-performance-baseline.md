# Startup and file-open baseline — 2026-10-10

Instrumented release build; no performance optimizations applied. Three user-driven
runs in `%LOCALAPPDATA%\slate\logs`, starting at 05:59:25, 06:00:14 and 06:00:41 UTC.

| Measurement (ms) | Run 1 | Run 2 | Run 3 |
| --- | ---: | ---: | ---: |
| Framework begin to app constructor | 166.558 | 158.592 | 151.823 |
| Slate app constructor | 0.430 | 0.400 | 0.395 |
| Configuration load | 0.324 | 0.303 | 0.301 |
| First UI CPU build | 3.506 | 3.443 | 3.431 |
| Main to first UI CPU build complete | 172.243 | 163.850 | 157.130 |
| File bytes | 1950 | 1675 | 3207 |
| Synchronous read_text | 103.744 | 99.927 | 347.687 |
| Entire open_path | 104.762 | 100.951 | 348.764 |
| Open to following UI frame end | 123.566 | 118.866 | 450.764 |
| Background directory traversal | 626.771 | 141.872 | 159.598 |

Files: `D:\MonsterManualMinis\comparacionsoportes2.py`, `comparar.py`,
`extract_stl_dimensions.py`, respectively. These are different files, not repeated
reads of a controlled single file. All background scans found zero Markdown files.

## Code review

- `main` creates a Glow/OpenGL eframe application. eframe creates the native window,
  GL context/painter and integration before invoking Slate's constructor. The
  framework timing is aggregate: it does not isolate driver, window or font costs.
- `SlateApp::new`: style/fonts, initial state, settings, optional document restore
  or standalone agenda. No initial document/agenda was loaded in these three runs.
  Constructor costs are negligible relative to framework initialization here.
- `setup_style` probes two Linux Noto font paths even on Windows. These probes and
  style setup took about 0.08 ms combined; not a meaningful measured bottleneck.
- `open_path` reads the text synchronously on the UI thread before constructing the
  buffer, restoring cursor/recent files, indexing Flow, launching the background
  rescan and saving settings. File reading dominates these measured opens.
- `.py` files skip synchronous Flow indexing. Markdown indexing and restored
  Markdown documents are NOT covered by this baseline and may behave differently.
- Opening any file launches a recursive Markdown folder scan, including `.py`
  files. This ran in the background but spent 142–627 ms searching for zero files.
  It may compete for disk/CPU; no direct startup blocking is established by that.
- While Flow is scanning, the UI requests continuous repaint. Its impact on
  responsiveness/CPU is not isolated by the current logs.
- Markdown UI frames publish Flow heartbeat and poll multiple request queues,
  with repeated scope-root detection. These return almost immediately for the
  measured `.py` files; costs for Markdown remain to be measured.

## Conclusions and next controlled comparisons

1. Do not rewrite startup/settings based on these runs: Slate's constructor takes
   under 0.5 ms; most of the measured 157–172 ms is framework initialization.
2. Compare repeated opens of the SAME file on D:, then a local copy on C:, using
   the same release build. Slow tiny reads could involve storage/cache/antivirus,
   but the current evidence cannot identify the cause.
3. Reading on a worker thread could keep the UI responsive during slow I/O; it
   would not necessarily make the underlying read faster. Protect dirty buffers,
   cancel/supersede old requests and preserve error handling if implemented.
4. Consider coalescing/debouncing folder scans rather than simply removing them;
   agenda consistency depends on this behavior.
5. Measure startup with restore enabled and a representative Markdown document
   before changing Flow startup work.

Clock begins at entry to `main` and excludes OS loader/Defender work before it.
UI markers indicate CPU UI construction, not GPU presentation or first visible
pixels. Logs use an asynchronous writer; instrumentation still has some overhead.
No before/after performance improvement is claimed by this baseline.

## Repeated-file follow-up and investigation

Three GUI runs at 06:30:46, 06:31:10 and 06:31:15 UTC opened the same 268-byte
`C:\Users\dnaon\Desktop\Abrir Miniaturas Lychee.cmd`:

| Measurement (ms) | Run 1 | Run 2 | Run 3 |
| --- | ---: | ---: | ---: |
| File read | 8.345 | 0.072 | 0.070 |
| Entire open_path | 9.361 | 1.056 | 1.026 |
| Open to following UI CPU frame end | 94.152 | 86.366 | 87.229 |
| Main to first UI CPU frame end | 168.364 | 166.511 | 170.594 |

Warm reads are consistent with OS caching. The remaining ~85 ms is NOT measured
file reading or UI construction: the following UI build took under 0.2 ms. The
old instrumentation cannot separate the tail of the opening frame from framework
render/present/event-loop scheduling. No intentional 85 ms sleep was found.

The first Desktop Flow scan took 3475 ms, reporting zero successful files and 464
errors. The other two processes exited before background completion. `sqlite3`
is absent from the coding harness process PATH; an isolated rescan of a one-file
fixture under `target/perf-investigation` reproduced `sqlite3 unavailable: program
not found`. Existing background code discarded each underlying error and retried
for every file. This establishes a missing backend in the reproduced environment,
not the error reason of every individual historical Desktop file.

Bounded changes after investigation:
- Check SQLite CLI before background traversal or destructive manual rebuild;
  report missing-backend failure once, with its cause, instead of per-file retries.
- Log up to five per-file read/index failures and retain the first cause in UI.
- Log the opening frame's post-load tail, the next UI begin and the between-frame
  gap. These remain CPU timing markers, not presentation measurements.
- Explicitly request repaint after picker activation.

No system SQLite installation, renderer replacement, or general removal of Flow
scanning was performed. A new same-file trial is needed to compare the gap with
these finer markers; do not claim the old ~85 ms has been eliminated yet.

## Embedded SQLite follow-up

The external-CLI preflight above is now superseded by bundled `rusqlite`.
Scanning and manual rebuild run on workers, with one writer connection and
transactions of up to 32 files. Single-buffer TSV exports are also coalesced off
the UI thread, avoiding waits on the shared manifest export mutex.

An isolated 100-Markdown/100-task API rescan took 14.1635 s with the old release
and installed CLI, versus 0.2059 s with the embedded release and a fresh database
(0.1249 s for an existing database). Only Windows System32 was exposed on PATH
for the new release. These single-run measurements include process launch; they
measure indexing, not graphical startup, typing, or the historical frame gap.
Valid trials used temporary scopes outside Git projects. A preliminary Git-nested
trial was discarded because the API normalized scope to the enclosing repository;
it regenerated that project's derived cache without editing its Markdown sources.

Old/new API data and IDs matched for 100 cached tasks. Embedded completion updated
both fixture Markdown and cached state without an external SQLite executable.
See `sqlite-setup.md` and `adr/embedded-sqlite.md` for architecture and regression
coverage. Startup and the historical ~85 ms post-open gap still need GUI retesting
with this release before claiming any improvement to those timings.

## User GUI retest — 22:16 UTC, embedded release

Four separate processes opened the same 268-byte Desktop `.cmd` again:

| Measurement (ms) | First | Second | Third | Fourth |
| --- | ---: | ---: | ---: | ---: |
| `fs::read_to_string` | 557.566 | 0.071 | 0.072 | 0.076 |
| Entire `open_path` | 558.536 | 1.044 | 1.047 | 1.035 |
| Opening-frame UI tail after loading | 5.949 | 5.659 | 6.030 | 5.856 |
| Between opening and next CPU UI frame | 1.070 | 78.797 | 75.726 | 76.164 |
| Main to first CPU UI end | 192.006 | 174.340 | 162.170 | 160.094 |

Logs: `performance-20261010T221611.549Z-15012.log`,
`performance-20261010T221618.924Z-19712.log`,
`performance-20261010T221623.452Z-5768.log`, and
`performance-20261010T221627.709Z-19952.log`.

The first file-open delay is overwhelmingly inside OS-backed file opening/reading,
not buffer construction, settings saving, or SQLite. The `.cmd` skips synchronous
Flow indexing, and its folder scan starts only after reading completes. Warm opens
occur in distinct processes, so Slate's in-process buffer/connection caches cannot
explain the repeated-file read improvement. Windows filesystem caching is consistent
with the evidence, but storage, antivirus/filter activity and other OS costs are not
separately measured. File metadata shows `Archive`, with no Offline/ReparsePoint
flags or link target; Desktop also has no reparse/link attributes. This is not proof
that all filtering or synchronization is absent.

The historical inter-frame gap remains in the three warm trials (~76–79 ms).
The opening frame has already built the loaded document UI ~7 ms after `open_path`
begins; the next-frame marker is not the first content presentation timestamp.
GPU/presentation and input dispatch remain unmeasured. The perceived ~1 second
first-open delay cannot be fully reconstructed from these CPU markers: approximately
565 ms is covered from `open_path` begin through the opening-frame UI end.

The first process exited before its background scan completed; no final result
can be claimed for it. Subsequent Desktop scans indexed all 464 documents and
897 tasks in 696.6 / 233.5 / 229.6 ms, with 15 batches and zero errors each.

The next responsiveness change should move file reading off the UI thread with
request supersession and dirty-buffer protection, not add a cache that can serve
stale text. That keeps the editor responsive during cold I/O; it does not guarantee
lower underlying disk/antivirus latency. No cache eviction, antivirus changes, or
source-file modifications were performed during this investigation.

## Cold-read responsiveness change

Main-editor opening now queues bounded background reads, decoding and line-index
construction. The current document stays visible; stale results or results arriving
after edits cannot replace it. Startup line, heading, agenda and daily navigation
are deferred until application. Esc cancels a pending open without joining readers.
See `adr/asynchronous-file-open.md` for safety rules and remaining boundaries.

New `file_open` markers split OS `open_file` from `read_bytes`, UTF-8 decoding,
worker buffer construction, queue delay and UI application. A follow-up GUI trial
can distinguish a slow open from a slow read without clearing caches or changing
antivirus settings. Tests simulate a blocked reader and verify CPU UI frames keep
building, but they do not establish native GPU/presentation latency. No new cold-
start/file-open speedup is claimed before user measurements of this release.
