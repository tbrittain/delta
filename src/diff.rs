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

#[derive(Debug, Clone, PartialEq)]
pub enum LineKind {
    Added,
    Removed,
    Context,
}

pub fn parse_diff(raw: &str, file: ChangedFile) -> DiffFile {
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

    DiffFile { file, hunks }
}

fn parse_hunk_header(header: &str) -> (u32, u32) {
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
