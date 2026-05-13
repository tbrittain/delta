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

**Observed:** On Windows, files appear in the file list but the diff view shows "No diff content." even for files that have changes.

**Suspected causes (not yet diagnosed):**
- `git diff` output on Windows may use CRLF line endings in ways the parser doesn't handle (though Rust's `str::lines()` should strip them)
- `git` not on PATH in the spawned console's environment
- Path separator differences (`\` vs `/`) between what git reports and what delta expects
- `core.autocrlf = true` causing git to show no textual differences

**To investigate:** Run `git diff HEAD~ HEAD` directly in a Windows terminal and verify it produces output. If it does, the issue is in the parser or the environment delta inherits.

**Priority:** High — blocking on Windows.

---

### No support for staged/unstaged changes

By design. If this workflow is needed, commit the changes first or use `git stash` to create a commit-like snapshot.
