# Periscope: Everything + independent metadata index

## Delivery

Windows Periscope prefers an already running Everything (default instance), using
its public Unicode IPC directly: v1 for interactive search and QUERY2 (1.4.1+)
for bulk full-path/size/mtime import. No SDK DLL, CLI subprocess, shell,
service installation, elevation, PATH modification or Everything configuration
change is required. Reference consulted: Voidtools Everything SDK
`ipc/everything_ipc.h` from https://www.voidtools.com/Everything-SDK.zip.
Only the protocol is implemented; no vendor binary/source is shipped.

Everything's user application must be running and able to answer queries. A service
alone is not sufficient. The optional `IS_DB_LOADED` readiness command is **not**
a gate: Windows can filter it independently from public query `WM_COPYDATA`, and
a false/unsupported readiness reply does not prove query IPC is blocked. Hung, unavailable, blocked IPC or invalid replies fall
back to SQLite with a visible reason. A successful empty Everything search is
not evidence that the location is fully indexed: Everything's own inclusions and
exclusions still apply. For an unindexed location, choose the SQLite backend in
Settings. Interactive searches do not merge partly covered scopes; preparation
can import covered roots and filesystem-scan only roots without an inventory.
Custom Everything instance names / 1.5 alpha instance discovery are not included.

## First-use consent and mandatory preparation

The first `:periscope` checks a persistent preparation marker **read-only in a
worker**, before any scope selection or search request. No filesystem scan begins
from that check. If the current root set has not been prepared, a centered prompt
lists the roots and asks for explicit Y/Enter acceptance or N/Esc cancellation.

Acceptance first imports Everything's indexed inventory using QUERY2, requesting
full paths, size and modification FILETIME in bounded **8,192-item pages**, sorted
by name (Everything's always-fast sort). There is **no per-file Slate stat/open**,
no 4,096 search-candidate limit, and SQLite writes use 2,048-row transactions.
Folder entries advance pagination but are not cached as files. Unknown metadata
is represented as zero, without filesystem probes. Slate's own cache directory
is excluded from the native query so creating its WAL does not change totals.

A root with no available inventory uses the slower filesystem scanner; already
imported roots are not walked again. Disabling Everything uses the original
scanner. Fallback and its reason stay visible in progress. This copies metadata
into Slate's independent cache, not Everything's database. No document contents
are read; filesystem fallback can take minutes or longer. Progress reports source,
root, indexed entries, files and completed roots, without a guessed percentage.
**There is no continue-while-indexing action.** Main editing/search shortcuts and
project/global opening are suppressed during preparation.

Only after every import/fallback and final database marker succeeds does search
open (or the project/global choice appear for an assigned document). The temporary completion of a fallback subset is not
published as overall success. Malformed/out-of-scope pages, interrupted mid-root
IPC, unavailable filesystem roots and database failures still block completion
and offer retry/cancel, retaining committed metadata.

**Normal live-index changes never restart or fail preparation.** Each root fixes
its initial entry budget on the first page, then advances offsets monotonically.
Growth does not extend that budget indefinitely (the last bounded page can include
extra entries); shrinkage ends at the current EOF, including an empty reply beyond
it. Pagination can repeat paths as names move: SQLite upserts deduplicate them and
completion counts unique committed rows rather than requiring unchanged totals.
The ready marker means the bounded initial pass finished, not that every filename
was captured at a single instant. New/moved names can be missed during live paging;
interactive Everything searches still consult its current index, and filesystem
fallback refreshes the independent cache when that backend is used.
Imports do not purge old entries: live omissions/exclusions are not proof of deletion.

Everything's coverage/exclusions and current index state apply; this is not a
transactional filesystem snapshot or a guarantee that protected/unindexed files
are included. Only a completely unserved root triggers filesystem fallback;
partly indexed but nonempty roots follow Everything's inventory. Fallback access
errors are reported as before.

Cancellation stops the one-shot worker cooperatively, keeps committed metadata,
and leaves search closed. An interrupted traversal does not write the ready
marker; a subsequent attempt reimports/rescans rather than persisting
an IPC offset or directory queue. If a complete traversal finished just before cancellation,
its completion marker can already exist, but cancellation still does not open
search. No cache is deleted. Next openings reuse the marker for the same normalized
root set; a changed root set/new cache asks again. Periodic maintenance after that
continues as below, so the prepared cache is not claimed perpetually current.

After preparation, unassigned documents open **Global directly**, without a
scope prompt. **Menu → Crear Proyecto** explicitly assigns an existing folder to
the current document; only assigned documents offer Project / Global. Git/CWD
are not membership. Saved documents persist association, untitled documents
retain it for the session until explicit save; see [document-projects.md](document-projects.md).

## Scope and UI

`:periscope` uses explicit document membership, not Git detection or the last
picker root. The alternative is Global, not Home. Tab switches project/global
only when the current document has a project.
On Windows the default global scope is local **fixed** drives enumerated through
Win32, without probing network/removable volumes on the UI thread. On other
platforms the fallback default remains Home.

Settings adds **Periscope → Use Everything when available**, persisted as
`periscope_everything = true`. Disabling it explicitly uses the independent
SQLite index. Keyboard selection/Left/Right/Enter follow other Settings rows.

Custom global roots are currently configured through repeated keys in the
existing config (`%APPDATA%\slate\config.toml` on Windows), not a new drive UI:

```toml
periscope_everything = true
periscope_root = "D:\\notes"
periscope_root = "C:\\Users\\dnaon\\Desktop"
```

Omit all `periscope_root` keys to use defaults. Only absolute roots are accepted.
They persist on later settings saves. UNC/removable paths are opt-in through these
keys; slow filesystem operations run on the scanner, not the UI/query thread.
Project scope restricts the search regardless of the global roots.

The active results dialog now uses the common centered 80% fixed-chrome frame.
Header/status and clickable close/scope/open/modal actions stay fixed; the painted
list derives its row capacity from available body height rather than 16 hardcoded
rows. Keyboard selection remains visible, the wheel moves through results, and
full paths are available on hover. Long scope/status text is truncated with hover
details, not allowed to expand the frame. Empty input is idle with a typing hint:
no broad first-page IPC query or filesystem scan is started for an empty search.

Results still use fuzzy scoring, folder grouping, keyboard selection and existing
main/modal opening handlers. Multiword searches recognize whitespace boundaries
and allow ordered fuzzy abbreviations within names (e.g. `Abrir minis` matches
`Abrir Miniaturas Lychee.cmd`), consistently in Everything ranking and SQLite.
Searching/opening CMD files does not execute them. Opening always reads the actual file; no document
content cache is introduced. The dirty main-buffer guard is retained.

The old `sh` / `fd` probing, synchronous per-key project walks, 20,000-file project
cutoff and 200,000-file Home-only in-memory collection were removed.

## Bounded work and honest limits

One lazy query worker coalesces pending requests. IDs protect result application;
latest query wins. Closing Periscope cancels query publication. SQLite scanning
uses a separate lazy worker, so slow filesystem discovery does not block search
of the existing cache. A scan can continue after the picker closes.

**Native ordinary search is first**, with regex disabled: `file:` plus a quoted
boundary-safe root group and quoted plain words. It returns the first 256 native
file matches immediately, ranks/truncates to 100, and does not scan 4,096 fuzzy
candidates first. Ordinary words use Everything's AND semantics, independent of
word order; filename prefixes rank first. Explicit slash/backslash terms enable
path matching. Operator/quote/control-bearing input is not injected as native
syntax: it uses escaped-regex fallback instead.

If native search has no files, basename-bounded fuzzy regex and then full-path
fuzzy remain the fallback. Filename matches take precedence, not a global merge
with all path-only matches. The independent SQLite ranker also accepts unordered
literal words, while fuzzy abbreviations remain ordered.

Each query thread reuses its private reply HWND. A new request cancels the old
client wait (checks between message pumps) and the next search on the same HWND
replaces the old Everything query. Per-request copydata tags discard late old
replies. Input bursts coalesce for 40 ms; no main-thread waits were introduced.
Query replies can still take up to five seconds in the worker (send 300 ms), but
superseded queries do not wait for that timeout. Inventory preparation keeps its
separate thread/large pages and cancellation contract.

Everything requests 256 candidates per page and evaluates up to **4,096** per
fuzzy search pass, showing at most **100** ranked file paths. The native ordinary
pass considers only the first page; native-empty + fuzzy-name + fuzzy-path may
run multiple passes. Displayed counts and route describe the producing pass. The status explicitly
reports evaluated/total candidates and **partial ranking: refine query** when
truncated. This is not a limit on the Everything index and not a promise that
these are the best 100 across every matching indexed path. Folder candidates count
in the protocol total; they are omitted as files and existing folder grouping
is applied afterward. Empty input requests only the first page.

Search text is escaped into regex, not injected into Everything command syntax.
Project/global scope prefixes are literal with directory boundaries; returned
paths are scope-filtered again. Unicode packets, lengths, item/string offsets and
terminators are checked before decoding. Strings preserve normal UTF-16 Unicode;
unpaired surrogates are unsupported rather than mapped to a different filename.
Win32 send/reply waits are bounded and confined to the worker; cancellation occurs
between IPC pages, not by forcibly interrupting Windows calls.

## SQLite fallback

Separate database: `%LOCALAPPDATA%\slate\periscope\paths.sqlite3`.
It contains paths, normalized names, parent/root, size, modification time and scan
metadata — **not document text, Flow tasks or task IDs**. It is unencrypted metadata.
WAL and short 256-file transactions permit queries while scanning.

The scanner enumerates directories iteratively, without an arbitrary file count
cutoff. It skips symlinks/reparse points, its own cache directory and paths that
cannot be represented losslessly as UTF-8. Inaccessible roots report unavailable;
errors do not trigger whole-root cache deletion. Successfully enumerated
directories retire missing files, and successful full scans retire unseen paths.
Database-issued scan generations prevent older processes overwriting/purging
newer scan metadata. Corrupt/open-failed caches are retained and errors reported;
no automatic eviction or deletion.

The fallback uses periodic rescans (nominal five-minute interval, dependent on
scan duration), not a USN/ReadDirectoryChanges watcher. Therefore newly created,
renamed or deleted paths can be temporarily stale, and initial indexing may be
slow. It does not promise Everything-like discovery speed or instantaneous
updates. Cached paths from inaccessible roots can remain; opening handles errors
through existing document-safe code. Root changes stop scheduling old scans, while
previous cached metadata remains for reuse. Only the top-100 ranking heap and
bounded write batches are retained during querying/scanning; a large directory
queue can still consume memory.

## Evidence / retest

260 tests pass in series; six manual benchmarks/integrations are ignored by default.
Explicit-project tests cover the bottom-right palette launcher, assignment without document saves,
Global without membership, per-file restart persistence, no stale scope leakage,
new/swapped buffers, picker cancellation and safe metadata retention.
Native-first additions cover safely scoped quoted words/operator rejection,
unordered word ranking, in-flight latest-query cancellation without filesystem
fallback, and ignoring wrong/stale native reply tags. Live global search on the
running Everything index found the same Desktop CMD with **`abrir mi`: 40.799 ms**
and **`mi abrir`: 47.184 ms**, both native, five candidates. A repeat measured
40.332/39.177 ms respectively; scoped Desktop lookup also passed. `Abrir minis`
falls back to fuzzy (1416.522 ms; 1286.697 ms on the repeat). These are synchronous worker IPC+ranking wall times:
they exclude the 40 ms input coalescing, UI polling and GPU/presentation, and do
not promise identical speed for every query or parity with Everything's own UI.
Results tests cover viewport containment/fixed actions, wheel selection with
untouched dirty main text, empty-query idle without IPC/scan, whitespace and
abbreviated CMD names. Live global `Abrir minis` found the actual Desktop
`Abrir Miniaturas Lychee.cmd`: 226 native candidates, 100 ranked results, 1306.458 ms
worker wall time. The file was neither read nor executed for this verification.
QUERY2 tests cover packed Unicode paths/metadata, malformed replies, 20,010
synthetic nonexistent files imported across pages without a filesystem inventory,
partial IPC failure/malformed pages, cancellation, boundary filtering and per-root
fallback selection. Additional regression tests simulate continuous growth,
shrinkage (including EOF beyond the new total), repeated paths, and validate that
offsets never restart and preparation completes within the initial budget. Live read-only QUERY2 returned the baseline size/mtime in
~121 ms. An isolated temporary SQLite import of **22,719 repository files** took
**985.586 ms** (~962.943 ms for an earlier 22,630-file sample). These are worker
IPC+SQLite wall times, not GUI latency or a measured whole-drive speedup.
Preparation tests cover consent without creating a DB/scanning, blocked scope
opening until completion, persistent same-scope reuse, root-set invalidation,
decline/cancellation, root/database failures, unchanged documents and centered
fixed chrome. Parallel suites were repeated, including a complete 241-test passing run.
Existing safety-flush and async-startup tests also exhibited intermittent
parallel failures in the previous delivery; the final serial run passed all
243 tests. The live metadata/empty-EOF IPC integration also passed (two queries
in 236.948 ms). No safety or async
open implementation was changed in this delivery.
A native Win32 mock server deliberately lacks readiness support but answers query
IPC: the regression test verifies query success and separately identified rejection.
New tests cover packet bounds/Unicode, regex escaping, scope boundaries including
literal `%`/`_`, persistence, incomplete-scan retention, concurrent scan generations,
20,010 indexed paths without the old cutoff, latest-query coalescing, automatic
fallback through an injected unavailable provider, owned config persistence and
keyboard toggle, UI response application and dirty-buffer opening protection.
All test databases/documents are owned temporary fixtures.

Read-only live test against the user's already running Everything found the
5M-character fixture outside Home in `D:\slate-baselines`; three candidates,
about **126–133 ms wall time** for IPC (including retest after removing readiness gating). This is query-worker latency, not complete
GUI input-to-present latency. The source fixture SHA-256 remains unchanged.

```text
cargo test --release --locked everything_ipc::tests::live_everything_finds_large_document_outside_home -- --ignored --nocapture
```

`periscope.query` logs backend, request ID, counts and worker elapsed time, not
search text. `periscope.ipc_failure` distinguishes query-send Win32 error codes,
explicit query rejection and accepted-query reply timeout. Access denied (Win32 5)
is reported as such, not conflated with readiness. The user's GUI screenshot
showed the old readiness-gate message; its exact OS cause was not reproduced by
the standalone live test, so GUI retest remains necessary. No elevation or Windows
message-filter/permission changes were made; fallback cache data was retained. GUI retest: open `:periscope`, Tab to Global (or N from the scope
prompt), search `large-document`, and confirm a `D:\slate-baselines` result.
On the first opening, consent to the root list and wait for completion before
choosing scope. Cancel/decline tests keep search closed. Verify the next opening
reuses completion without repeating the preparation prompt. Try typing rapidly
and changing scope; verify stale searches do not replace the current list. Test SQLite with a small configured root first, if desired, rather
than forcing a full-disk initial scan just for validation. Do not stop Everything
or kill Slate to test fallback.

The result-dialog fixed-chrome migration and GUI root selector remain separate
UI follow-ups; this delivery changes the search engine, not all remaining dialogs.
