# Asynchronous main-document opening

## Decision

Move main-document file opening, byte reading, UTF-8 decoding and line-index
construction out of the UI thread. Keep the current document visible and editable
until the requested file is ready. Do not introduce a content cache.

## Execution and safety

`file_loader::Loader` lazily starts at most two reader threads per app. Its pending
queue retains only the newest request, and its result channel holds at most two
results. A second reader lets a newer local request run while an older read is
slow. Results carry request IDs; only the current request may commit to the UI.

The UI checks the originating path, buffer revision and dirty state before applying
an outcome. Edits, undo/save transitions, another document promotion, new-buffer
creation or a confirmation action invalidate/cancel the pending request. A reader
never modifies editor state, recents, source files, settings or Flow databases.
Failed/invalid-UTF-8 and obsolete results leave the current document unchanged.

Incoming text/paste/cut, pressed keys and pointer presses postpone application
until a later frame, so input intended for the visible old document is not applied
to the incoming one. Revision/dirty guards then detect edits made in that frame.

The prepared buffer receives a new revision on application, invalidating renderer
caches. Cursor restore, startup `--line`, heading fragments, daily dates and agenda
line destinations are applied to the loaded document, not to the previous buffer.
Only successful application updates recents/settings and starts Flow work.

Workers wake egui on completion. The UI does not busy-repaint throughout a read.
The footer shows the requested file and Esc cancellation; ordinary dialog Escape
handling retains priority. No reader thread is joined on the UI/closing thread.

## Diagnostics and validation

`file_open` spans now separate worker startup/queue wait, `open_file`, `read_bytes`,
`decode_utf8`, `build_buffer_worker`, dispatch-to-UI and application stages. Total
request latency still includes filesystem waiting; it is not UI blocking duration.
Presentation timing remains outside these CPU markers.

Tests inject gated reads rather than evicting Windows caches or disabling antivirus.
They check visible-document retention and frame construction while blocked, latest
request wins, coalescing/two-reader bounds, same-frame input safety, edits even after
dirty is cleared, cancellation, failures, UTF-8 validation and deferred navigation.
Test settings writes and documents stay in unique temporary directories. Reopening
also verifies externally modified text is read again instead of cached.

## Boundaries

This change covers the main editor's `open_path` routes and startup restore, not
link-preview/modal-buffer reads or every remaining filesystem operation. Flow
single-buffer sync and settings persistence retain their existing application path.

Portable synchronous OS reads cannot be forcibly aborted. Cancellation discards
queued jobs/results, not an already-running OS read. If both readers hang on storage,
the newest request remains queued, but the editor remains usable and closing does
not join those readers. This improves responsiveness during cold I/O; it does not
promise faster disk/antivirus latency or fix the previously measured inter-frame gap.
