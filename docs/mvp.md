# delta — MVP Scope

## Goal

```
branch review TUI
powered by git diff
with lightweight AI feedback export
```

A human sits down, runs `delta main`, reviews what an AI agent changed on the branch, leaves notes, and gets a structured export they can hand back to the agent. That's the whole loop.

---

## MVP Features

- [x] `delta <from> [<to>]` invocation (e.g. `delta main`, `delta abc123 def456`)
- [x] Enumerate all changed files (added, modified, deleted, renamed)
- [x] File list panel with status indicators and color-coding
- [x] Per-file unified diff view with hunk display and line numbers
- [x] Keyboard navigation: lazygit-style (arrow keys, tab to switch panels, enter to select)
- [x] Status bar showing available keys at all times
- [x] Selected hunk indicator (`▶` marker + `N/M` counter in panel title)
- [x] Soft line wrapping (long lines wrap at panel boundary)
- [x] Context folding: `Space` collapses long context runs into a placeholder
- [x] Attach a multi-line note to a hunk (`c` to open, `Enter` for newlines, `Ctrl+D` to submit)
- [x] Edit existing note (`e`), delete existing note (`d`)
- [x] Submitted notes display inline in the diff (◎ marker)
- [x] On exit: export all notes as markdown to stdout
- [x] `--output <file>` flag to write directly to a file
- [x] `--json` flag to export as JSON instead
- [x] Spawns a terminal window when invoked without a TTY (e.g. from Claude Code `!` commands)

## Out of Scope for MVP (still post-MVP)

- Syntax highlighting
- Mouse support
- Multiple export targets / agent integrations
- Persistent review state across sessions
- Staging / unstaged change review (only committed diffs)
- Search within diffs
- Side-by-side diff view
- Virtual hunk splitting for pure-addition large files

---

## Success Criteria

Running `delta main` in a repo with changes:
1. Opens a TUI immediately
2. Shows all changed files in a navigable list
3. Shows the diff for the selected file
4. Allows leaving a note on any hunk
5. On quit, writes a markdown file with all notes, anchored to file + hunk + code context
6. Works when invoked from Claude Code via `! delta main` (spawns terminal, pipes output back)
