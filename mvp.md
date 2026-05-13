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

- [ ] `delta <base-ref>` invocation (e.g. `delta main`)
- [ ] Enumerate all changed files (added, modified, deleted, renamed)
- [ ] File list panel with status indicators
- [ ] Per-file unified diff view with hunk display
- [ ] Keyboard navigation: file list, hunk-to-hunk
- [ ] Attach a text note to a hunk
- [ ] On exit: export all notes as markdown to stdout or a file
- [ ] On exit: optionally export as JSON

## Out of Scope for MVP

- Difftastic / structural diffing
- Syntax highlighting
- Context folding (collapse unchanged lines)
- Mouse support
- Multiple export targets / agent integrations
- Persistent review state across sessions
- Staging / unstaged change review (only committed branch diffs)
- Search within diffs
- Side-by-side diff view

---

## Build Order

1. Project scaffold (`cargo new delta --bin`)
2. Git integration layer (enumerate files, fetch per-file diffs, parse unified diff into IR)
3. Basic ratatui app shell (event loop, quit keybind)
4. File list panel
5. Diff view panel for selected file
6. Hunk navigation
7. Comment/note input modal
8. Export layer (markdown + JSON)

---

## Success Criteria

Running `delta main` in a repo with a feature branch should:
1. Open a TUI immediately (< 200ms)
2. Show all changed files in a navigable list
3. Show the diff for the selected file
4. Allow leaving a note on any hunk
5. On quit, write a markdown file with all notes, anchored to file + hunk + code context
