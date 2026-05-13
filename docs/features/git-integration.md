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

Running `delta` with no arguments errors. In practice the most common use case is reviewing the last commit (`HEAD^`), so typing the ref every time is friction.

**Possible directions:**
- Make `<from>` optional, defaulting to `HEAD^` — bare `delta` becomes `delta HEAD^`
- Alternative default: `HEAD~1` (same commit, clearer semantics)

**Priority:** Small quality-of-life improvement; trivial to implement.

### No support for staged/unstaged changes

By design. If this workflow is needed, commit the changes first or use `git stash` to create a commit-like snapshot.
