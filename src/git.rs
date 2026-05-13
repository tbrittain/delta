use anyhow::{bail, Context, Result};
use std::process::Command;

use crate::diff::{ChangedFile, FileStatus};

pub fn changed_files(base: &str) -> Result<Vec<ChangedFile>> {
    let output = Command::new("git")
        .args(["diff", "--name-status", &format!("{}..HEAD", base)])
        .output()
        .context("Failed to run git. Is git installed and are you inside a git repository?")?;

    if !output.status.success() {
        bail!(
            "git diff failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    let stdout = String::from_utf8(output.stdout)?;
    let mut files = Vec::new();

    for line in stdout.lines() {
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

    Ok(files)
}

pub fn file_diff(base: &str, path: &str) -> Result<String> {
    let output = Command::new("git")
        .args(["diff", &format!("{}..HEAD", base), "--", path])
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
