# app.rs — features & planned improvements

## Current state

`src/app.rs` owns all application state and pure state-transition logic:

- **`App` struct** — files, diff content, highlights, scroll positions, selected hunk/note, mode, folding sets, comment scroll/anchor.
- **`Mode`** — `Normal` or `Comment { hunk_idx, input, cursor, original }`.
- **`FeedbackNote`** — a note attached to a hunk (file path, hunk header, hunk content, note text).
- **Selection helpers** — `selected_range`, `delete_selection` (pure, fully tested).
- **Scroll helpers** — `scroll_comment_to_cursor`, `scroll_to_selected_hunk`, `diff_content_lines`, `hunk_scroll_offset`.
- **Context folding** — `context_run_visual_lines`, `hunk_has_foldable_context`.

Notes are currently attached at **hunk granularity** — one `FeedbackNote` per hunk, keyed by `(file, hunk_header)`.

---

## Planned improvements

### Line-level comments (sub-hunk commenting)
**Goal:** Let the reviewer select specific changed lines within a hunk and attach a comment to that line range. The note should record the line number(s) and optionally highlight those lines in the diff view.

**Current state:** Comments attach to a whole hunk. Only one note per hunk is supported. `FeedbackNote` has no line-range field.

**Design sketch:**
- Add a "line-select mode" in the diff view (e.g. `v` to enter, arrow keys to extend, `c` to comment on the selection).
- `FeedbackNote` gains `line_range: Option<(u32, u32)>`.
- The inline marker (◎) anchors to the last selected line rather than the hunk footer.
- Export formats (markdown, JSON) include the line reference.
- Multiple notes per hunk become possible; the key changes from `(file, hunk_header)` to `(file, hunk_header, line_range)` or similar.

This is a significant data-model and UX change — design carefully before implementing.
