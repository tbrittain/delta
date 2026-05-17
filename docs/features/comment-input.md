# Comment Input

## How it works

Press `c` on any hunk in the diff view to open the comment input. If the hunk already has a whole-hunk note, `c` redirects to edit mode pre-populated with the existing text.

The input opens as a **modal popup** overlaid on the diff view. The popup title shows the hunk header (`@@ -10,5 +10,6 @@`) so it is always clear which hunk is being commented on. A blinking bar (caret) cursor is used — it sits at the insertion point without displacing characters. Text wraps within the popup using whole-word wrapping: when a word would overflow the panel edge, the entire word is pushed to the next visual line. If a single word exceeds the panel width it is broken at the character boundary. The space at a word-break boundary is included in the preceding row's text so that selection highlighting covers it without gaps.

**Keys in comment mode:**

| Key | Action |
|---|---|
| `Enter` | Insert a newline |
| `Ctrl+S` | Submit the comment |
| `Esc` | Cancel — restores the original note if editing |
| `←` / `→` | Move cursor one character |
| `↑` / `↓` | Move cursor one visual line (handles wrapped text) |
| `Home` / `End` | Jump to start/end of current logical line |
| `Ctrl+←` / `Ctrl+→` | Word-level jump |
| `Ctrl+Shift+←` / `Ctrl+Shift+→` | Word-level jump with selection |
| `Shift+arrow` | Extend selection |
| `Ctrl+A` | Select all |
| `Ctrl+C` | Copy selection to clipboard |
| `Ctrl+V` | Paste from clipboard (replaces selection if active) |
| `Ctrl+X` | Cut selection to clipboard |
| `Backspace` / `Delete` | Delete char before/after cursor; deletes selection if active |

Typing any character while text is selected replaces the selection (standard editor behaviour).

`Ctrl+C` is captured in raw terminal mode (crossterm disables signal processing), so it does not send SIGINT — it copies.

Once submitted, notes display inline in the diff with a `◎` marker (soft blue, italic). Long note lines are truncated with `…` at the diff panel edge. Multi-line notes render with `◎` on the first line and indented continuation lines. Line-level notes anchor the `◎` marker immediately after the last selected line; whole-hunk notes appear at the end of the hunk.

### Line-level (sub-hunk) commenting

Press `v` on any hunk to enter **line-select mode**. A blue selection highlight shows the current selection (starts at line 0 of the hunk). Use `↑`/`↓` to extend the selection to the desired line range, then `c` to comment on just those lines.

**Keys in line-select mode:**

| Key | Action |
|---|---|
| `↑` / `↓` | Move the selection cursor (anchor stays at line 0) |
| `c` | Open the comment popup for the selected line range |
| `d` | Delete the note on the selected range (if one exists) |
| `Esc` | Return to normal mode |

Multiple notes per hunk are allowed when each targets a different line range. The note identity is `(file, hunk_header, line_range)`.

**Exported format:**
- Markdown heading includes the range: `` ## `src/main.rs` · `@@ -10,5 +10,6 @@` · L12–14 ``
- JSON includes an optional `lines` field: `{ "start": 12, "end": 14 }` — omitted for whole-hunk notes.
- `code` in the export contains only the selected lines (not the full hunk).

### Editing and deleting existing notes

When the selected hunk has a **whole-hunk** note, the status bar shows `e: edit  d: delete` instead of `c: comment`.

- `e` — removes the existing note and re-opens the comment popup pre-populated with the old text
- `d` — deletes the note immediately
- `c` — also redirects to edit when a note exists (same as `e`)

For **line-level** notes: enter line-select mode (`v`) and position the selection on the note's range, then `c` (opens in edit mode) or `d` (deletes immediately).

`Esc` during an edit always restores the original note — cancelling never loses data.

---

## Known issues / open feedback

---

### No mouse click-to-position in the comment popup

The cursor can be moved only by keyboard. Clicking inside the popup to reposition the cursor is not supported.

**Priority:** Post-MVP.
