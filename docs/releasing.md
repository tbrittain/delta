# Releasing delta

## Branching model

- **`main`** — always releasable. Protected; changes land via pull request.
- **Feature branches** — named `feature/description`, `fix/description`, etc. Open a PR against `main`. CI runs automatically on every PR.

## CI

Every push to `main` and every pull request runs:
- `cargo test` — full unit + integration test suite
- `cargo build --release` — verifies the release binary compiles

PRs should not be merged if CI is red.

---

## Cutting a release

### 1. Update the version

Edit `Cargo.toml` and bump the version:

```toml
[package]
version = "0.2.0"
```

Commit and push to `main`:

```bash
git add Cargo.toml Cargo.lock
git commit -m "Bump version to 0.2.0"
git push
```

### 2. Tag the release

Tags must be prefixed with `v`:

```bash
git tag v0.2.0
git push origin v0.2.0
```

### 3. Wait for the release workflow

The `release.yml` workflow triggers on the tag push. It cross-compiles for all five targets, packages the binaries, and creates a GitHub release automatically with generated release notes.

Targets built:
| Target | Platform |
|---|---|
| `x86_64-unknown-linux-gnu` | Linux x86_64 |
| `aarch64-unknown-linux-gnu` | Linux ARM64 |
| `x86_64-apple-darwin` | macOS Intel |
| `aarch64-apple-darwin` | macOS Apple Silicon |
| `x86_64-pc-windows-msvc` | Windows x86_64 |

### 4. Verify the release

Check [GitHub Releases](https://github.com/tbrittain/delta/releases) to confirm all five artifacts are attached and the release notes look correct.

---

## Repository visibility

The install script and release artifacts require the repository to be **public** for unauthenticated access. Make the repo public before publishing a release intended for external users.

---

## Install script

`install.sh` at the repo root supports Linux and macOS. It:
1. Detects OS and architecture
2. Fetches the latest release tag from the GitHub API
3. Downloads and extracts the correct binary
4. Installs to `~/.local/bin` (configurable via `--install-dir`)
5. Handles replacing an existing install

**Usage:**
```bash
# Install latest release
curl -sSf https://raw.githubusercontent.com/tbrittain/delta/main/install.sh | bash

# Install to a custom directory
curl -sSf https://raw.githubusercontent.com/tbrittain/delta/main/install.sh | bash -s -- --install-dir /usr/local/bin

# Run locally
bash install.sh [--install-dir DIR]
```

Windows users should download the `.zip` from the Releases page and add the binary to their PATH manually.
