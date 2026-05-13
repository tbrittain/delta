# delta — Architecture

## High-Level Pipeline

```
git diff <from>..<to>
    ↓
Changed file enumeration
    ↓
Per-file diff parsing
    ↓
Normalized internal diff representation (IR)
    ↓
Interactive TUI review session
    ↓
Human feedback capture
    ↓
Structured export for AI agents
```

---

## Components

### A. Git Integration Layer

Responsible for:
- Enumerating changed files between branch and base ref
- Fetching per-file diffs on demand
- Resolving the merge base

Key git commands used:
```bash
git diff <from>..<to> --name-status       # enumerate changed files
git diff <from>..<to> -- <file>           # per-file diff content
```

`<to>` defaults to `HEAD` when not specified.

Implementation: shell-out initially; libgit2 as a possible future upgrade.

---

### B. Git Backend Abstraction

All git operations are accessed through a trait, allowing tests to inject a fake implementation:

```rust
trait GitBackend {
    fn changed_files(&self, from: &str, to: &str) -> Result<Vec<ChangedFile>>;
    fn file_diff(&self, from: &str, to: &str, path: &str) -> Result<String>;
}
```

`SystemGit` is the production implementation that shells out to git. The unified diff output is parsed directly into the IR.

Future possibilities:
- `similar` crate for inline character-level diffing
- Structural/semantic engines (difftastic, etc.)

---

### C. Internal Diff Representation (IR)

Critical design point: decouple rendering from the diff source.

```rust
struct ChangedFile {
    path: PathBuf,
    status: FileStatus,   // Added, Modified, Deleted, Renamed
}

struct DiffFile {
    file: ChangedFile,
    hunks: Vec<Hunk>,
}

struct Hunk {
    header: String,       // e.g. "@@ -10,6 +10,8 @@"
    old_start: u32,
    new_start: u32,
    lines: Vec<DiffLine>,
}

struct DiffLine {
    old_lineno: Option<u32>,
    new_lineno: Option<u32>,
    kind: LineKind,        // Added, Removed, Context
    content: String,
}
```

The IR enables:
- Caching parsed diffs
- Alternate renderers
- Navigation and search
- Anchoring feedback notes
- Future engine swapping without touching the TUI

---

### D. Interactive Review TUI

Main responsibilities:
- File list panel (all changed files with status indicators)
- Diff view panel (hunks for selected file)
- Hunk navigation (jump to next/prev hunk)
- Context folding (collapse unchanged regions)
- Comment attachment (open input for a note on a hunk or line range)

Importantly: read-only review surface. delta never modifies files.

---

### E. Feedback Capture Layer

Feedback is attached to diff hunks or selected line ranges and is intentionally ephemeral — no persistent storage, no database.

```rust
struct FeedbackNote {
    file: PathBuf,
    hunk_header: String,   // e.g. "@@ -10,6 +10,8 @@"
    hunk_content: String,  // the diff lines of the hunk (+/- prefixed)
    note: String,          // the human's comment (may contain newlines)
}
```

Feedback exists only for the lifetime of a delta session. On exit it is written out and discarded.

---

### F. AI Export Layer

On session close, all feedback notes are serialized to one or both of:

**Markdown** (default, human-readable, paste-friendly):
```markdown
The following are code review notes from a human reviewer. Please address each item before proceeding.

---

## `src/auth.rs` · `@@ -42,6 +42,9 @@`

```diff
-    log::debug!("token: {}", token);
+    log::debug!("authenticated");
```

> **Human:** Refresh token is logged in plaintext. Remove this log line.

---
```

**JSON** (for programmatic consumption):
```json
{
  "notes": [
    {
      "file": "src/auth.rs",
      "hunk": "@@ -42,6 +42,9 @@",
      "code": "-    log::debug!(\"token: {}\", token);\n+    log::debug!(\"authenticated\");",
      "note": "Refresh token is logged in plaintext."
    }
  ]
}
```

Designed to feed: Claude Code, Codex, Aider, Cursor, custom agent workflows.

---

## Rendering Model

```
git diff output
    ↓
IR (DiffFile / Hunk / DiffLine)
    ↓
ratatui renderer
```

The renderer is never directly coupled to the diff source. This separation enables maintainability, testability, and future engine swapping.

---

## Performance Considerations

### Startup
- Near-instant launch — no runtime, no VM, no plugin loading
- Single static binary

### Lazy Evaluation
- Parse only the currently visible file's diff on load
- Background-parse remaining files
- Cache parsed IR per file

### Virtualized Rendering
- Only render visible lines and hunks
- Critical for large diffs, generated files, lockfiles
