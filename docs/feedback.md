# delta — UX Feedback

Issues and observations collected from real usage. These are not bugs per se — more friction points and rough edges to address in future iterations.

---

## Single-hunk large files feel awkward

**Observed:** A file with many added lines (e.g. a new README) is treated as one single hunk. Navigating and commenting on it feels unwieldy — there's no way to target a specific section of the addition.

**Context:** Ran `delta HEAD^` after committing the README. The entire file was one `@@ -0,0 +1,N @@` hunk.

**Possible directions:**
- Context folding: collapse unchanged/uninteresting regions so large hunks feel smaller
- Sub-hunk selection: let the user highlight a range of lines within a hunk before commenting
- Virtual hunk splitting: break hunks above a certain line count into navigable chunks

**Priority:** Revisit after more testing.

---

## Selected hunk is not obvious

**Observed:** There is no clear visual indicator showing which hunk is currently "active" for commenting. The hunk header at the top of the diff panel does reflect the selection, but this wasn't immediately noticeable — it requires the user to look away from where they're navigating.

**Possible directions:**
- Highlight the entire selected hunk's lines with a subtle background tint (e.g. dark blue/gray background on the hunk block)
- Add a visible gutter marker (e.g. `▶`) beside the selected hunk header
- Show a persistent "Hunk N of M" indicator in the status bar or panel title

**Resolved:** Added bold yellow `▶` marker to the selected hunk header; non-selected hunks are indented with two spaces to maintain alignment. Panel title now shows `filename — N/M` hunk position at all times.

---

## No line wrapping in diff view

**Observed:** Long lines extend beyond the panel width and are cut off. There is no horizontal scrolling or wrapping.

**Possible directions:**
- Soft-wrap long lines within the panel width
- Horizontal scroll support
- Truncate with a visible `…` indicator and allow horizontal scroll to reveal the rest

**Resolved:** Enabled soft wrapping (`Wrap { trim: false }`) so long lines wrap at the panel boundary with indentation preserved. Note: scroll offset is based on logical line count, so hunk-jump positioning may be slightly imprecise on files with many very long lines.

---

## Side-by-side diff view

**Observed:** The diff is currently inline (unified diff style). A side-by-side (split) view would be useful for reviewing modifications where seeing old vs new simultaneously helps.

**Possible directions:**
- Add a toggle (e.g. `s`) to switch between inline and side-by-side modes
- Side-by-side would split the diff panel further: old on left, new on right

**Priority:** Post-MVP. Non-trivial to implement; requires significant layout and IR changes.

---

## Cannot edit or delete existing comments

**Observed:** Once a comment is submitted, there is no way to modify or remove it. The only recourse is to quit and restart the session.

**Possible directions:**
- Navigate to a submitted note (◎ marker) and press `e` to edit or `d` to delete
- Show a notes list view accessible via a keybind

**Resolved:** `e` re-opens the comment input pre-populated with the existing text; `d` deletes it immediately. Status bar shows these hints contextually when the current hunk has a note.

---

## Export markdown format needs rethinking

**Observed:** The exported markdown has issues:
- The `# Delta Review` title is fluff — adds no value when pasting into an agent context
- `**Feedback:**` as a label is too neutral; should make clear this is human/reviewer feedback
- No framing at the top to tell the agent what it's looking at or what to do with it

**Possible directions:**
- Remove or make the title optional
- Replace `**Feedback:**` with `**Reviewer note:**` or `**Human:**`
- Add a brief preamble: e.g. "The following are code review notes from a human reviewer. Please address each item." (possibly configurable or suppressible)
- Reconsider whether including the full hunk code block adds value or is noise for the agent

**Resolved:** Title replaced with a plain preamble directing the agent. File and hunk merged onto one header line. Code block uses `diff` fence for syntax highlighting. Human note rendered as a blockquote (`> **Human:** ...`) to visually separate it from the diff content.

---

## Cannot diff between two arbitrary commits

**Observed:** `delta` always diffs against HEAD. There is no way to compare two non-HEAD commits directly (e.g. `delta <commit-A> <commit-B>`). The workaround is a temporary `git worktree` pointed at one of the commits, then running delta from inside it with the other as the base ref.

**Context:** Wanted to run delta over the range `42ce9f2..1aaaf25` (scaffold → refactoring pivot) as a real-world test case.

**Possible directions:**
- Add an optional second argument: `delta <from> <to>` diffs between two arbitrary refs
- Keep the current single-ref model but document the worktree workaround

**Resolved:** Added optional second positional arg — `delta <from> <to>`. When omitted, `<to>` defaults to `HEAD` so existing invocations are unchanged.

---

## Syntax highlighting in diff view

**Observed:** Diff lines are colored by change type (green/red/gray) but not by language syntax. Keywords, strings, and identifiers all render in the same flat color, making dense code harder to scan.

**Possible directions:**
- Integrate the `syntect` crate (Sublime Text grammars, pure Rust) to highlight tokens within each diff line, layered on top of the existing add/remove color
- Detect language from file extension; fall back gracefully for unknown types
- Highlight only added/context lines since removed lines are less critical to read carefully

**Priority:** Post-MVP. Meaningful readability improvement, not blocking core workflow.
