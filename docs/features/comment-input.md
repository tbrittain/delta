# Comment Input

## How it works

Press `c` on any hunk in the diff view to open the comment input. If the hunk already has a note, `c` redirects to edit mode pre-populated with the existing text.

The input opens as a **modal popup** overlaid on the diff view. The popup title shows the hunk header (`@@ -10,5 +10,6 @@`) so it is always clear which hunk is being commented on. A blinking bar (caret) cursor is used — it sits at the insertion point without displacing characters. Text wraps within the popup.

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

Once submitted, notes display inline in the diff with a `◎` marker (soft blue, italic). Long note lines are truncated with `…` at the diff panel edge. Multi-line notes render with `◎` on the first line and indented continuation lines.

### Editing and deleting existing notes

When the selected hunk has a note, the status bar shows `e: edit  d: delete` instead of `c: comment`.

- `e` — removes the existing note and re-opens the comment popup pre-populated with the old text
- `d` — deletes the note immediately
- `c` — also redirects to edit when a note exists (same as `e`)

`Esc` during an edit always restores the original note — cancelling never loses data.

---

## Known issues / open feedback

### No line-level (sub-hunk) commenting

Comments attach to a whole hunk. There is no way to select specific changed lines within a hunk and annotate only those lines.

**Design sketch:**
- `v` enters line-select mode in the diff view; arrow keys extend the selection; `c` opens the comment popup for that line range.
- The exported note would include the line range (e.g. `@@ -10,5 +10,6 @@ lines 12–14`).
- `FeedbackNote` gains a `line_range: Option<(u32, u32)>` field.
- Multiple notes per hunk become possible once the key includes the line range.
- The inline `◎` marker anchors to the last selected line rather than the hunk footer.

**Priority:** High value. Significant data-model and UX change — design carefully before implementing.

---

### Word wrap breaks words mid-word

When a line of text in the comment popup reaches the panel edge, the wrap currently splits at the character boundary — a long word is cut and continued on the next visual line. The preferred behaviour is **whole-word wrapping**: the entire word that would overflow is pushed down to the next line, matching standard editor and text-box conventions.

**Priority:** Low friction improvement, no data model changes needed. Affects only the visual line layout in the popup renderer.

---

### No mouse click-to-position in the comment popup

The cursor can be moved only by keyboard. Clicking inside the popup to reposition the cursor is not supported.

**Priority:** Post-MVP.
