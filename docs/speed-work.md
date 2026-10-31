# Speed work — 2026-10-31

Inspired by how Fred stays fast (cheap edits, wide search, measuring everything),
applied without changing Slate's editing model. Release build, 16 logical CPUs,
5 MB / 27,343-line fixture `D:/slate-baselines/large-document-5m.md`.

## 1. Undo stores deltas, not documents

`EditorBuffer` used to push a full copy of the text before every edit (up to 256
copies: ~1.3 GB for the 5 MB fixture). Each entry is now `start / removed / inserted`
plus cursor state; history is capped at 10,000 entries or 64 MiB of deltas.
Every mutation goes through one incremental path (line index splice + layout
change), including indent/dedent and line moves, which previously rebuilt the
whole line index.

Typing groups into one undo step per word (whitespace after a word, newline,
cursor move, any other edit or a 1 s pause starts a new step). Undo now also
restores a selection that typing replaced.

| 5 MB fixture, CPU per op incl. frame | before | after |
| --- | ---: | ---: |
| insert | 2–3 ms | 0.8–1.4 ms |
| undo | 5–6 ms | 0.5–1.1 ms |
| 300 single-char edits, history size | ~1.3 GB (capped 256) | < 4 KiB |

Reproduce: `cargo test --release -- --ignored --nocapture benchmark_large_document_editor`.

## 2. Find searches on all cores

`search::find_matches` splits large texts (≥ 256 KiB) into per-thread chunks,
searches with `memchr::memmem`, and merges in order before returning — callers
still get every match in the same frame. Overlapping matches and case-insensitive
behavior are preserved.

Also fixed: the old code lowercased with `to_lowercase()` and used those offsets
on the original text; characters whose lowercase has a different UTF-8 length
(e.g. `İ`) shifted every later match. Case folding now keeps byte lengths exact.

| query on 5 MB | before | after |
| --- | ---: | ---: |
| `e` (345,812 matches) | 30.1 ms | 7.0 ms |
| `lorem` / needle / `café` | 19–20 ms | 2.1–2.3 ms |

`:find-note` now reads and tests files on all cores in batches of 512 (stopping
early at 200 results) and computes result hints in parallel.

Reproduce: `cargo test --release -- --ignored --nocapture benchmark_find_large_document`.

## 3. Timing overlay

`F12` or `:perf` (`:hud`, `:timings`) shows frame CPU (last, p50, max over 2 s),
total CPU of the previous frame including tessellation, CPU of the last frame
that handled a keystroke, last find / find-note / open timings, startup time,
document size and undo history size. It refreshes every 250 ms only while open.

## 4. Builds

- `[profile.dev.package."*"] opt-level = 3`: dependencies are optimized once and
  cached, so debug builds are pleasant to use. The full test suite went from
  ~23 s to ~11–13 s.
- `[profile.release] lto = "thin", codegen-units = 1` for the shipped binary.
  Release rebuild of Slate alone: ~1 min 10 s.
- `main.rs` 15,111 → ~10,100 lines: shortcuts, settings, help, Save As, agenda,
  picker, Periscope and preview UIs plus the `flow-api` CLI moved to their own
  modules unchanged; the dead `lance_dialog` / `file_picker_dialog` were removed.
- Compiler warnings 85 → 0 in normal builds (remaining test-only deprecations).

## 5. Startup

Cold start of the window is dominated by eframe creating the window and GL
context: ~300–350 ms on this machine, Slate's own constructor < 0.5 ms.

- **Single instance.** When Slate is already running, `slate file.md` hands the
  path (and `--line`) to it over a loopback-only socket guarded by a per-run
  token, then exits. Measured: forwarding done 4–6 ms after `main`; ~90 ms
  wall-clock including Windows process spawn, versus ~300+ ms for a new window.
  The running window un-minimizes, focuses and opens the file with the same
  dirty-buffer guard as `:open`. Opt out with `--new-window` / `-n` or
  `single_instance = false` in `config.toml`. `--scratch` and `--agenda`
  always start their own window.
- **Renderer startup cost.** wgpu reaches the app constructor in ~465–500 ms
  versus ~300–400 ms for glow. Accepted for the latency win in section 6.
- **Flow index off the UI thread on open.** Opening a Markdown file wrote the
  Flow SQLite cache (commit + fsync) on the UI thread: 6–12 ms per open, 47 ms on
  the first open in a folder. It now runs first on the existing background rescan
  thread (with short retries while SQLite is busy). First frame after opening:
  ~51 ms → ~5 ms CPU.

Not done: keeping Slate resident after its window closes (instant reopen from
nothing running). It needs a tray/hidden-window lifecycle and exit semantics
decided first. Saving still updates the Flow index on the UI thread (6+ ms per
save); moving it needs care because daily events use its result.

## 6. Keystroke-to-photon latency

Measured with PresentMon 2.6 (`MsAllInputToPhotonLatency`), 60 Hz monitor,
RTX 3090, ~100–360 keystrokes per window.

- **Idle repaint loop removed.** Drawing the caret requested another frame, so
  a focused, idle Slate presented ~250 frames/s. Now an idle window presents
  ~1 frame/s (the 1 s background poll). Guarded by
  `idle_focused_editor_does_not_schedule_frames`.
- **wgpu is the default renderer.** OpenGL (glow) presents through
  "Composed: Copy with GPU GDI": median 19–29 ms key to screen. wgpu on DX12
  with `desired_maximum_frame_latency = 1` and `AutoNoVsync` (tearing allowed)
  uses a DXGI flip-model swapchain; once Windows promotes it to
  "Hardware: Independent Flip" the median is **2.5 ms** (p10–p90 1.3–3.4 ms).
  `Fifo` and `Mailbox` were measured too and stayed in "Hardware Composed"
  at 33–35 ms, so they are only available through `SLATE_PRESENT`.
  `SLATE_RENDERER=glow` forces OpenGL; Slate also falls back to glow
  automatically when wgpu cannot create a surface.
- **Window drags no longer render.** In the Windows move loop, every
  `WM_WINDOWPOSCHANGED` became `WindowEvent::Moved` and every `WM_PAINT` a
  `RedrawRequested`, and egui answered both with another frame: glow rendered
  ~330 frames/s during a drag, and wgpu's flip-model present blocked the loop,
  so drags moved at ~30 Hz. `src/window_drag.rs` subclasses the window and,
  only between `WM_ENTERSIZEMOVE` and `WM_EXITSIZEMOVE`, answers pure moves and
  paints with the default procedure, then requests one repaint at the end.
  Each drag logs `window.drag ... frames=N`; measured: 0 frames and ~120
  position updates/s (was ~30). Resizes still repaint.

## 7. Text representation

See [adr/text-buffer-representation.md](adr/text-buffer-representation.md):
keep a single `String` + line index for now.
