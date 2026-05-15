# Diff View

## How it works

The right panel shows the unified diff for the currently selected file. Lines are color-coded by change type using background color:

- **Green background** (`+`) — added lines
- **Red background** (`-`) — removed lines
- **No background** (` `) — context lines (unchanged)

Language syntax is highlighted using the `syntect` crate (base16-ocean.dark theme, Sublime Text grammars). The language is detected from the file extension; unknown extensions fall back to plain text. Syntax foreground colors are layered on top of the change-type background so both signals are visible simultaneously.

Line numbers appear in dark gray to the left of each line, showing the new file line number for added/context lines and the old file line number for removed lines.

### Hunk navigation

Each file's diff is divided into **hunks** — contiguous regions of change with surrounding context. Navigate between them with `[` (previous) and `]` (next).

The active hunk is marked with a bold cyan `▶` before its header. Non-selected hunks are indented two spaces to preserve alignment. The panel title shows `filename — N/M` so you always know your position.

### Context folding

Consecutive runs of 6 or more unchanged context lines are collapsed by default into a placeholder:

```
  ·· 12 lines of context ··
```

Press `Space` to expand a folded region; press `Space` again to fold it back. The status bar shows `Space: expand` or `Space: fold` when the current hunk has foldable content.

**Important:** Context folding operates _within_ a hunk on long runs of unchanged lines. The gaps _between_ hunks are lines git doesn't include in the diff at all — those are not accessible from within delta.

### Line wrapping

Long lines wrap at the panel boundary (`Wrap { trim: false }`). The gutter (line number + prefix) appears on the first visual row; continuation rows start at the left edge with no indent. There is no horizontal scrolling. The scroll accounting tracks visual rows (after wrapping) so hunk navigation and scroll-cap remain accurate regardless of line length.

---

## Planned improvements

### Whitespace-sensitivity flags (`-w` / `-b`)
**Goal:** Let the reviewer control whether whitespace-only changes appear in the diff.

- `git diff -w` — ignores all whitespace; lines differing only in spacing become invisible.
- `git diff -b` — ignores whitespace *changes* but not the presence/absence of whitespace. Less aggressive than `-w`.

**UX:** `w` key cycles `none → -b → -w → none`. The active flag appears in the diff panel title. Changing it re-fetches the file diff with the flag appended to the `git diff` invocation. See `git-integration.md` for the backend changes needed.

### Full-file view with collapsed gaps
**Goal:** View the entire file, not just the changed hunks. Unchanged sections between hunks collapse by default with a line-count placeholder; expanding one loads those lines from git.

**Current:** Context folding already works *within* a hunk. This extends the same concept to *gaps between hunks*, which are currently invisible.

Default view stays diff-only; full-file is opt-in per file. See `git-integration.md` for the backend method needed to fetch arbitrary line ranges.

### Find in diff (Ctrl+F, diff view focused)
**Goal:** Incremental text search over the visible diff content. Matches are highlighted; `n` / `N` jumps between them. `Esc` clears.

**Notes:** Match against rendered line content (not raw bytes). Use a distinct highlight color separate from the selection color.

### Scroll-position indicator
**Goal:** A narrow indicator on the right edge of the diff panel showing the current viewport position relative to total diff content — like a scrollbar or minimap. Gives spatial context when navigating long diffs.

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

### No side-by-side diff view

The diff is inline (unified diff style). Some reviewers find side-by-side easier for reading modifications where old and new should be compared directly.

**Possible directions:**
- Toggle with `s` to switch between inline and split view
- Split view would divide the diff panel: old content left, new content right

**Priority:** Post-MVP. Non-trivial — requires significant layout and rendering changes.

---

### No word-level diff highlighting

Lines are highlighted by change type but not at the word or token level. On a modified line, the entire line is red or green; the specific words that changed are not called out.

**Possible directions:**
- `similar` crate (pure Rust) for word-level diff between the old and new version of each changed line, rendered as intra-line highlights

**Priority:** Post-MVP. Complements syntax highlighting; best implemented alongside or after it.

---

### No line-wrap toggle

The diff view always uses soft word wrap (`Wrap { trim: false }`). There is no way to disable wrapping and scroll horizontally instead, which some users prefer for wide diffs.

**Possible directions:**
- `w` key toggles between soft-wrap (current) and no-wrap + horizontal scroll (`←`/`→`)

**Priority:** Post-MVP. Soft wrap is a reasonable default; toggle adds flexibility without breaking anything.

---

### ~~Syntax highlighting missing for TypeScript and JSX/TSX~~ _(fixed in 0.4.0)_

Resolved by adding the `two-face` crate, which extends syntect's default grammar set with 200+ extra syntaxes. `.ts`, `.tsx`, and `.jsx` are now fully highlighted using TypeScript grammars. `.vue`, `.svelte`, TOML, Dockerfile, and many others also benefit. The extra set is checked before the syntect default set in `highlight.rs`.
