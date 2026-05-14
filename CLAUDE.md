# delta — Development Guide

## Coding Standards

### Tests are mandatory

Every non-trivial piece of logic must have tests. Do not implement a feature or fix a bug without covering it with tests. Tests are not optional and are not to be added "later."

**Current standard: unit tests + integration tests.**
- Each module should have a `#[cfg(test)]` block with tests for its public functions.
- Pure logic (diff parsing, export formatting, app state transitions) must be unit tested.
- Git shell-out functions (`SystemGit`) are covered by integration tests in `tests/integration.rs`.

**Integration tests** live in `tests/` and use a fixture git repository built programmatically in `tests/common/mod.rs`. They test the full git → parse → app → export pipeline against real git output.

When in doubt, write the test first.

### Test placement

Use in-module tests (`#[cfg(test)]` at the bottom of each file) for unit tests. Integration tests go in `tests/`.

### What must be tested

- `diff.rs`: the unified diff parser (`parse_diff`, `parse_hunk_header`) — this is the core data pipeline
- `export.rs`: markdown and JSON output format
- `app.rs`: state transitions (file selection, hunk navigation, comment submission/cancellation, fold state)
- `ui.rs`: rendering logic in `build_diff_text` and helpers (use ratatui's `Text` output directly)
- `git.rs`: `parse_name_status` and other pure functions; `SystemGit` via integration tests
- Any new parsing or transformation logic added in future modules

### Design code to be testable

New code must be structured so it can be tested without a real terminal, real git process, or other external dependencies. Concretely:

- **Extract pure functions.** Logic that transforms data (parsing, formatting, state transitions) must live in functions that take plain arguments and return plain values — no I/O, no side effects. These are trivially testable.
- **Use the `GitBackend` trait.** Any code that needs git data must accept `&impl GitBackend`, not call `git::SystemGit` directly. This allows tests to inject a fake implementation.
- **Keep I/O at the boundary.** Terminal rendering (`ui.rs`) and subprocess calls (`git.rs` `SystemGit`) are the only places that touch the outside world. Everything else must be pure and injectable.
- **Thin dispatchers are acceptable untested.** Code that does nothing except call other tested functions (e.g. `run_event_loop` matching a key and calling an `App` method) does not need its own unit test. The logic it dispatches to must be tested.

If you find yourself writing logic inside a function that shells out or renders to the terminal, stop and extract it.

### Unsafe code

Do not write `unsafe` Rust unless there is genuinely no safe alternative **and** a human has explicitly approved the decision in conversation. Before reaching for `unsafe`:

1. Exhaust safe alternatives (trait objects, wrapper crates, restructuring).
2. If no safe alternative exists, explain why to the user and get explicit sign-off before writing or committing the unsafe block.
3. Add a `// SAFETY:` comment on every `unsafe {}` block explaining the invariants that make it sound.

The only current approved use of `unsafe` in this codebase is in `main.rs` `attach_to_console()`: three `SetStdHandle` calls that redirect Windows standard handles to the attached console before crossterm touches `GetStdHandle`. This was approved 2026-05-13 after confirming no safe alternative exists — crossterm's `enable_raw_mode()` unconditionally calls `GetStdHandle(STD_INPUT_HANDLE)` internally with no public API to override it.

---

## Commit Style

All commits must use **conventional commits** format:

```
<type>: <description>

[optional body]

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>
```

Types:
- `feat:` — new user-facing feature
- `fix:` — bug fix
- `refactor:` — restructuring with no feature or fix
- `test:` — adding or updating tests only
- `docs:` — documentation changes only
- `chore:` — maintenance, dependency updates, tooling
- `ci:` — CI/CD workflow changes

Breaking changes: append `!` after the type (`feat!:`) or add `BREAKING CHANGE:` in the footer.

This convention is required because semver automation from commit history is planned for the future.

---

## Feature docs & planned improvements

All planned features, design notes, and improvement ideas live in **`docs/features/<feature>.md`**, one file per **user-visible feature area** — named after what the user sees and interacts with, not after internal source modules.

Current files:
- `diff-view.md` — the diff panel (hunks, folding, syntax highlighting, planned: full-file view, find, whitespace flags)
- `file-navigation.md` — the file list panel and notes panel (planned: tree view, find-in-files)
- `comment-input.md` — the comment popup/editor (planned: line-level comments)
- `export.md` — the output formats (markdown, JSON)
- `git-integration.md` — how delta shells out to git, invocation options (planned: whitespace flags backend, line-range fetch)

Rules:
- Each doc describes **current behaviour first**, then planned improvements. Keep it accurate as the app evolves.
- When a major new user-visible feature is added, create a new doc for it. Do **not** create docs for internal source modules.
- Implementation notes (which structs/functions are involved) are fine to include — the audience is a developer.
