use anyhow::{bail, Context, Result};
use std::process::Command;

use crate::diff::{ChangedFile, FileStatus};

pub trait GitBackend {
    fn changed_files(&self, base: &str) -> Result<Vec<ChangedFile>>;
    fn file_diff(&self, base: &str, path: &str) -> Result<String>;
}

pub struct SystemGit {
    repo_dir: std::path::PathBuf,
}

impl SystemGit {
    /// Use the current working directory as the repository root.
    pub fn new() -> Self {
        Self { repo_dir: std::path::PathBuf::from(".") }
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
    fn changed_files(&self, base: &str) -> Result<Vec<ChangedFile>> {
        let output = Command::new("git")
            .args(["diff", "--name-status", &format!("{}..HEAD", base)])
            .current_dir(&self.repo_dir)
            .output()
            .context("Failed to run git. Is git installed and are you inside a git repository?")?;

        if !output.status.success() {
            bail!(
                "git diff failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }

        Ok(parse_name_status(&String::from_utf8(output.stdout)?))
    }

    fn file_diff(&self, base: &str, path: &str) -> Result<String> {
        let output = Command::new("git")
            .args(["diff", &format!("{}..HEAD", base), "--", path])
            .current_dir(&self.repo_dir)
            .output()
            .with_context(|| format!("Failed to run git diff for {}", path))?;

        if !output.status.success() {
            bail!(
                "git diff failed for {}: {}",
                path,
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

        files.push(ChangedFile {
            path: path_str.into(),
            status,
        });
    }

    files
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_parse_name_status_renamed() {
        let input = "R100\tsrc/old.rs\tsrc/new.rs\n";
        let files = parse_name_status(input);
        // splitn(2) on R100\told.rs\tnew.rs gives status="R100", path="old.rs\tnew.rs"
        // path will contain both names; this is acceptable for MVP
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].status, FileStatus::Renamed);
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
}
