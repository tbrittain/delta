# git.rs — features & planned improvements

## Current state

`src/git.rs` provides the `GitBackend` trait and its real implementation `SystemGit`. It shells out to `git` for two operations:

- `changed_files(from, to)` — runs `git diff --name-status` to enumerate changed files.
- `file_diff(from, to, path)` — runs `git diff` for a single file to produce the unified diff text fed into the parser.

Pure helpers tested separately: `parse_name_status`.

---

## Planned improvements

### Arbitrary line-range fetch (for full-file view)
**Goal:** Add a method to fetch a contiguous range of lines from a file at a given ref, without requiring a diff. Needed by the full-file view feature in `ui.md`.

**Sketch:**
```rust
fn file_lines(at: &str, path: &str, start: u32, end: u32) -> Result<String>;
```

Implementation: `git show <ref>:<path>` piped through `sed -n '<start>,<end>p'` or equivalent. On Windows, use a Rust line-range extractor rather than shell tools for portability.
