# Diff View

## How it works

The right panel shows the unified diff for the currently selected file. Lines are color-coded:

- **Green** (`+`) — added lines
- **Red** (`-`) — removed lines
- **Gray** (` `) — context lines (unchanged)

Line numbers appear in dark gray to the left of each line, showing the new file line number for added/context lines and the old file line number for removed lines.

### Hunk navigation

Each file's diff is divided into **hunks** — contiguous regions of change with surrounding context. Navigate between them with `[` (previous) and `]` (next).

The active hunk is marked with a bold yellow `▶` before its header. Non-selected hunks are indented two spaces to preserve alignment. The panel title shows `filename — N/M` so you always know your position.

### Context folding

Consecutive runs of 6 or more unchanged context lines are collapsed by default into a placeholder:

```
  ·· 12 lines of context ··
```

Press `Space` to expand a folded region; press `Space` again to fold it back. The status bar shows `Space: expand` or `Space: fold` when the current hunk has foldable content.

**Important:** Context folding operates _within_ a hunk on long runs of unchanged lines. The gaps _between_ hunks are lines git doesn't include in the diff at all — those are not accessible from within delta.

### Line wrapping

Long lines wrap at the panel boundary with indentation preserved (`Wrap { trim: false }`). There is no horizontal scrolling. Scroll offsets are based on logical line count, so hunk-jump positioning may be slightly imprecise on files with many very long lines.

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

### No syntax highlighting

Diff lines are colored by change type (green/red/gray) but not by language syntax. Keywords, strings, and identifiers all render in the same flat color, making dense code harder to scan.

**Possible directions:**
- `syntect` crate (Sublime Text grammars, pure Rust) for token-level highlighting layered on top of change-type color
- Detect language from file extension; fall back gracefully for unknown types

**Priority:** Post-MVP. Meaningful readability improvement, not blocking core workflow.

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
