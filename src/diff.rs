use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq)]
pub enum FileStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
}

impl FileStatus {
    pub fn indicator(&self) -> &'static str {
        match self {
            FileStatus::Added => "A",
            FileStatus::Modified => "M",
            FileStatus::Deleted => "D",
            FileStatus::Renamed => "R",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ChangedFile {
    pub path: PathBuf,
    pub status: FileStatus,
}

#[derive(Debug, Clone)]
pub struct DiffFile {
    pub file: ChangedFile,
    pub hunks: Vec<Hunk>,
}

#[derive(Debug, Clone)]
pub struct Hunk {
    pub header: String,
    pub old_start: u32,
    pub new_start: u32,
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone)]
pub struct DiffLine {
    pub old_lineno: Option<u32>,
    pub new_lineno: Option<u32>,
    pub kind: LineKind,
    pub content: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LineKind {
    Added,
    Removed,
    Context,
}

pub fn parse_diff(raw: &str, file: ChangedFile) -> DiffFile {
    let hunk_markers = raw.lines().filter(|l| l.starts_with("@@")).count();
    log::debug!(
        "[parse] parse_diff: path={} raw_bytes={} hunk_markers_found={}",
        file.path.display(), raw.len(), hunk_markers
    );

    let mut hunks = Vec::new();
    let mut current_hunk: Option<Hunk> = None;
    let mut old_line = 0u32;
    let mut new_line = 0u32;

    for line in raw.lines() {
        if line.starts_with("@@") {
            if let Some(hunk) = current_hunk.take() {
                hunks.push(hunk);
            }
            let (old_start, new_start) = parse_hunk_header(line);
            old_line = old_start;
            new_line = new_start;
            current_hunk = Some(Hunk {
                header: line.to_string(),
                old_start,
                new_start,
                lines: Vec::new(),
            });
        } else if let Some(ref mut hunk) = current_hunk {
            let (kind, content) = if let Some(c) = line.strip_prefix('+') {
                (LineKind::Added, c.to_string())
            } else if let Some(c) = line.strip_prefix('-') {
                (LineKind::Removed, c.to_string())
            } else if let Some(c) = line.strip_prefix(' ') {
                (LineKind::Context, c.to_string())
            } else {
                continue;
            };

            let old_lineno = match kind {
                LineKind::Added => None,
                _ => {
                    let n = old_line;
                    old_line += 1;
                    Some(n)
                }
            };
            let new_lineno = match kind {
                LineKind::Removed => None,
                _ => {
                    let n = new_line;
                    new_line += 1;
                    Some(n)
                }
            };

            hunk.lines.push(DiffLine {
                old_lineno,
                new_lineno,
                kind,
                content,
            });
        }
    }

    if let Some(hunk) = current_hunk {
        hunks.push(hunk);
    }

    log::debug!("[parse] parse_diff: result={} hunks", hunks.len());
    DiffFile { file, hunks }
}

pub(crate) fn parse_hunk_header(header: &str) -> (u32, u32) {
    // Parse: @@ -old_start[,old_count] +new_start[,new_count] @@
    let mut old_start = 1u32;
    let mut new_start = 1u32;

    if let Some(rest) = header.strip_prefix("@@ ") {
        let parts: Vec<&str> = rest.split_whitespace().collect();
        if parts.len() >= 2 {
            if let Some(old) = parts[0].strip_prefix('-') {
                old_start = old.split(',').next().and_then(|s| s.parse().ok()).unwrap_or(1);
            }
            if let Some(new) = parts[1].strip_prefix('+') {
                new_start = new.split(',').next().and_then(|s| s.parse().ok()).unwrap_or(1);
            }
        }
    }

    (old_start, new_start)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Helpers ──────────────────────────────────────────────────────────────

    fn modified_file(path: &str) -> ChangedFile {
        ChangedFile {
            path: PathBuf::from(path),
            status: FileStatus::Modified,
        }
    }

    // A realistic two-hunk unified diff (as produced by `git diff`).
    const SAMPLE_DIFF: &str = "\
diff --git a/src/main.rs b/src/main.rs
index abc1234..def5678 100644
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,5 +1,7 @@
 use std::io;
+use anyhow::Result;
+use clap::Parser;

 fn main() {
-    println!(\"Hello, world!\");
+    println!(\"Hello, delta!\");
 }
@@ -20,3 +22,4 @@
 fn helper() {
+    // new comment
     do_something();
 }
";

    // ── parse_hunk_header ─────────────────────────────────────────────────────

    #[test]
    fn test_hunk_header_basic() {
        let (old, new) = parse_hunk_header("@@ -1,5 +1,7 @@");
        assert_eq!(old, 1);
        assert_eq!(new, 1);
    }

    #[test]
    fn test_hunk_header_nonzero_start() {
        let (old, new) = parse_hunk_header("@@ -20,3 +22,4 @@");
        assert_eq!(old, 20);
        assert_eq!(new, 22);
    }

    #[test]
    fn test_hunk_header_with_trailing_context() {
        // git often appends a function name after the second @@
        let (old, new) = parse_hunk_header("@@ -42,6 +42,9 @@ fn authenticate(");
        assert_eq!(old, 42);
        assert_eq!(new, 42);
    }

    #[test]
    fn test_hunk_header_no_count() {
        // single-line hunks omit the count: @@ -5 +5 @@
        let (old, new) = parse_hunk_header("@@ -5 +5 @@");
        assert_eq!(old, 5);
        assert_eq!(new, 5);
    }

    // ── parse_diff ────────────────────────────────────────────────────────────

    #[test]
    fn test_parse_diff_hunk_count() {
        let diff = parse_diff(SAMPLE_DIFF, modified_file("src/main.rs"));
        assert_eq!(diff.hunks.len(), 2);
    }

    #[test]
    fn test_parse_diff_preserves_file_path() {
        let diff = parse_diff(SAMPLE_DIFF, modified_file("src/main.rs"));
        assert_eq!(diff.file.path, PathBuf::from("src/main.rs"));
    }

    #[test]
    fn test_parse_diff_first_hunk_header() {
        let diff = parse_diff(SAMPLE_DIFF, modified_file("src/main.rs"));
        assert_eq!(diff.hunks[0].header, "@@ -1,5 +1,7 @@");
    }

    #[test]
    fn test_parse_diff_first_hunk_line_kinds() {
        let diff = parse_diff(SAMPLE_DIFF, modified_file("src/main.rs"));
        let hunk = &diff.hunks[0];

        let added: Vec<_> = hunk.lines.iter().filter(|l| l.kind == LineKind::Added).collect();
        let removed: Vec<_> = hunk.lines.iter().filter(|l| l.kind == LineKind::Removed).collect();
        let context: Vec<_> = hunk.lines.iter().filter(|l| l.kind == LineKind::Context).collect();

        assert_eq!(added.len(), 3);   // +use anyhow, +use clap, +println delta
        assert_eq!(removed.len(), 1); // -println world
        assert_eq!(context.len(), 3); // use std::io, fn main(), } (blank line has no leading space so parser skips it)
    }

    #[test]
    fn test_parse_diff_added_line_content() {
        let diff = parse_diff(SAMPLE_DIFF, modified_file("src/main.rs"));
        let hunk = &diff.hunks[0];
        let added_contents: Vec<&str> = hunk
            .lines
            .iter()
            .filter(|l| l.kind == LineKind::Added)
            .map(|l| l.content.as_str())
            .collect();
        assert!(added_contents.contains(&"use anyhow::Result;"));
        assert!(added_contents.contains(&"use clap::Parser;"));
    }

    #[test]
    fn test_parse_diff_second_hunk() {
        let diff = parse_diff(SAMPLE_DIFF, modified_file("src/main.rs"));
        let hunk = &diff.hunks[1];
        assert_eq!(hunk.old_start, 20);
        assert_eq!(hunk.new_start, 22);
        assert_eq!(hunk.lines.iter().filter(|l| l.kind == LineKind::Added).count(), 1);
        assert_eq!(hunk.lines.iter().filter(|l| l.kind == LineKind::Context).count(), 3);
    }

    #[test]
    fn test_parse_diff_added_lines_have_no_old_lineno() {
        let diff = parse_diff(SAMPLE_DIFF, modified_file("src/main.rs"));
        for hunk in &diff.hunks {
            for line in &hunk.lines {
                if line.kind == LineKind::Added {
                    assert!(line.old_lineno.is_none());
                }
            }
        }
    }

    #[test]
    fn test_parse_diff_removed_lines_have_no_new_lineno() {
        let diff = parse_diff(SAMPLE_DIFF, modified_file("src/main.rs"));
        for hunk in &diff.hunks {
            for line in &hunk.lines {
                if line.kind == LineKind::Removed {
                    assert!(line.new_lineno.is_none());
                }
            }
        }
    }

    #[test]
    fn test_parse_diff_empty_input() {
        let diff = parse_diff("", modified_file("src/main.rs"));
        assert!(diff.hunks.is_empty());
    }

    #[test]
    fn test_parse_diff_skips_git_headers() {
        // The diff --git / --- / +++ header lines must not appear as diff lines
        let diff = parse_diff(SAMPLE_DIFF, modified_file("src/main.rs"));
        for hunk in &diff.hunks {
            for line in &hunk.lines {
                assert!(!line.content.starts_with("--git"));
                assert!(!line.content.starts_with("-- a/"));
                assert!(!line.content.starts_with("++ b/"));
            }
        }
    }
}
