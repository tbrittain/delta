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

## Cannot diff between two arbitrary commits

**Observed:** `delta` always diffs against HEAD. There is no way to compare two non-HEAD commits directly (e.g. `delta <commit-A> <commit-B>`). The workaround is a temporary `git worktree` pointed at one of the commits, then running delta from inside it with the other as the base ref.

**Context:** Wanted to run delta over the range `42ce9f2..1aaaf25` (scaffold → refactoring pivot) as a real-world test case.

**Possible directions:**
- Add an optional second argument: `delta <from> <to>` diffs between two arbitrary refs
- Keep the current single-ref model but document the worktree workaround

**Priority:** Revisit after more testing.
