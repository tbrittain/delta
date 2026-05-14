# Git Integration

## How it works

delta shells out to `git diff` to enumerate changed files and fetch per-file diffs. It does not use libgit2 or any in-process git library.

### Invocation

```bash
delta <from> [<to>]
```

`<to>` defaults to `HEAD` when omitted. Both arguments accept any git ref — branch names, commit hashes, `HEAD^`, `HEAD~3`, `origin/main`, etc.

```bash
delta main              # current branch vs main
delta HEAD^             # latest commit only
delta abc1234 def5678   # two arbitrary commits
delta main feature      # two branch tips
```

### What delta diffs

delta compares **committed history only**. Staged or unstaged working tree changes are out of scope. Commit your changes first, then run delta.

### File statuses

Files are shown with status indicators:

| Indicator | Meaning | Color |
|---|---|---|
| `[A]` | Added | Green |
| `[M]` | Modified | Yellow |
| `[D]` | Deleted | Red |
| `[R]` | Renamed | Cyan |

For renamed files, delta displays the **new path**. The old path is not shown.

### No-TTY behaviour

When delta is invoked without an interactive terminal (e.g. from Claude Code's `!` command runner), it detects the missing TTY via `IsTerminal` and spawns a new terminal window running itself. The parent process blocks until the window closes, then reads the review output and prints it to stdout for the caller to capture.

Terminal emulators tried in order: `$TERMINAL` env var, `xterm`, `kitty`, `alacritty`, `gnome-terminal`, `konsole`, `xfce4-terminal`.

---

## Known issues / open feedback

### Renamed file shows new path only

The file list shows `src/renamed.rs` for a rename, with no indication of what the old name was. A reviewer unfamiliar with the codebase may not know what changed.

**Possible directions:**
- Show `src/old_name.rs → src/renamed.rs` in the file list

**Priority:** Low. Easy to implement; minor UX improvement.

### No default invocation — `<from>` is required

**Resolved:** `<from>` now defaults to `HEAD~`, so bare `delta` reviews the latest commit. `<to>` continues to default to `HEAD`.

### Windows: diff view shows "No diff content"

**Resolved:** Fixed in `d2594c4` by passing `--no-ext-diff` to `git diff`, forcing git's built-in unified diff output regardless of any external difftool configured by the user (`diff.tool`, `diff.external`, etc.). External difftool output is not in unified diff format and was silently producing no parseable content.

---

### No support for staged/unstaged changes

By design. If this workflow is needed, commit the changes first or use `git stash` to create a commit-like snapshot.

---

### Future: in-process git via git2

delta currently shells out to `git` for all operations. The `git2` crate (libgit2 bindings) would allow in-process git operations — faster, no PATH dependency, richer access to repository internals. Not needed until the subprocess approach becomes a bottleneck or limitation.

### Future: structural diffing via Difftastic

Difftastic produces AST-aware diffs that ignore formatting noise and understand language structure. No stable machine-readable output format exists yet (as of 2025), so integration would require parsing ANSI output or an upstream contribution. Worth revisiting if Difftastic gains a structured output mode.
