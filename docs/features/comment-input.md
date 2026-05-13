# Comment Input

## How it works

Press `c` on any hunk in the diff view to open the comment input. If the hunk already has a note, `c` redirects to edit mode rather than opening a blank input.

The input appears inline below the hunk, prefixed with `▶`. A block cursor `█` shows the current insert position.

**Keys in comment mode:**

| Key | Action |
|---|---|
| `Enter` | Insert a newline (multi-line input) |
| `Ctrl+D` | Submit the comment |
| `Esc` | Cancel without saving |
| `←` / `→` | Move cursor left/right |
| `Backspace` | Delete the character before the cursor |

**Note:** `Ctrl+Enter` is indistinguishable from `Enter` in most terminal emulators. `Ctrl+D` ("done") is used for submission instead.

Once submitted, notes display inline in the diff with a `◎` marker. Multi-line notes render with the `◎` on the first line and indented continuation lines.

### Editing and deleting existing notes

When the selected hunk has a note, the status bar shows `e: edit  d: delete` instead of `c: comment`.

- `e` — removes the existing note and re-opens the comment input with the old text pre-populated and the cursor at the end
- `d` — deletes the note immediately
- `c` — also redirects to edit when a note exists (same as `e`)

---

## Known issues / open feedback

### Cursor can only move character by character

Left/Right arrows move one character at a time. There is no word-jump (`Ctrl+←`/`Ctrl+→`), no Home/End, and no mouse click-to-position. For long notes, editing content in the middle requires holding down the arrow key.

**Possible directions:**
- `Ctrl+Left`/`Ctrl+Right` for word-level jumps
- `Home`/`End` to jump to start/end of current line
- `Ctrl+A`/`Ctrl+E` (Unix readline conventions)

**Priority:** Medium. Workable but tedious on long notes.

---

### Enter keybind is counterintuitive for users expecting cancel

**Observed:** When editing a comment, pressing `Enter` expecting it to cancel/revert changes instead inserts a newline. Users coming from contexts where Enter = confirm/cancel may be surprised.

**Note:** Pressing `Esc` after `Enter` inserts a newline does not help recover — `Esc` cancels the whole session. This was previously a data-loss bug (Esc during an edit permanently deleted the original note); that bug is now fixed — `Esc` always restores the original note.

**Possible directions:**
- Better onboarding: ensure the status bar is prominent enough that `Ctrl+D: submit` is seen before the user presses Enter

**Priority:** Low — status bar documents the keybinds clearly. More of a first-use surprise.

---

### Comment input below viewport when hunk is at the bottom of the screen

**Observed:** When the selected hunk is near the bottom of the diff view, the inline comment input (which appears below the hunk's last line) renders outside the visible viewport. The first line of the input is cut off or invisible, and any wrapped continuation lines cannot be seen at all.

**Root cause:** Entering comment mode does not adjust `diff_scroll` to ensure the input area is visible. The `Paragraph` widget clips content below the viewport.

**Possible directions:**
- When `start_comment` is called, scroll the diff view down enough to show the comment input area (estimate based on hunk position + line count)
- Alternatively, reserve a fixed number of rows at the bottom of the diff panel when in comment mode

**Resolved:** When `c` or `e` is pressed, `scroll_to_show_comment_input` adjusts `diff_scroll` so the comment input is visible before the first render.

---

### No multi-line visual scrolling in comment input

The comment input renders inline in the diff view and does not scroll independently. For very long multi-line notes, the input area may push other content off screen.

**Priority:** Low for now.
