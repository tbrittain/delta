# Releasing delta

## Branching model

- **`main`** — always releasable. Protected; the only way to push is via PR merge.
- **`dev`** — integration branch. Feature branches are merged here first.
- **Feature branches** — named `feature/description`, `fix/description`, `test/description`, etc. Open a PR against `dev` (or `main` for hotfixes).

CI runs automatically on every PR and every push to `main`.

---

## Cutting a release

### 1. Bump the version on the feature branch

Before merging, update `Cargo.toml` on the feature branch:

```toml
[package]
version = "0.2.0"
```

Commit it alongside the feature work:

```bash
git add Cargo.toml Cargo.lock
git commit -m "chore: bump version to 0.2.0"
```

The version bump rides in with the PR — no separate commit to `main` needed.

### 2. Merge the PR

Merge into `main` via GitHub. The merge commit on `main` becomes the release commit.

### 3. Tag the merge commit

After the PR is merged, tag `main` at the merge commit:

```bash
git checkout main && git pull
git tag v0.2.0
git push origin v0.2.0
```

Tags must be prefixed with `v`.

### 4. Wait for the release workflow

The `release.yml` workflow triggers on the tag push. It cross-compiles for all targets, packages the binaries, and creates a GitHub release automatically with generated release notes.

Targets built:
| Target | Platform |
|---|---|
| `x86_64-unknown-linux-gnu` | Linux x86_64 |
| `x86_64-pc-windows-msvc` | Windows x86_64 |

macOS is not a supported target and is excluded from release builds.

### 5. Verify the release

Check [GitHub Releases](https://github.com/tbrittain/delta/releases) to confirm all artifacts are attached and the release notes look correct.

---

## Install scripts

### Linux — `install.sh`

Fetches the latest release and installs to `~/.local/bin` (configurable).

```bash
curl -sSf https://raw.githubusercontent.com/tbrittain/delta/main/install.sh | bash
curl -sSf https://raw.githubusercontent.com/tbrittain/delta/main/install.sh | bash -s -- --install-dir /usr/local/bin
```

Default install location: `~/.local/bin/delta` (XDG-compliant)

### Windows — `install.ps1`

Supports PowerShell 5.1+. Installs to `%LOCALAPPDATA%\Programs\delta\delta.exe` and adds the directory to the user PATH.

```powershell
iwr -useb https://raw.githubusercontent.com/tbrittain/delta/main/install.ps1 | iex
```

To install to a custom location:
```powershell
& ([scriptblock]::Create((iwr -useb https://raw.githubusercontent.com/tbrittain/delta/main/install.ps1).Content)) -InstallDir "C:\Tools"
```
