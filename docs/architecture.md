# delta — Architecture

## High-Level Pipeline

```
git diff <base>..HEAD
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
git diff <base>..HEAD --name-status       # enumerate changed files
git diff <base>..HEAD -- <file>           # per-file diff content
git merge-base <base> HEAD                # find fork point
git rev-parse --abbrev-ref HEAD           # current branch name
```

Implementation: shell-out initially; libgit2 as a possible future upgrade.

---

### B. Diff Engine Abstraction

Architecture supports pluggable engines via a trait:

```rust
trait DiffEngine {
    fn diff(&self, file: &ChangedFile, base: &str) -> Result<DiffFile>;
}
```

MVP implementation: parse `git diff` unified diff output directly.

Future possibilities:
- `similar` crate for inline character-level diffing
- Histogram diff
- Structural/semantic engines (difftastic, etc.)

---

### C. Internal Diff Representation (IR)

Critical design point: decouple rendering from the diff source.

```rust
struct DiffFile {
    old_path: PathBuf,
    new_path: PathBuf,
    status: FileStatus,   // Added, Modified, Deleted, Renamed
    hunks: Vec<Hunk>,
}

struct Hunk {
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
    hunk_header: String,      // e.g. "@@ -10,6 +10,8 @@"
    selected_lines: String,   // the specific lines the note is on
    note: String,             // the human's comment
}
```

Feedback exists only for the lifetime of a delta session. On exit it is written out and discarded.

---

### F. AI Export Layer

On session close, all feedback notes are serialized to one or both of:

**Markdown** (default, human-readable, paste-friendly):
```markdown
## src/auth.rs

**Hunk:** `@@ -42,6 +42,9 @@`
**Code:**
```rust
let token = refresh_token.to_string();
log::debug!("token: {}", token);
```
**Feedback:** Refresh token is logged in plaintext. Remove this log line and redact sensitive fields.
```

**JSON** (for programmatic consumption):
```json
{
  "notes": [
    {
      "file": "src/auth.rs",
      "hunk": "@@ -42,6 +42,9 @@",
      "lines": "let token = ...\nlog::debug!(...)",
      "note": "Refresh token is logged in plaintext..."
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
