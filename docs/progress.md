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
- [x] Line numbers displayed in diff view (right-aligned, dark gray, per-line)

---

## Git Integration
- [x] Enumerate changed files (`git diff <base>..HEAD --name-status`)
- [x] Fetch per-file diff content (`git diff <base>..HEAD -- <file>`)
- [x] Handle renamed files properly (extracts new path from `R100\told\tnew` format)

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

## TUI — Layout & Navigation (additional)
- [x] Soft line wrapping in diff view (preserves indentation, no truncation)
- [x] Selected hunk indicator: `▶` marker on active hunk header
- [x] Panel title shows `filename — N/M` hunk position counter

---

## TUI — Comment Flow
- [x] `c` enters comment mode on current hunk (redirects to edit if note already exists)
- [x] Multi-line comment input: `Enter` inserts newline, `Ctrl+D` submits, `Esc` cancels
- [x] Inline comment input renders below hunk with cursor indicator (multi-line aware)
- [x] Submitted notes display inline in diff (◎ marker, multi-line aware)
- [x] Note dot marker (●) in file list for files with notes
- [x] `e` edits existing comment (re-opens input pre-populated with old text)
- [x] `d` deletes existing comment on current hunk
- [x] Status bar shows `e: edit  d: delete` contextually when hunk has a note

---

## Export Layer
- [x] Markdown export: file path, hunk header, code block, feedback text
- [x] JSON export: structured `{ notes: [...] }` array
- [x] `--output <file>` flag writes to file instead of stdout
- [x] `--json` flag switches to JSON format
- [x] Export only runs if there are notes (silent exit otherwise)
- [x] End-to-end pipeline tests cover export without requiring a terminal

---

## Tests
- [x] `diff.rs`: 14 unit tests (hunk header parsing, diff parsing, line kinds, line numbers)
- [x] `app.rs`: 53 unit tests (file navigation, hunk navigation, scroll, hunk offset, comment flow, multi-line, edit/delete, context folding)
- [x] `export.rs`: 20 unit tests (markdown format, JSON format, preamble, blockquote, diff fence, multi-line notes)
- [x] `git.rs`: 8 unit tests (name-status parsing, rename path extraction)
- [x] `ui.rs`: 12 unit tests (hunk marker, indent, loading, multi-line rendering, fold/expand rendering)
- [x] Integration tests: 23 tests — fixture git repo (M, A, D, R), git layer, parse pipeline, arbitrary range, full app→export flow

---

## End-to-End
- [x] Integration tests exercise the full git → parse pipeline against a fixture repo
- [x] Full pipeline tests (git → parse → App state → export) without requiring a terminal
- [x] No-TTY handling: delta spawns a terminal window when stdin is not a TTY (e.g. Claude Code `!` commands); output is piped back to the caller on exit
- [x] Verified end-to-end: `! delta HEAD^` in Claude Code opens gnome-terminal, review output lands in conversation

---

## Next Up (in order)

### 1. Bug: `c` on a hunk with an existing note creates a duplicate
- [x] Pressing `c` when the current hunk already has a note should redirect to edit mode rather than opening a blank input

### 2. Arbitrary range comparison
- [x] Add optional second positional argument: `delta <from> <to>`
- [x] When two args provided, diff `<from>..<to>` instead of `<base>..HEAD`
- [x] Update CLI help and README

### 3. Multi-line comment input
- [x] Replace single-line input with a multi-line text area
- [x] Support newlines within the comment body
- [x] `Enter` inserts newline; `Ctrl+D` submits (Ctrl+Enter indistinguishable from Enter in most terminals)

### 4. Context folding
- [x] Collapse consecutive unchanged context lines within a hunk into a `·· N lines ··` placeholder
- [x] `Space` toggles fold/expand on the selected hunk
- [x] App state tracks which hunks are expanded (`expanded_hunks: HashSet<usize>`)
- [x] Scroll offset and diff content line calculations updated to account for folded vs expanded state
- [x] Hunk-jump scroll positioning updated accordingly
- [x] Status bar shows `Space: expand` / `Space: fold` contextually when hunk is foldable

---

## Post-MVP (not scheduled)
- [ ] Version indicator — show currently installed version somewhere in the TUI (status bar or panel title)
- [ ] Syntax highlighting in diff view (syntect crate)
- [ ] Renamed file: display old→new path in file list
- [ ] Mouse support
- [ ] Performance: background-load remaining file diffs
- [ ] Performance: virtualized rendering for very large diffs
- [ ] `similar` crate for inline word-level diff highlighting
- [ ] Side-by-side diff view
- [ ] Virtual hunk splitting for pure-addition large files (no context to fold)
