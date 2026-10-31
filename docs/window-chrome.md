# Window title and controls

The 30 pt custom titlebar centers its monospace 12 pt title on the entire window,
not on the space remaining to the left of the window controls. Symmetric margins
reserve the control width on both sides. Long titles ellipsize on one line; very
narrow windows omit the title instead of overlapping controls.

Minimize, maximize, restore and close icons are vector strokes, not font glyphs.
Each control retains a 38 pt wide button, focus/click behavior, an accessible name
and a tooltip matching footer typography. Hover/focus uses a subtle background;
Close uses muted red. The restore icon uses overlapping outlines.

Viewport commands are unchanged. Titlebar dragging, double-click maximize/restore,
resize edges and the application's existing safe-close handling remain in place.

There is **no titlebar menu or dropdown**. A text-only **Menú · Ctrl+P** launcher
sits at the right of the bottom command row and opens the existing command palette.
Crear Proyecto is a palette command. The launcher uses the footer's muted 12 pt
monospace style, reserves non-overlapping hint space, hides whole at tiny widths,
and disappears while a nonempty command is being edited (typed text is preserved).
Click and Ctrl+P share the same palette-opening path. Titlebar dragging is restored
across the original left region. See [document-projects.md](document-projects.md).

260 tests pass serially, with six manual benchmarks/integrations ignored. New tests verify title
centering/control separation at widths 180, 300, 420, 900 and 1600 pt, long titles,
vector-only icon bounds at scales 1, 1.5 and 2, and actual button clicks dispatching
close, minimize, maximize and restore commands. Visual retest remains recommended.
