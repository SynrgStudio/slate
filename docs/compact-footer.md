# Compact, contextual, configurable footer

The status row is independent of the command-input/history rows. Command-line
help also adapts to width: complete shortcut blocks disappear from the right
before they would be clipped. Hover shows the complete help. This applies to
idle/context hints and focused-but-empty input, not actual typed commands. Opening a file
now displays its basename rather than its full path. Hover the status row for a
details popup containing the full path, saved/modified state, cursor position,
line/word/character counts, mode and complete status message. Both footer and
command-help tooltips use monospace 12 pt and the footer's muted text color
(RGB 136,154,176), scoped locally without changing other UI typography.

The row has separate, clipped left and right regions. Text widths are measured
with the actual monospace font. Long filenames/messages are ellipsized; optional
right-hand fields are omitted in priority order as space decreases (hints first,
then counts and mode/wrap, preserving cursor position longest). Hidden fields
remain available in the details popup. No horizontal overlap is allowed.

A modified document uses an amber dot; a clean document shows green `saved`.
Normal `edit` mode is not repeated. Preview and transient modes can appear,
and search context shows current/total matches. Status messages appear for four
seconds after the status changes; the complete message remains in the popup.
This does not replace safety/conflict dialogs or alter dirty-state semantics.

## Settings → Footer

Ten persisted checkboxes follow Editor font size. ↑/↓ select, ← disables,
→ enables, Enter toggles, and mouse clicks toggle the checkbox. Visibility options:

| Config key | Default |
| --- | --- |
| `footer_filename` | true |
| `footer_state` | true |
| `footer_cursor` | true |
| `footer_mode` | true |
| `footer_wrap` | true |
| `footer_lines` | false |
| `footer_words` | false |
| `footer_characters` | false |
| `footer_shortcuts` | true |
| `footer_messages` | true |

Old configs without these keys use compact defaults; invalid booleans are ignored.
Even enabled optional fields may be hidden in a narrow window. All options can
be disabled; the command-input row remains unchanged.

The footer no longer scans the whole document for word/character counts every
frame. Counts are requested only when their options are enabled or the details
area is hovered, and cached by buffer identity/revision. This is derived metadata,
not file-content caching. Editing invalidates the counts; scrolling does not.

## Validation

212 tests pass; two manual large-document benchmarks are ignored by default.
Tooltip styling is tested for matching font/color and isolation from parent UI.
Command-hint tests check complete blocks and measured widths from 0 to 1100 pt.
New tests cover long paths/statuses with every option enabled at widths 180,
260, 420, 900 and 1600 points, disjoint left/right geometry, disabled fields,
config roundtrip/backward compatibility, keyboard toggles, last-row visibility in
a 420×260 Settings dialog, count caching and preservation of the document and
command buffer. Test settings writes use owned temporary directories.

Visual retest: open the large-document copy, resize the window, hover the footer,
toggle fields in Settings, search, save and wait four seconds. No measured
performance improvement is attributed to the metadata-cache change yet.
