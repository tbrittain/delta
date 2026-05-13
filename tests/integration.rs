mod common;

use common::FixtureRepo;
use delta::diff::{ChangedFile, FileStatus, LineKind, parse_diff};
use delta::git::{GitBackend, SystemGit};

// ── Git integration layer ─────────────────────────────────────────────────────

#[test]
fn test_changed_files_returns_all_four() {
    let repo = FixtureRepo::new();
    let git = SystemGit::with_dir(&repo.path);
    let files = git.changed_files(FixtureRepo::BASE_REF).unwrap();
    assert_eq!(files.len(), 4);
}

#[test]
fn test_changed_files_includes_modified() {
    let repo = FixtureRepo::new();
    let git = SystemGit::with_dir(&repo.path);
    let files = git.changed_files(FixtureRepo::BASE_REF).unwrap();
    let has_modified = files.iter().any(|f| f.status == FileStatus::Modified);
    assert!(has_modified, "expected at least one Modified file");
}

#[test]
fn test_changed_files_includes_added() {
    let repo = FixtureRepo::new();
    let git = SystemGit::with_dir(&repo.path);
    let files = git.changed_files(FixtureRepo::BASE_REF).unwrap();
    let added: Vec<_> = files.iter().filter(|f| f.status == FileStatus::Added).collect();
    assert_eq!(added.len(), 1);
    assert!(added[0].path.ends_with("new.rs"));
}

#[test]
fn test_changed_files_includes_deleted() {
    let repo = FixtureRepo::new();
    let git = SystemGit::with_dir(&repo.path);
    let files = git.changed_files(FixtureRepo::BASE_REF).unwrap();
    let deleted: Vec<_> = files.iter().filter(|f| f.status == FileStatus::Deleted).collect();
    assert_eq!(deleted.len(), 1);
    assert!(deleted[0].path.ends_with("deleted.rs"));
}

#[test]
fn test_changed_files_paths_are_relative() {
    let repo = FixtureRepo::new();
    let git = SystemGit::with_dir(&repo.path);
    let files = git.changed_files(FixtureRepo::BASE_REF).unwrap();
    // Paths from git diff --name-status are always repo-relative, never absolute.
    for f in &files {
        assert!(
            f.path.is_relative(),
            "expected relative path, got {:?}",
            f.path
        );
    }
}

#[test]
fn test_file_diff_returns_nonempty_for_modified_file() {
    let repo = FixtureRepo::new();
    let git = SystemGit::with_dir(&repo.path);
    let raw = git.file_diff(FixtureRepo::BASE_REF, "src/main.rs").unwrap();
    assert!(!raw.is_empty());
}

#[test]
fn test_file_diff_contains_hunk_marker() {
    let repo = FixtureRepo::new();
    let git = SystemGit::with_dir(&repo.path);
    let raw = git.file_diff(FixtureRepo::BASE_REF, "src/main.rs").unwrap();
    assert!(raw.contains("@@"), "expected unified diff hunk marker");
}

#[test]
fn test_file_diff_for_added_file_has_only_additions() {
    let repo = FixtureRepo::new();
    let git = SystemGit::with_dir(&repo.path);
    let raw = git.file_diff(FixtureRepo::BASE_REF, "src/new.rs").unwrap();
    let file = ChangedFile { path: "src/new.rs".into(), status: FileStatus::Added };
    let diff = parse_diff(&raw, file);
    for hunk in &diff.hunks {
        for line in &hunk.lines {
            assert_ne!(
                line.kind,
                LineKind::Removed,
                "a newly added file should have no removed lines"
            );
        }
    }
}

// ── Diff parsing pipeline (git output → IR) ───────────────────────────────────

#[test]
fn test_parse_diff_main_rs_has_one_hunk() {
    let repo = FixtureRepo::new();
    let git = SystemGit::with_dir(&repo.path);
    let raw = git.file_diff(FixtureRepo::BASE_REF, "src/main.rs").unwrap();
    let file = ChangedFile { path: "src/main.rs".into(), status: FileStatus::Modified };
    let diff = parse_diff(&raw, file);
    assert_eq!(diff.hunks.len(), 1);
}

#[test]
fn test_parse_diff_main_rs_has_added_and_removed_lines() {
    let repo = FixtureRepo::new();
    let git = SystemGit::with_dir(&repo.path);
    let raw = git.file_diff(FixtureRepo::BASE_REF, "src/main.rs").unwrap();
    let file = ChangedFile { path: "src/main.rs".into(), status: FileStatus::Modified };
    let diff = parse_diff(&raw, file);

    let hunk = &diff.hunks[0];
    let added = hunk.lines.iter().filter(|l| l.kind == LineKind::Added).count();
    let removed = hunk.lines.iter().filter(|l| l.kind == LineKind::Removed).count();

    // HEAD_MAIN adds 2 lines, removes 1
    assert_eq!(added, 2, "expected 2 added lines in main.rs hunk");
    assert_eq!(removed, 1, "expected 1 removed line in main.rs hunk");
}

#[test]
fn test_parse_diff_main_rs_added_line_content() {
    let repo = FixtureRepo::new();
    let git = SystemGit::with_dir(&repo.path);
    let raw = git.file_diff(FixtureRepo::BASE_REF, "src/main.rs").unwrap();
    let file = ChangedFile { path: "src/main.rs".into(), status: FileStatus::Modified };
    let diff = parse_diff(&raw, file);

    let added_contents: Vec<&str> = diff.hunks[0]
        .lines
        .iter()
        .filter(|l| l.kind == LineKind::Added)
        .map(|l| l.content.as_str())
        .collect();

    assert!(
        added_contents.iter().any(|c| c.contains("Hello, delta!")),
        "expected added line to contain 'Hello, delta!', got: {:?}",
        added_contents
    );
}

#[test]
fn test_parse_diff_lib_rs_has_only_additions() {
    let repo = FixtureRepo::new();
    let git = SystemGit::with_dir(&repo.path);
    let raw = git.file_diff(FixtureRepo::BASE_REF, "src/lib.rs").unwrap();
    let file = ChangedFile { path: "src/lib.rs".into(), status: FileStatus::Modified };
    let diff = parse_diff(&raw, file);

    let removed = diff.hunks.iter()
        .flat_map(|h| &h.lines)
        .filter(|l| l.kind == LineKind::Removed)
        .count();
    assert_eq!(removed, 0, "lib.rs only adds lines, expected no removals");
}

#[test]
fn test_parse_diff_new_file_has_hunk_starting_at_line_one() {
    let repo = FixtureRepo::new();
    let git = SystemGit::with_dir(&repo.path);
    let raw = git.file_diff(FixtureRepo::BASE_REF, "src/new.rs").unwrap();
    let file = ChangedFile { path: "src/new.rs".into(), status: FileStatus::Added };
    let diff = parse_diff(&raw, file);

    assert!(!diff.hunks.is_empty());
    assert_eq!(diff.hunks[0].new_start, 1);
}

#[test]
fn test_parse_diff_added_lines_have_new_line_numbers() {
    let repo = FixtureRepo::new();
    let git = SystemGit::with_dir(&repo.path);
    let raw = git.file_diff(FixtureRepo::BASE_REF, "src/main.rs").unwrap();
    let file = ChangedFile { path: "src/main.rs".into(), status: FileStatus::Modified };
    let diff = parse_diff(&raw, file);

    for hunk in &diff.hunks {
        for line in &hunk.lines {
            if line.kind == LineKind::Added {
                assert!(
                    line.new_lineno.is_some(),
                    "added line should have a new_lineno"
                );
                assert!(
                    line.old_lineno.is_none(),
                    "added line should not have an old_lineno"
                );
            }
        }
    }
}
