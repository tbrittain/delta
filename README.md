# delta

A terminal-based diff review tool for AI-assisted development workflows.

When an AI agent finishes a coding task and checks in with you, delta gives you a structured way to review its changes, leave targeted feedback on specific hunks of code, and export that feedback in a format you can hand directly back to the agent.

---

## The Problem

Reviewing AI-generated code changes typically looks like this:

1. Agent makes changes and checks in
2. You hunt through files manually to find what changed
3. You copy file paths and write a long unstructured message back
4. Agent iterates — repeat

Steps 2–4 are friction. delta replaces them with a focused review session that produces structured output.

---

## Workflow

```
delta <from> [<to>]
```

Opens a terminal UI showing all files changed between `<from>` and `<to>` (defaults to HEAD). You navigate the diff, leave notes on specific hunks, and when you quit, delta writes your feedback — anchored to file, hunk, and code context — to stdout (or a file). Feed that output back to the agent.

---

## Prerequisites

- Rust toolchain (`rustup` recommended — [rustup.rs](https://rustup.rs))
- Git

---

## Building

```bash
cargo build --release
```

The binary is at `target/release/delta`.

**To install to your PATH**, run from the repository root:

```bash
cargo install --path /path/to/delta
```

For example, if you cloned to `~/source/delta`:

```bash
cargo install --path ~/source/delta
```

This installs the `delta` binary to `~/.cargo/bin/`, which is on your PATH if you followed the rustup installation. After that, `delta` works from any directory.

---

## Usage

```bash
delta <base-ref>
```

**Examples:**

```bash
delta main              # current branch vs main (to = HEAD by default)
delta origin/main       # current branch vs remote main
delta HEAD^             # changes in the latest commit only
delta HEAD~3            # changes across the last 3 commits
delta abc1234           # current branch vs a specific commit hash
delta HEAD^2            # current branch vs the second parent of a merge commit
delta abc1234 def5678   # diff between two arbitrary commits
delta main feature      # diff between two branch tips
```

`<from>` and `<to>` accept anything git understands as a commit reference. `<to>` defaults to `HEAD` when omitted.

**Note:** delta compares committed history only. Staged or unstaged working tree changes are not in scope — commit your changes first, then run delta.

**Options:**

| Flag | Description |
|---|---|
| `--output <file>` | Write feedback to a file instead of stdout |
| `--json` | Export as JSON instead of markdown |

**Export to a file:**

```bash
delta main --output review.md
```

**Pipe output directly into a Claude Code conversation:**

Run delta via Claude Code's `!` command prefix. Because delta is a TUI, it will automatically open a new terminal window for the review session. When you quit, the output is piped back into the conversation:

```bash
! delta main
```

Leave your notes in the terminal window, press `q` to quit, and the review appears in the chat for Claude to act on immediately.

To save to a file instead:

```bash
! delta main --output review.md
```

If no terminal window appears, set `$TERMINAL` to your preferred emulator:

```bash
export TERMINAL=gnome-terminal   # or xterm, kitty, alacritty, etc.
```

---

## Key Bindings

### File list (left panel)

| Key | Action |
|---|---|
| `↑` / `↓` | Navigate files |
| `Enter` | Open file and switch to diff view |
| `Tab` | Switch to diff panel |
| `q` | Quit |

### Diff view (right panel)

| Key | Action |
|---|---|
| `↑` / `↓` | Scroll |
| `[` / `]` | Previous / next hunk |
| `c` | Add comment to current hunk |
| `e` | Edit existing comment on current hunk |
| `d` | Delete existing comment on current hunk |
| `Space` | Expand / fold context lines in current hunk |
| `Tab` | Switch to file list |
| `q` | Quit and export feedback |

The panel title shows the current file and hunk position (`filename — 2/5`). The status bar shows `e: edit  d: delete` when the current hunk already has a comment, and `c: comment` otherwise.

### Comment input

| Key | Action |
|---|---|
| `Enter` | New line |
| `Ctrl+D` | Submit comment |
| `Esc` | Cancel |

---

## Export Format

### Markdown (default)

```markdown
The following are code review notes from a human reviewer. Please address each item before proceeding.

---

## `src/auth.rs` · `@@ -42,6 +42,9 @@`

\```diff
-    log::debug!("token: {}", token);
+    log::debug!("authenticated");
\```

> **Human:** The refresh token was being logged in plaintext. Make sure no other sensitive fields are logged nearby.

---
```

### JSON (`--json`)

```json
{
  "notes": [
    {
      "file": "src/auth.rs",
      "hunk": "@@ -42,6 +42,9 @@",
      "code": "-    log::debug!(\"token: {}\", token);\n+    log::debug!(\"authenticated\");",
      "note": "The refresh token was being logged in plaintext."
    }
  ]
}
```

---

## What delta is not

- A text editor — diffs are read-only
- A replacement for GitHub/GitLab code review
- A persistent review platform — feedback is ephemeral, tied to one session
- A merge tool

---

## Status

Early development. Core review workflow is functional; rough edges may exist.
