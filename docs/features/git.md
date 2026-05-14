# git.rs — features & planned improvements

## Current state

`src/git.rs` provides the `GitBackend` trait and its real implementation `SystemGit`. It shells out to `git` for two operations:

- `changed_files(from, to)` — runs `git diff --name-status` to enumerate changed files.
- `file_diff(from, to, path)` — runs `git diff` for a single file to produce the unified diff text fed into the parser.

Pure helpers tested separately: `parse_name_status`.

---

## Planned improvements

### Whitespace-sensitivity flags (`-w` / `-b`)
**Goal:** Let the reviewer control how the diff is computed — specifically whether whitespace changes are included. Two levels are useful:

- **`-w` (ignore all whitespace):** `git diff -w` — lines that differ only in whitespace are treated as unchanged. Useful for reviewing content changes after reformatting.
- **`-b` (ignore whitespace changes):** `git diff -b` — whitespace changes are ignored but the presence/absence of whitespace is kept. Less aggressive than `-w`.

**UX sketch:**  
A toggle in the diff view (e.g. `w` key cycles None → `-b` → `-w` → None). The active flag is shown in the diff panel title. Changing the flag re-runs `file_diff` for the current file with the new flag appended to the `git diff` command.

**Notes:**
- `GitBackend::file_diff` needs to accept an optional `DiffFlags` parameter (or a `DiffOptions` struct).
- The `App` struct would carry a `diff_whitespace: WhitespaceMode` field (enum).
- Integration tests should cover each mode against a fixture with whitespace-only changes.
- Blank-line changes (`--ignore-blank-lines` / `-B`) could be a third mode if desired.

### Arbitrary line-range fetch (for full-file view)
**Goal:** Add a method to fetch a contiguous range of lines from a file at a given ref, without requiring a diff. Needed by the full-file view feature in `ui.md`.

**Sketch:**
```rust
fn file_lines(at: &str, path: &str, start: u32, end: u32) -> Result<String>;
```

Implementation: `git show <ref>:<path>` piped through `sed -n '<start>,<end>p'` or equivalent. On Windows, use a Rust line-range extractor rather than shell tools for portability.
