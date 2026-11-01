# Editor font and stationary Settings chrome

- Settings now includes **Editor font size** (8–32 logical points). The default is
  12 pt, equal to the existing line-number font. Change it with the numeric control
  or select the row and use Left/Right; Enter resets it to 12 pt.
- `editor_font_size` is saved in the existing config file. Invalid/non-finite values
  fall back to 12 pt; finite values outside the range are clamped.
- The size applies to the main editor, link-preview editor and Scratch. Read-only
  Markdown preview and dialog/toolbar fonts keep their existing independent style.
- Gutter numbers retain their 12 pt size, but now share the text's vertical center
  instead of being shifted down by 2 points. Gutter width uses actual digit metrics.
- Changing size reflows wrapping and reveals the current cursor; it does not alter
  document text, undo history or dirty state.

Settings uses the full bounded dialog width, removing the blank right-hand panel
caused by a 520-point inner frame inside a wider outer area. Its header and footer
are outside the vertical body scroll area. Keyboard selection reveals the selected
row, while ordinary mouse-wheel scrolling does not snap back to that row. The
command palette likewise keeps its header/input/footer outside scrolling results.
Existing stationary help and Open/Recent/Lance picker layouts are retained.
Settings also includes ten persisted Footer visibility checkboxes after the font
row; see [compact-footer.md](compact-footer.md) for behavior and defaults.
The following Periscope checkbox enables optional Everything integration; the
independent SQLite backend remains available. See [periscope-index.md](periscope-index.md).

Headless regression tests cover Settings dimensions and chrome at 420×260,
900×560 and 1200×800, scrolling, keyboard selection, font persistence/clamping,
keyboard font adjustments, command-palette chrome, and gutter/text centering at
8, 12, 24 and 32 pt. Row height accommodates both editor and gutter fonts, so
small text does not make line numbers overlap. Test config writes use a unique
temporary directory.

## Font and cursor (Settings → Editor · font and cursor)

- **Font**: Hack (egui's built-in monospace, the default) or **JetBrains Mono**
  (embedded from `assets/fonts/JetBrainsMonoNL-Regular.ttf`, the no-ligature
  build, SIL Open Font License in `assets/fonts/JetBrainsMono-OFL.txt`). The
  usual monospace fonts stay behind it for symbols and emoji it lacks. It
  applies to the editor, link preview and Scratch; dialogs keep their font.
- **Block cursor**: a full character cell with the character drawn inverted,
  instead of the 1.5 px line.
- **Animate cursor jumps**: the cursor glides (90 ms, ease-out) to a new place
  — clicks, line jumps, search, Ctrl+Home/End. Moving one character (typing,
  ←/→) stays instant, and text is always drawn in the frame of the key; only
  the caret is interpolated. The animation runs in document coordinates, so
  scrolling never animates it, and it asks for frames only while gliding.
- **Cursor color**: seven swatches (←/→ cycles them, Enter goes back to
  snow) plus a picker for any color.
- **Cursor trail while it glides**: a fading smear from where the cursor was
  to where it is. The cursor still lands at 90 ms; the trail's tail catches
  up later, then nothing is drawn or scheduled. Needs animation on.
- **Trail weight** 1–5 (default 3): how long the tail lags (125–265 ms) and
  how opaque it is right behind the cursor (23–67 %). ←/→ adjusts, Enter
  resets to 3.
- **Trail while typing too**: each keystroke leaves a short trail. The
  cursor itself is already after the new letter; only the trail follows,
  and a new key continues the trail from where its tail was.
- Saved as `editor_font = "hack" | "jetbrains-mono"`, `block_cursor`,
  `cursor_animation`, `cursor_trail`, `cursor_trail_weight`,
  `cursor_trail_typing` and `cursor_color = "#rrggbb"` in
  `config.toml`; all off, Hack and snow by default.
