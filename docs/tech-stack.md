# delta — Technology Stack

## Language: Rust

**Decision: Rust.**

Reasons:
- Near-instant startup — no GC pause, no VM warmup
- Memory efficiency
- Single static binaries (easy to install, no runtime deps)
- Strong ecosystem for terminal tooling (ratatui, crossterm, clap)
- Excellent fit for systems-level CLI tools

Avoided:
- Electron / browser rendering (startup cost, resource use)
- Python/Node (startup latency, distribution complexity)
- Heavyweight GUI frameworks

---

## TUI Stack: ratatui + crossterm

### [ratatui](https://ratatui.rs)
The standard Rust TUI framework. Immediate-mode rendering, composable widgets, well-maintained.

### [crossterm](https://github.com/crossterm-rs/crossterm)
Cross-platform terminal backend for ratatui. Handles raw mode, key events, mouse input.

Why this combination:
- Mature and actively maintained
- Performant
- Portable (Linux, macOS, Windows)
- Works correctly inside tmux and over SSH

---

## Diff Engine: git diff (MVP)

**MVP decision: shell out to `git diff`, parse unified diff format.**

- Zero additional dependencies
- Unified diff format is simple and well-defined to parse
- Line numbers are authoritative — no mapping problem
- Sufficient for MVP review workflow

**Candidate upgrade: [`similar`](https://github.com/mitsuhiko/similar) crate**
- Pure Rust diff library
- Produces inline character-level diffs (word-level highlighting within a line)
- Good for future "what exactly changed on this line" highlighting
- Does not require shelling out

**Deferred: Difftastic**
- Structural/semantic diffing (AST-aware)
- No stable machine-readable output format as of 2025
- Deferred to post-MVP; would require parsing ANSI output or upstream contribution

---

## Argument Parsing: clap

Standard Rust CLI argument parsing. Derive-macro style for low boilerplate.

---

## Syntax Highlighting (future)

**Candidate: [`syntect`](https://github.com/trishume/syntect)**
- Pure Rust syntax highlighting using Sublime Text grammars
- For coloring diff content by language in the TUI

**Candidate: [`tree-sitter`](https://tree-sitter.github.io/tree-sitter)**
- If syntax-aware selections or symbol navigation are added post-MVP
- More powerful but heavier dependency

---

## Git Integration

MVP: shell out to `git` subprocess.
Future: [`git2`](https://github.com/rust-lang/git2-rs) crate (libgit2 bindings) for in-process git operations.
