use anyhow::{bail, Context, Result};
use std::process::Command;

use crate::diff::{ChangedFile, FileStatus};

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum WhitespaceMode {
    #[default]
    None,
    /// `git diff -b`: ignore changes in whitespace amount
    IgnoreChanges,
    /// `git diff -w`: ignore all whitespace
    IgnoreAll,
}

impl WhitespaceMode {
    /// The git flag for this mode, or `None` for the default (no flag).
    pub fn flag(self) -> Option<&'static str> {
        match self {
            Self::None          => None,
            Self::IgnoreChanges => Some("-b"),
            Self::IgnoreAll     => Some("-w"),
        }
    }

    /// Cycle to the next mode: None → -b → -w → None.
    pub fn next(self) -> Self {
        match self {
            Self::None          => Self::IgnoreChanges,
            Self::IgnoreChanges => Self::IgnoreAll,
            Self::IgnoreAll     => Self::None,
        }
    }

    /// Short label shown in the diff panel title, or `""` when no mode is active.
    pub fn label(self) -> &'static str {
        match self {
            Self::None          => "",
            Self::IgnoreChanges => " (-b)",
            Self::IgnoreAll     => " (-w)",
        }
    }
}

pub trait GitBackend {
    fn changed_files(&self, from: &str, to: &str) -> Result<Vec<ChangedFile>>;
    fn file_diff(&self, from: &str, to: &str, path: &str, whitespace: WhitespaceMode) -> Result<String>;
}

pub struct SystemGit {
    repo_dir: std::path::PathBuf,
}

impl SystemGit {
    /// Resolve the git repo root starting from the current working directory.
    pub fn new() -> Self {
        Self::new_at(std::path::Path::new("."))
    }

    /// Resolve the git repo root starting from `start`, then run all git commands from
    /// that root. This ensures that file paths returned by `--name-status` (which are
    /// always relative to the repo root) are valid pathspecs for subsequent `git diff`
    /// calls, regardless of which subdirectory the user ran delta from.
    pub fn new_at(start: &std::path::Path) -> Self {
        let repo_dir = Command::new("git")
            .args(["rev-parse", "--show-toplevel"])
            .current_dir(start)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| std::path::PathBuf::from(s.trim()))
            .unwrap_or_else(|| start.to_path_buf());
        log::debug!("[git] repo root resolved to {:?}", repo_dir);
        Self { repo_dir }
    }

    /// Use a specific directory as the repository root.
    /// Primarily used in tests to point at a fixture repository.
    pub fn with_dir(dir: impl Into<std::path::PathBuf>) -> Self {
        Self { repo_dir: dir.into() }
    }
}

impl Default for SystemGit {
    fn default() -> Self {
        Self::new()
    }
}

impl GitBackend for SystemGit {
    fn changed_files(&self, from: &str, to: &str) -> Result<Vec<ChangedFile>> {
        log::debug!(
            "[git] changed_files: from={:?} to={:?} cwd={:?}",
            from, to, self.repo_dir
        );

        let output = Command::new("git")
            .args(["diff", "--no-ext-diff", "--name-status", &format!("{}..{}", from, to)])
            .current_dir(&self.repo_dir)
            .output()
            .context("Failed to run git. Is git installed and are you inside a git repository?")?;

        log::debug!(
            "[git] changed_files: exit={:?} stdout_bytes={} stderr_bytes={}",
            output.status.code(), output.stdout.len(), output.stderr.len()
        );
        log::debug!(
            "[git] changed_files: stdout={:?}",
            String::from_utf8_lossy(&output.stdout)
        );
        if !output.stderr.is_empty() {
            log::debug!(
                "[git] changed_files: stderr={:?}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        if !output.status.success() {
            bail!(
                "git diff failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }

        let files = parse_name_status(&String::from_utf8(output.stdout)?);
        log::debug!("[git] changed_files: parsed {} files", files.len());
        for f in &files {
            log::debug!("[git]   {:?} -> {}", f.status, f.path.display());
        }
        Ok(files)
    }

    fn file_diff(&self, from: &str, to: &str, path: &str, whitespace: WhitespaceMode) -> Result<String> {
        // Git always accepts forward slashes; on Windows PathBuf produces backslashes.
        let normalized = path.replace('\\', "/");
        let range = format!("{}..{}", from, to);

        log::debug!(
            "[git] file_diff: from={:?} to={:?} path={:?} normalized={:?} whitespace={:?} cwd={:?}",
            from, to, path, normalized, whitespace, self.repo_dir
        );

        let mut cmd = Command::new("git");
        cmd.arg("diff").arg("--no-ext-diff");
        if let Some(flag) = whitespace.flag() {
            cmd.arg(flag);
        }
        cmd.arg(&range).arg("--").arg(&normalized);
        cmd.current_dir(&self.repo_dir);

        let output = cmd.output()
            .with_context(|| format!("Failed to run git diff for {}", path))?;

        log::debug!(
            "[git] file_diff: exit={:?} stdout_bytes={} stderr_bytes={}",
            output.status.code(), output.stdout.len(), output.stderr.len()
        );
        if !output.stderr.is_empty() {
            log::debug!(
                "[git] file_diff: stderr={:?}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        // Log a preview of stdout so we can see if the diff content is arriving.
        let preview: String = output.stdout.iter()
            .take(400)
            .map(|&b| b as char)
            .collect();
        log::debug!("[git] file_diff: stdout_preview={:?}", preview);

        if !output.status.success() {
            bail!(
                "git diff failed for {}: {}",
                normalized,
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }

        Ok(String::from_utf8(output.stdout)?)
    }
}

/// Parse the output of `git diff --name-status` into a list of changed files.
/// Extracted as a pure function so it can be tested without a real git process.
pub fn parse_name_status(output: &str) -> Vec<ChangedFile> {
    let mut files = Vec::new();

    for line in output.lines() {
        let mut parts = line.splitn(2, '\t');
        let status_str = parts.next().unwrap_or("").trim();
        let path_str = parts.next().unwrap_or("").trim();

        if path_str.is_empty() {
            continue;
        }

        let status = match status_str.chars().next().unwrap_or('?') {
            'A' => FileStatus::Added,
            'D' => FileStatus::Deleted,
            'R' => FileStatus::Renamed,
            _ => FileStatus::Modified,
        };

        // Renames produce two tab-separated paths: "old_path\tnew_path".
        // We only need the new path for display and diffing.
        let path = if status == FileStatus::Renamed {
            path_str.splitn(2, '\t').nth(1).unwrap_or(path_str)
        } else {
            path_str
        };

        files.push(ChangedFile {
            path: path.into(),
            status,
        });
    }

    files
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── WhitespaceMode ────────────────────────────────────────────────────────

    #[test]
    fn test_whitespace_mode_flags() {
        assert_eq!(WhitespaceMode::None.flag(), None);
        assert_eq!(WhitespaceMode::IgnoreChanges.flag(), Some("-b"));
        assert_eq!(WhitespaceMode::IgnoreAll.flag(), Some("-w"));
    }

    #[test]
    fn test_whitespace_mode_cycle() {
        assert_eq!(WhitespaceMode::None.next(), WhitespaceMode::IgnoreChanges);
        assert_eq!(WhitespaceMode::IgnoreChanges.next(), WhitespaceMode::IgnoreAll);
        assert_eq!(WhitespaceMode::IgnoreAll.next(), WhitespaceMode::None);
    }

    #[test]
    fn test_whitespace_mode_labels() {
        assert_eq!(WhitespaceMode::None.label(), "");
        assert_eq!(WhitespaceMode::IgnoreChanges.label(), " (-b)");
        assert_eq!(WhitespaceMode::IgnoreAll.label(), " (-w)");
    }

    // ── parse_name_status ─────────────────────────────────────────────────────

    #[test]
    fn test_parse_name_status_modified() {
        let input = "M\tsrc/main.rs\n";
        let files = parse_name_status(input);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path.to_str().unwrap(), "src/main.rs");
        assert_eq!(files[0].status, FileStatus::Modified);
    }

    #[test]
    fn test_parse_name_status_added() {
        let input = "A\tsrc/new_file.rs\n";
        let files = parse_name_status(input);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].status, FileStatus::Added);
    }

    #[test]
    fn test_parse_name_status_deleted() {
        let input = "D\tsrc/old_file.rs\n";
        let files = parse_name_status(input);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].status, FileStatus::Deleted);
    }

    #[test]
    fn test_parse_name_status_renamed_uses_new_path() {
        let input = "R100\tsrc/old.rs\tsrc/new.rs\n";
        let files = parse_name_status(input);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].status, FileStatus::Renamed);
        assert_eq!(files[0].path, std::path::PathBuf::from("src/new.rs"));
    }

    #[test]
    fn test_parse_name_status_renamed_discards_old_path() {
        let input = "R075\tsrc/utils/old_name.rs\tsrc/utils/new_name.rs\n";
        let files = parse_name_status(input);
        assert_eq!(files.len(), 1);
        // Must not contain the old path in any form
        assert!(!files[0].path.to_string_lossy().contains("old_name"));
        assert_eq!(files[0].path.to_string_lossy(), "src/utils/new_name.rs");
    }

    #[test]
    fn test_parse_name_status_multiple_files() {
        let input = "M\tsrc/main.rs\nA\tsrc/git.rs\nD\tsrc/old.rs\n";
        let files = parse_name_status(input);
        assert_eq!(files.len(), 3);
        assert_eq!(files[0].status, FileStatus::Modified);
        assert_eq!(files[1].status, FileStatus::Added);
        assert_eq!(files[2].status, FileStatus::Deleted);
    }

    #[test]
    fn test_parse_name_status_empty() {
        let files = parse_name_status("");
        assert!(files.is_empty());
    }

    #[test]
    fn test_parse_name_status_skips_malformed_lines() {
        let input = "M\tsrc/main.rs\n\nsome garbage line\nA\tsrc/new.rs\n";
        let files = parse_name_status(input);
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn test_file_diff_normalizes_backslash_paths() {
        // On Windows, PathBuf converts forward slashes to backslashes.
        // Ensure the normalization in file_diff produces forward slashes for git.
        let path_with_backslash = "src\\main.rs";
        let normalized = path_with_backslash.replace('\\', "/");
        assert_eq!(normalized, "src/main.rs");
    }
}
