# Dialog consistency audit and first delivery

## Contract

One centered frame occupying 80% of the available window, monospace Slate colors,
stationary header/footer, and scrolling owned by the body. Long metadata truncates
with a full-text tooltip rather than expanding the frame. Keep existing keyboard
bindings and safe-save/dirty-buffer guards; no document writes merely from painting.

## Delivered: forms / embedded buffers

`src/dialog_ui.rs` provides a shared fixed-chrome frame, 16 pt title, 13 pt body,
bounded subtitle and responsive clickable shortcut actions. Footer actions wrap;
when descriptions would require more than two rows, keys remain and tooltips
provide descriptions. Tooltip typography matches the footer. No nested frame or
outer whole-dialog scrolling. Bodies retain their own scrolling behavior.

`src/form_dialogs.rs` migrates:

- Periscope scope prompt: project folder / Global / cancel, same handlers as keys.
  Global searches every drive with Seek, not the legacy Home walk;
  see [periscope-index.md](periscope-index.md).
- Capture: optional title keeps initial focus; the captured-text preview scrolls
  vertically using visible rows instead of stopping after 220 pt. Long individual
  lines truncate with a full-text tooltip. No archival until explicit confirmation.
- Scratch: editor fills the body instead of a fixed 330 pt area. Actual archive
  location replaces a hard-coded Linux path. Archive, Entries and Hide call the
  existing handlers. Hiding preserves nonempty text.
- Modal buffer: path/dirty/heading metadata fits the header, editor fills the body,
  and swap/promote/save/close remain available in a fixed footer. Closing dirty
  text uses the existing guard, not forced discard.

## Delivered: Periscope results

The active renderer now uses `dialog_ui` rather than a hard-coded 860 pt/16-row
frame. Input/list stay in the bounded body; row capacity follows available height
and the selected window follows keyboard or wheel navigation. Close, scope,
open/expand and modal actions remain fixed at the bottom. Full paths and long
scope/status details have hover text. Tests cover the screenshot-sized viewport,
small/large geometry and preservation of dirty text. Search correction and live
Desktop CMD verification are documented in [periscope-index.md](periscope-index.md).

## Find note — migrated

`src/find_note_ui.rs` uses the shared centered 80% frame, stationary header and
responsive clickable footer. Removed the fixed 860 pt width/16 rows and old
renderer. Results use the body height, reveal keyboard selection after resize,
and support wheel navigation. Match hints hide below the useful column width;
full path/hint are available on hover. Long queries stay clipped in their own
region with the typed end visible. No row or query paint overlaps chrome.

Search/filtering, heading expansion, filesystem scope and existing main/modal
opening handlers remain unchanged. Dirty main text is never replaced; clean main
opens still use the async loader. Visible-file reads for heading indicators remain
fresh and synchronous as before: **this delivery makes no search/IO speed claim**.
Very short bodies may have no whole result row; selection is retained for resizing.

## Audit queue — not yet migrated

Active call sites in `SlateApp::update` / editor UI were checked, not just names:

| Dialog | Remaining findings |
| --- | --- |
| Link assist / heading / web | Shared active renderer; outer whole-dialog scroll and legacy sizing |
| Save As | Legacy dimensions and list viewport; header/footer need separation |
| Scratch entries | Fixed 820×470 sizing, 8 rows, cropped preview; adaptive panes/scroll needed |
| Flow item edit | Active form; check fixed fields, footer, autocomplete and keyboard focus |
| Agenda | Active multi-segment form/lists; check column widths and each scroll region |

Open/Recent/Lance use the active `slate_picker_dialog`, already fixed-chrome and
covered by picker tests. The old `file_picker_dialog` and `lance_dialog` are not
called by current UI dispatch; do not confuse those legacy renderers with active
pickers. Settings, palette, help and safety notices already have fixed chrome.
Inline link/org suggestions are popovers, not full 80% dialogs, and are separate
from this audit.

Next bounded wave: Link assist, then Save As / Scratch entries;
Flow/Agenda last to avoid mixing geometry changes with task semantics.
The audit does not declare all dialogs complete or optimize Flow without measurement.

## Validation / retest

266 tests pass serially, six manual benchmarks/integrations ignored. After the user's Capture/Scratch
screenshots exposed missing anchoring, DialogArea now centers by default; callers
cannot accidentally omit it. Tests assert position as well as size, including
resizing the same open dialog between small and large windows. New tests exercise all four forms
at 420×260, 900×560 and 1200×800, 80% geometry, disjoint header/body/footer,
contained actions, stationary chrome under wheel input, actual editor-body
scrolling, Capture focus/typing, dirty modal close and Scratch hide retaining text.
Fixtures do not archive or save user files; font size is deterministic.

Visual retest requested: `:capture`, `:scratch`, a linked modal buffer and the
Periscope scope prompt, especially after resizing and with long content/paths.
Five active renderer groups remain pending as listed above.

Find note regressions additionally cover same-open-dialog resizing with long
Unicode paths/query/hints at 420×260, 900×600 and 1200×800; contained rows and final
selection reveal; wheel/keyboard/click navigation with stationary chrome; empty
results and Escape; heading toggles/filter input; modal opening without losing
unsaved main text; dirty-main refusal and unchanged disk; clean async-main dispatch.
Manual GUI retest requested: `:find-note`, type a filter, toggle headings, resize,
scroll, open with Enter / Shift+Enter, and confirm dirty text remains protected.
