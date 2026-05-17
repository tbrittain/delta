# Diff View

## How it works

The right panel shows the unified diff for the currently selected file. Lines are color-coded by change type using background color:

- **Green background** (`+`) — added lines
- **Red background** (`-`) — removed lines
- **No background** (` `) — context lines (unchanged)

Language syntax is highlighted using the `syntect` crate (base16-ocean.dark theme, Sublime Text grammars) extended by `two-face` for languages not in the default Sublime bundle. The language is detected from the file extension; unknown extensions fall back to plain text. Syntax foreground colors are layered on top of the change-type background so both signals are visible simultaneously. Supported extensions include all syntect defaults plus TypeScript (`.ts`), TSX/JSX (`.tsx`, `.jsx`), TOML, Dockerfile, and 200+ others from `two-face`.

Line numbers appear in dark gray to the left of each line, showing the new file line number for added/context lines and the old file line number for removed lines.

### Hunk navigation

Each file's diff is divided into **hunks** — contiguous regions of change with surrounding context. Navigate between them with `[` (previous) and `]` (next).

`[` and `]` cross file boundaries: pressing `]` at the last hunk of a file advances to the first hunk of the next file; pressing `[` at the first hunk of a file jumps to the last hunk of the previous file. At the first or last file, the key is a no-op.

### Whitespace-sensitivity

Press `w` in the diff view to cycle through three whitespace modes:

| Mode | Flag | Effect |
|---|---|---|
| (none, default) | — | All changes shown |
| `-b` | `git diff -b` | Ignore changes in whitespace amount |
| `-w` | `git diff -w` | Ignore all whitespace |

The active mode appears in the diff panel title (e.g., `filename — 1/3 (-b)`) and in the status bar. Changing the mode immediately re-fetches and re-renders the diff for the current file.

### Scroll-position indicator

When the diff content is taller than the visible panel, a narrow scroll indicator appears on the right edge of the diff panel. The thumb position and size show the current viewport location relative to the total diff height.

The active hunk is marked with a bold cyan `▶` before its header. Non-selected hunks are indented two spaces to preserve alignment. The panel title shows `filename — N/M` so you always know your position.

### Character-level intraline diff

Within modified lines (consecutive removed/added pairs in a hunk), the specific characters that changed are highlighted with a brighter background — brighter green for added chars, brighter red for removed chars — layered on top of the line-level background and syntax colours.

Pairs are matched 1:1 by position within each removed/added run. Pure additions and pure deletions with no counterpart receive no intraline highlight. Context lines are never highlighted. The diffing uses character granularity (each Unicode scalar value is a diff unit), so single-character changes within multi-byte sequences are reported precisely.

### Context folding

Consecutive runs of 6 or more unchanged context lines are collapsed by default into a placeholder:

```
  ·· 12 lines of context ··
```

Press `Space` to expand a folded region; press `Space` again to fold it back. The status bar shows `Space: expand` or `Space: fold` when the current hunk has foldable content.

**Important:** Context folding operates _within_ a hunk on long runs of unchanged lines. The gaps _between_ hunks are lines git doesn't include in the diff at all — those are not accessible from within delta.

### Line wrapping

Long lines wrap at the panel boundary (`Wrap { trim: false }`). The gutter (line number + prefix) appears on the first visual row; continuation rows start at the left edge with no indent. There is no horizontal scrolling. The scroll accounting tracks visual rows (after wrapping) so hunk navigation and scroll-cap remain accurate regardless of line length.

### Side-by-side diff view

Press `s` in the diff view to toggle between inline (unified) and side-by-side layout. Inline is the default; `s` switches to split, and `s` again returns to inline.

In split view the diff panel is divided into two half-width columns separated by a `│` divider:

- **Left column** — old file content: removed lines and context lines
- **Right column** — new file content: added lines and context lines
- Each column has its own line-number gutter (4 digits + 1 space)
- Removed/added line runs are paired 1:1 by position; when one side has more lines than the other, the shorter side shows blank rows for the unpaired lines
- Intraline character-level highlighting is preserved on both sides
- Context folding works the same as in inline mode
- Hunk navigation (`[`/`]`) and scroll accounting correctly account for the taller side of each pair

The `[split]` label appears in the diff panel title while the split view is active.

---

## Planned improvements

### Full-file view with collapsed gaps
**Goal:** View the entire file, not just the changed hunks. Unchanged sections between hunks collapse by default with a line-count placeholder; expanding one loads those lines from git.

**Current:** Context folding already works *within* a hunk. This extends the same concept to *gaps between hunks*, which are currently invisible.

Default view stays diff-only; full-file is opt-in per file. See `git-integration.md` for the backend method needed to fetch arbitrary line ranges.

### Find in diff (Ctrl+F, diff view focused)
**Goal:** Incremental text search over the visible diff content. Matches are highlighted; `n` / `N` jumps between them. `Esc` clears.

**Notes:** Match against rendered line content (not raw bytes). Use a distinct highlight color separate from the selection color.

### Go to line (Ctrl+G)
**Goal:** A small modal (like the comment popup) where the reviewer types a line number. The diff view scrolls to and selects the hunk containing that line.

---

## Known issues / open feedback

### Large pure-addition files have no navigable sub-structure

When an entire file is new (e.g. a freshly committed README), git produces a single `@@ -0,0 +1,N @@` hunk containing only `+` lines. There are no context lines, so context folding does nothing. The entire file appears as one undivided hunk that is impossible to target at a specific section.

**Context:** First observed reviewing the README commit.

**Possible directions:**
- Virtual hunk splitting: break hunks above a threshold line count into navigable sub-hunks
- Sub-hunk selection: let the user mark a range of lines within a hunk before pressing `c`

**Priority:** Would significantly improve the experience for large new files. Requires meaningful changes to the IR and app state.

---


### No line-wrap toggle

The diff view always uses soft word wrap (`Wrap { trim: false }`). There is no way to disable wrapping and scroll horizontally instead, which some users prefer for wide diffs.

**Possible directions:**
- `w` key toggles between soft-wrap (current) and no-wrap + horizontal scroll (`←`/`→`)

**Priority:** Post-MVP. Soft wrap is a reasonable default; toggle adds flexibility without breaking anything.

---

