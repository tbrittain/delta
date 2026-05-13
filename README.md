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

## Installation

### Option 1 — Install script (Linux and macOS)

```bash
curl -sSf https://raw.githubusercontent.com/tbrittain/delta/main/install.sh | bash
```

Installs the latest release binary to `~/.local/bin`. To install elsewhere:

```bash
curl -sSf https://raw.githubusercontent.com/tbrittain/delta/main/install.sh | bash -s -- --install-dir /usr/local/bin
```

### Option 2 — Download a release binary

Download the binary for your platform from the [Releases page](https://github.com/tbrittain/delta/releases/latest) and place it on your PATH.

| Platform | File |
|---|---|
| Linux x86_64 | `delta-vX.Y.Z-x86_64-unknown-linux-gnu.tar.gz` |
| Linux ARM64 | `delta-vX.Y.Z-aarch64-unknown-linux-gnu.tar.gz` |
| macOS Intel | `delta-vX.Y.Z-x86_64-apple-darwin.tar.gz` |
| macOS Apple Silicon | `delta-vX.Y.Z-aarch64-apple-darwin.tar.gz` |
| Windows x86_64 | `delta-vX.Y.Z-x86_64-pc-windows-msvc.zip` |

### Option 3 — Build from source

Requires the [Rust toolchain](https://rustup.rs) and Git.

```bash
git clone https://github.com/tbrittain/delta.git
cargo install --path delta
```

---

## Usage

```bash
delta <base-ref>
```

**Examples:**

```bash
delta                   # latest commit (HEAD~ vs HEAD) — the default
delta main              # current branch vs main
delta origin/main       # current branch vs remote main
delta HEAD^             # same as bare delta
delta HEAD~3            # changes across the last 3 commits
delta abc1234           # current branch vs a specific commit hash
delta HEAD^2            # current branch vs the second parent of a merge commit
delta abc1234 def5678   # diff between two arbitrary commits
delta main feature      # diff between two branch tips
```

Both `<from>` and `<to>` accept anything git understands as a commit reference. `<from>` defaults to `HEAD~` and `<to>` defaults to `HEAD` when omitted.

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

### Notes panel (Tab from diff view when notes exist)

| Key | Action |
|---|---|
| `↑` / `↓` | Navigate between notes |
| `Enter` | Jump to the note's file and hunk in the diff view |
| `Space` | Expand / collapse the full note text |
| `e` | Jump to the note's hunk and enter edit mode |
| `d` | Delete the selected note |
| `Tab` | Return to the diff view |

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
