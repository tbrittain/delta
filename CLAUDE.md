# delta — Development Guide

## Coding Standards

### Tests are mandatory

Every non-trivial piece of logic must have tests. Do not implement a feature or fix a bug without covering it with tests. Tests are not optional and are not to be added "later."

**Current standard: unit tests.**
- Each module should have a `#[cfg(test)]` block with tests for its public functions.
- Pure logic (diff parsing, export formatting, app state transitions) must be unit tested.
- Git shell-out functions are exempt from unit tests for now but will be covered when integration tests are introduced.

**Future standard: integration tests** (approach TBD, not required yet).

When in doubt, write the test first.

### Test placement

Use in-module tests (`#[cfg(test)]` at the bottom of each file) for unit tests. Do not create separate test files until integration tests are introduced.

### What must be tested

- `diff.rs`: the unified diff parser (`parse_diff`, `parse_hunk_header`) — this is the core data pipeline
- `export.rs`: markdown and JSON output format
- `app.rs`: state transitions (file selection, hunk navigation, comment submission/cancellation)
- Any new parsing or transformation logic added in future modules

### Design code to be testable

New code must be structured so it can be tested without a real terminal, real git process, or other external dependencies. Concretely:

- **Extract pure functions.** Logic that transforms data (parsing, formatting, state transitions) must live in functions that take plain arguments and return plain values — no I/O, no side effects. These are trivially testable.
- **Use the `GitBackend` trait.** Any code that needs git data must accept `&impl GitBackend`, not call `git::SystemGit` directly. This allows tests to inject a fake implementation.
- **Keep I/O at the boundary.** Terminal rendering (`ui.rs`) and subprocess calls (`git.rs` `SystemGit`) are the only places that touch the outside world. Everything else must be pure and injectable.
- **Thin dispatchers are acceptable untested.** Code that does nothing except call other tested functions (e.g. `run_event_loop` matching a key and calling an `App` method) does not need its own unit test. The logic it dispatches to must be tested.

If you find yourself writing logic inside a function that shells out or renders to the terminal, stop and extract it.
