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

## Install scripts

### Linux and macOS — `install.sh`

Detects OS and architecture, fetches the latest release, installs to `~/.local/bin` (configurable), and adds it to PATH if needed.

```bash
curl -sSf https://raw.githubusercontent.com/tbrittain/delta/main/install.sh | bash
curl -sSf https://raw.githubusercontent.com/tbrittain/delta/main/install.sh | bash -s -- --install-dir /usr/local/bin
```

Default install locations:
- Linux: `~/.local/bin/delta` (XDG-compliant)
- macOS: `~/.local/bin/delta`

### Windows — `install.ps1`

Supports PowerShell 5.1+ (the version shipped with Windows). Installs to `%LOCALAPPDATA%\Programs\delta\delta.exe` and adds the directory to the user PATH.

```powershell
iwr -useb https://raw.githubusercontent.com/tbrittain/delta/main/install.ps1 | iex
```

To install to a custom location:
```powershell
& ([scriptblock]::Create((iwr -useb https://raw.githubusercontent.com/tbrittain/delta/main/install.ps1).Content)) -InstallDir "C:\Tools"
```
