# ui.rs — features & planned improvements

## Current state

`src/ui.rs` owns the full TUI: event loop, all panel rendering (file list, diff view, notes panel, comment popup), cursor movement helpers, and clipboard I/O.

**Panels:**
- **File list** (left, 32 cols) — flat list of changed files with status indicators and note markers.
- **Diff view** (right, flex) — hunk-by-hunk diff with syntax highlighting, context folding, and inline note markers (◎). No line wrap; long lines clip at the panel edge.
- **Notes panel** (below diff, 10 rows, shown only when notes exist) — scrollable list of submitted notes with file + hunk context.
- **Comment popup** (modal overlay) — blinking-bar terminal cursor, selection highlighting, clipboard support (Ctrl+C/V/X), full cursor navigation (arrows, Home/End, Ctrl+word-jump, Shift-extend).

**Resize:** `Event::Resize` events cause an immediate redraw via the event loop's `continue` path. ratatui's `autoresize()` picks up the new dimensions in `terminal.draw()`.

**Line-wrap policy:** Diff code lines intentionally do NOT wrap (clipped at panel edge). The comment popup DOES wrap (`Wrap { trim: false }`) because prose notes should reflow. If horizontal scrolling in the diff view is ever desired, `Paragraph::scroll((v, h))` already supports it — just track a `diff_hscroll` in `App`.

---

## Planned improvements

### File list: directory tree view
**Goal:** Replace the flat file list with a proper tree view that mirrors the directory structure of the diff. Directories containing changed files can be expanded or collapsed. A collapsed directory shows a count (e.g. `src/ (3 files)`).

**Notes:**
- The data model already has full paths — grouping by directory prefix is the only structural change.
- Navigation (arrows, Enter) should work on both directory nodes and file leaves.
- Carry the `●` note marker up to directory nodes when any child file has notes.

### Find in diff / find in files (Ctrl+F)
**Goal:** Two search modes triggered by Ctrl+F, context-dependent on which panel is focused:

1. **Find in diff** (focused panel = DiffView) — incremental search across the visible diff content for the current file. Matching lines are highlighted; pressing Enter / n / N jumps between matches. Esc exits search and returns to normal navigation.
2. **Find in files** (focused panel = FileList) — filters the file list down to files whose paths match the search string. The list updates as the user types. Esc clears the filter and restores the full list.

**Notes:**
- Both modes should show the search input in the status bar (or a dedicated input line above it).
- Find-in-diff: match against the rendered diff line content (not raw bytes). Highlight matches with a distinct background colour distinct from the selection colour.
- Find-in-files: simple substring or glob match against the file path. Consider case-insensitive matching by default.

### Full-file view with collapsed unchanged sections
**Goal:** Allow viewing the entire file, not just the changed hunks. Unchanged sections between hunks are collapsed by default. The reviewer can expand any gap to read surrounding code (similar to GitHub's "Expand context" button).

**Current state:** We already have per-hunk context folding (`FOLD_THRESHOLD`, `toggle_hunk_fold`) that collapses long runs of unchanged lines *within* a hunk. This feature extends the same idea to *gaps between hunks* — those gaps are currently invisible.

**Notes:**
- Interleave "gap" placeholders between hunks with a line count. Space (or a dedicated key) on a gap placeholder loads and shows those lines.
- The git backend needs a method to fetch arbitrary line ranges for a file at a given ref (see `git.md`).
- Default view stays diff-only; full-file mode is opt-in per file.
