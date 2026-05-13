# delta — Progress Tracker

This document tracks implementation status. Check items off as they are completed.

---

## Project Setup
- [x] Git repository
- [x] Rust project scaffold (`cargo init`)
- [x] Dependencies: ratatui, crossterm, clap, anyhow, serde/serde_json
- [x] CLAUDE.md with coding standards
- [x] Docs directory with concept, architecture, tech-stack, mvp
- [x] GitHub remote (tbrittain/delta, private)

---

## Core Data Layer
- [x] IR types: `ChangedFile`, `DiffFile`, `Hunk`, `DiffLine`, `LineKind`, `FileStatus`
- [x] Unified diff parser (`parse_diff`, `parse_hunk_header`)
- [x] `GitBackend` trait + `SystemGit` implementation
- [x] `parse_name_status` — pure function for parsing `git diff --name-status` output
- [ ] Line number tracking is parsed into IR but not yet displayed

---

## Git Integration
- [x] Enumerate changed files (`git diff <base>..HEAD --name-status`)
- [x] Fetch per-file diff content (`git diff <base>..HEAD -- <file>`)
- [ ] Handle renamed files properly (currently path contains both old and new names)

---

## TUI — Layout & Navigation
- [x] Two-panel layout: file list (left) + diff view (right)
- [x] Status bar with context-sensitive key hints
- [x] Tab to switch between panels
- [x] Arrow key navigation within each panel
- [x] File list: shows filename + status indicator `[M]`, `[A]`, `[D]`, `[R]`
- [x] File list: color-code by status (A=green, M=yellow, D=red, R=cyan)
- [x] Diff view: renders added/removed/context lines with color
- [x] Diff view: show line numbers alongside each line
- [x] Diff view: scroll upper-bound capping (viewport-aware, won't scroll past content)
- [x] Hunk header display (cyan, bold+yellow when selected)
- [x] `[` / `]` to jump between hunks (updates `selected_hunk`)
- [x] Hunk jump scrolls the diff view to bring selected hunk into view
- [x] Enter on file list opens file and switches to diff panel
- [x] Auto-load diff when navigating file list

---

## TUI — Comment Flow
- [x] `c` enters comment mode on current hunk
- [x] Inline comment input renders below hunk with cursor indicator
- [x] Character input appends to comment; Backspace removes last char
- [x] Enter submits comment; Esc cancels
- [x] Submitted notes display inline in diff (◎ marker)
- [x] Note dot marker (●) in file list for files with notes
- [ ] Multi-line comment input (currently single-line only; post-MVP)

---

## Export Layer
- [x] Markdown export: file path, hunk header, code block, feedback text
- [x] JSON export: structured `{ notes: [...] }` array
- [x] `--output <file>` flag writes to file instead of stdout
- [x] `--json` flag switches to JSON format
- [x] Export only runs if there are notes (silent exit otherwise)
- [ ] End-to-end export smoke test against a real repo

---

## Tests
- [x] `diff.rs`: 13 unit tests (hunk header parsing, diff parsing, line kinds, line numbers)
- [x] `app.rs`: 30 unit tests (file navigation, hunk navigation, scroll capping, hunk offset, comment flow, state transitions)
- [x] `export.rs`: 11 unit tests (markdown format, JSON format, empty cases)
- [x] `git.rs`: 7 unit tests (name-status parsing, edge cases)
- [ ] Integration tests (approach TBD — likely a fixture git repo)

---

## End-to-End Smoke Test
- [ ] Run `delta main` in a real repo with staged changes and verify full flow works
- [ ] Verify exported markdown is well-formed and contains correct file/line context

---

## Post-MVP (not scheduled)
- [ ] Syntax highlighting in diff view (syntect crate)
- [ ] Context line folding (collapse unchanged regions)
- [ ] Renamed file: display old→new path
- [ ] Mouse support
- [ ] Performance: background-load remaining file diffs
- [ ] Performance: virtualized rendering for very large diffs
- [ ] `similar` crate for inline word-level diff highlighting
- [ ] Integration tests with fixture git repo
