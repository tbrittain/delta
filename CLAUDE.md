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
