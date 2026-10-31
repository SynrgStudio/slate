# Explicit document projects

## User flow

1. Write normally; an unassigned document opens Periscope directly in Global.
2. **Menú · Ctrl+P** at the bottom right opens the command palette (no dropdown
   or titlebar menu). Choose `create-project`, or use `:project`.
3. Choose an existing folder: Enter selects the highlighted folder, → navigates,
   ← returns to the parent/drives, Ctrl+Enter selects the current folder, Esc cancels.
4. The current document belongs to that project. Only now does Periscope offer
   Project / Global. Creating/assigning a project does not open Periscope itself.

The existing first-use consent/cache preparation still runs before searching when
necessary; it is not the project/global scope prompt. Git/CWD/last-opened directories
are no longer inferred as Periscope membership. Registered project folders make
files inside them project members (longest nested root wins); explicitly assigned
files may live outside the folder without being moved.

## Preservation and persistence

- No document write, move, buffer replacement, revision change or dirty-state
  change occurs from project selection. The folder itself is not created/modified.
- Untitled documents are associated **for this session** by buffer identity.
  On a successful explicit Save/Save As, membership is attached to the saved path,
  including destinations outside the selected folder. Save As defaults to the
  assigned folder for untitled documents. No untitled file is silently
  named or saved; untitled project metadata is not integrated into crash recovery.
- Saved-path membership and registered folders persist in
  `%APPDATA%\slate\projects.json` (v1 JSON, paths only). Other documents/windows do
  not inherit the last Periscope scope. New buffers forget session membership;
  swaps retain membership with the buffer. Closing/resetting embedded buffers
  retires their transient membership too.
- Metadata uses checked atomic writes with the fingerprint of the bytes read.
  Concurrent edits, corrupted/unsupported JSON, protected targets and oversized
  metadata are retained, not overwritten. Failure reports session-only assignment;
  explicit document saving can still succeed with a metadata warning.
- Creating a project cancels pending main-file loading to protect the current
  document; active safety/editor forms must be closed first. No CWD mutation,
  service installation, Flow schema/task-ID changes or global index deletion.

Flow's existing Git/filesystem scope logic remains unchanged by this delivery.
This is an explicit Periscope/document association, not a change to Flow's task
organization semantics or a physical folder creation command.

## Validation

260 tests pass serially; six manual integrations/benchmarks remain ignored.
New tests cover no automatic Git membership or stale scope inheritance, untitled
text/revision/identity/dirty-state preservation, cancel/choose-current-folder,
explicit Save As and restart persistence outside the project, buffer swapping,
corrupt/concurrent metadata retention, and the actual footer launcher click action.
Launcher geometry is checked at narrow/normal widths and 1/1.5/2 scale; Ctrl+P and
click preserve buffer/command text and use the same palette entry point.
The preparation regression now expects direct Global opening for an unassigned
document after consent/index completion.

Manual GUI retest: type unsaved text → Menu → Crear Proyecto → choose folder →
Periscope offers scope; cancel selector makes no project; new/unrelated document
opens Global; saved project document retains its association after restart.
