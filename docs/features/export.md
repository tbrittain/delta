# Export

## How it works

When you quit delta (`q`), all notes are exported. If there are no notes, delta exits silently.

### Output destination

By default, output goes to **stdout**:

```bash
delta main                          # prints to stdout
delta main > review.md              # redirect to a file
delta main --output review.md       # same, via flag
delta main --json                   # JSON format to stdout
delta main --json --output out.json # JSON to a file
```

### Markdown format (default)

```markdown
The following are code review notes from a human reviewer. Please address each item before proceeding.

---

## `src/auth.rs` · `@@ -42,6 +42,9 @@`

```diff
-    log::debug!("token: {}", token);
+    log::debug!("authenticated");
```

> **Human:** The refresh token was logged in plaintext.

---
```

Structure:
- Opening preamble directing the agent
- One `##` section per note, combining file path and hunk header
- `diff` code block showing the hunk content
- Blockquote with `> **Human:**` for the reviewer's note
- Multi-line notes: each continuation line gets its own `>` prefix

### JSON format (`--json`)

```json
{
  "notes": [
    {
      "file": "src/auth.rs",
      "hunk": "@@ -42,6 +42,9 @@",
      "code": "-    log::debug!(\"token: {}\", token);",
      "note": "The refresh token was logged in plaintext."
    }
  ]
}
```

### Using with Claude Code

Running `! delta HEAD^` from a Claude Code conversation spawns a terminal window (delta detects the missing TTY). When you quit the review session, the output is printed to stdout and captured by Claude Code, landing directly in the conversation.

If no terminal window appears, set `$TERMINAL`:

```bash
export TERMINAL=gnome-terminal   # or xterm, kitty, alacritty, etc.
```

---

## Known issues / open feedback

### No way to quit without exporting

Pressing `q` always exports notes if any exist. There is no keybind to discard the session and exit cleanly without producing output — useful when you've left scratch notes or changed your mind about the review.

**Possible directions:**
- `Q` (shift-q) quits without exporting
- A confirmation prompt when quitting with notes: "Export notes? (y/n)"
- `Ctrl+Q` for discard-and-quit

**Priority:** Low but useful. Workaround: quit and ignore/delete the output.

---

### No mechanism to suppress the hunk code block

The diff code block in the export gives the agent context, but for an agent that already has the full codebase available, it may be redundant noise. There is no flag to omit it.

**Possible directions:**
- `--no-code` flag to emit only the file, hunk header, and note
- Configurable template for the markdown format

**Priority:** Low. Code block is generally useful; easy to ignore.
