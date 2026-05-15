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

## Planned improvements

### Arbitrary line-range fetch
Needed by the full-file view feature. Goal: fetch a contiguous range of lines from a file at a given git ref, without running a diff.

```rust
fn file_lines(at: &str, path: &str, start: u32, end: u32) -> Result<String>;
```

Implementation: `git show <ref>:<path>` then extract the requested lines in Rust (no shell `sed`/`head` — keep it portable on Windows).

---

## Known issues / open feedback

### Renamed file display

The file list shows `old_name.rs → new_name.rs` for renamed files. Both the old and new filenames are shown so a reviewer can immediately see what changed. The old path is captured from the `R\told\tnew` columns that `git diff --name-status` emits and stored in `ChangedFile.old_path`.

### Windows: diff view shows "No diff content"

**Resolved:** Fixed in `d2594c4` by passing `--no-ext-diff` to `git diff`, forcing git's built-in unified diff output regardless of any external difftool configured by the user. A configured external difftool produces output in a different format that the parser cannot handle. `--no-ext-diff` bypasses any such configuration and always uses git's built-in unified diff.

---

### No support for staged/unstaged changes

By design. If this workflow is needed, commit the changes first or use `git stash` to create a commit-like snapshot.

---

### Future: in-process git via git2

delta currently shells out to `git` for all operations. The `git2` crate (libgit2 bindings) would allow in-process git operations — faster, no PATH dependency, richer access to repository internals. Not needed until the subprocess approach becomes a bottleneck or limitation.

### Note on deeper diff algorithms

The `similar` crate covers the depth needed here — it provides multiple diff algorithms (LCS, patience, Myers) in pure Rust and is sufficient for word-level and line-level highlighting. No deeper diff engine is required.
