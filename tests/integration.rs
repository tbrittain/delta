mod common;

use std::process::{Command, Stdio};

use common::FixtureRepo;
use delta::app::App;
use delta::diff::{ChangedFile, FileStatus, LineKind, parse_diff};
use delta::export;
use delta::git::{GitBackend, SystemGit, WhitespaceMode};

// ── Git integration layer ─────────────────────────────────────────────────────

#[test]
fn test_changed_files_returns_all_five() {
    let repo = FixtureRepo::new();
    let git = SystemGit::with_dir(&repo.path);
    let files = git.changed_files(FixtureRepo::FROM_REF, FixtureRepo::TO_REF).unwrap();
    assert_eq!(files.len(), 5);
}

#[test]
fn test_changed_files_includes_renamed() {
    let repo = FixtureRepo::new();
    let git = SystemGit::with_dir(&repo.path);
    let files = git.changed_files(FixtureRepo::FROM_REF, FixtureRepo::TO_REF).unwrap();
    let renamed: Vec<_> = files.iter().filter(|f| f.status == FileStatus::Renamed).collect();
    assert_eq!(renamed.len(), 1);
    // Must show the new path, not the old one
    assert!(renamed[0].path.ends_with("renamed.rs"), "got {:?}", renamed[0].path);
    assert!(!renamed[0].path.to_string_lossy().contains("old_name"));
}

#[test]
fn test_changed_files_includes_modified() {
    let repo = FixtureRepo::new();
    let git = SystemGit::with_dir(&repo.path);
    let files = git.changed_files(FixtureRepo::FROM_REF, FixtureRepo::TO_REF).unwrap();
    let has_modified = files.iter().any(|f| f.status == FileStatus::Modified);
    assert!(has_modified, "expected at least one Modified file");
}

#[test]
fn test_changed_files_includes_added() {
    let repo = FixtureRepo::new();
    let git = SystemGit::with_dir(&repo.path);
    let files = git.changed_files(FixtureRepo::FROM_REF, FixtureRepo::TO_REF).unwrap();
    let added: Vec<_> = files.iter().filter(|f| f.status == FileStatus::Added).collect();
    assert_eq!(added.len(), 1);
    assert!(added[0].path.ends_with("new.rs"));
}

#[test]
fn test_changed_files_includes_deleted() {
    let repo = FixtureRepo::new();
    let git = SystemGit::with_dir(&repo.path);
    let files = git.changed_files(FixtureRepo::FROM_REF, FixtureRepo::TO_REF).unwrap();
    let deleted: Vec<_> = files.iter().filter(|f| f.status == FileStatus::Deleted).collect();
    assert_eq!(deleted.len(), 1);
    assert!(deleted[0].path.ends_with("deleted.rs"));
}

#[test]
fn test_changed_files_paths_are_relative() {
    let repo = FixtureRepo::new();
    let git = SystemGit::with_dir(&repo.path);
    let files = git.changed_files(FixtureRepo::FROM_REF, FixtureRepo::TO_REF).unwrap();
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
    let raw = git.file_diff(FixtureRepo::FROM_REF, FixtureRepo::TO_REF, "src/main.rs", WhitespaceMode::None).unwrap();
    assert!(!raw.is_empty());
}

#[test]
fn test_file_diff_contains_hunk_marker() {
    let repo = FixtureRepo::new();
    let git = SystemGit::with_dir(&repo.path);
    let raw = git.file_diff(FixtureRepo::FROM_REF, FixtureRepo::TO_REF, "src/main.rs", WhitespaceMode::None).unwrap();
    assert!(raw.contains("@@"), "expected unified diff hunk marker");
}

#[test]
fn test_file_diff_for_added_file_has_only_additions() {
    let repo = FixtureRepo::new();
    let git = SystemGit::with_dir(&repo.path);
    let raw = git.file_diff(FixtureRepo::FROM_REF, FixtureRepo::TO_REF, "src/new.rs", WhitespaceMode::None).unwrap();
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
    let raw = git.file_diff(FixtureRepo::FROM_REF, FixtureRepo::TO_REF, "src/main.rs", WhitespaceMode::None).unwrap();
    let file = ChangedFile { path: "src/main.rs".into(), status: FileStatus::Modified };
    let diff = parse_diff(&raw, file);
    assert_eq!(diff.hunks.len(), 1);
}

#[test]
fn test_parse_diff_main_rs_has_added_and_removed_lines() {
    let repo = FixtureRepo::new();
    let git = SystemGit::with_dir(&repo.path);
    let raw = git.file_diff(FixtureRepo::FROM_REF, FixtureRepo::TO_REF, "src/main.rs", WhitespaceMode::None).unwrap();
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
    let raw = git.file_diff(FixtureRepo::FROM_REF, FixtureRepo::TO_REF, "src/main.rs", WhitespaceMode::None).unwrap();
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
    let raw = git.file_diff(FixtureRepo::FROM_REF, FixtureRepo::TO_REF, "src/lib.rs", WhitespaceMode::None).unwrap();
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
    let raw = git.file_diff(FixtureRepo::FROM_REF, FixtureRepo::TO_REF, "src/new.rs", WhitespaceMode::None).unwrap();
    let file = ChangedFile { path: "src/new.rs".into(), status: FileStatus::Added };
    let diff = parse_diff(&raw, file);

    assert!(!diff.hunks.is_empty());
    assert_eq!(diff.hunks[0].new_start, 1);
}

#[test]
fn test_parse_diff_added_lines_have_new_line_numbers() {
    let repo = FixtureRepo::new();
    let git = SystemGit::with_dir(&repo.path);
    let raw = git.file_diff(FixtureRepo::FROM_REF, FixtureRepo::TO_REF, "src/main.rs", WhitespaceMode::None).unwrap();
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

// ── Subdirectory invocation ───────────────────────────────────────────────────
//
// Regression: when delta is run from inside a repo subdirectory, git diff
// --name-status returns paths relative to the repo root, but a subsequent
// `git diff -- <path>` executed from the subdirectory interprets the pathspec
// relative to cwd, so nothing matches and stdout is empty.  new_at() resolves
// the repo root first so both calls run from the same place.

#[test]
fn test_changed_files_from_subdirectory() {
    let repo = FixtureRepo::new();
    // Simulate delta being run from inside the src/ subdirectory.
    let git = SystemGit::new_at(&repo.path.join("src"));
    let files = git.changed_files(FixtureRepo::FROM_REF, FixtureRepo::TO_REF).unwrap();
    assert_eq!(files.len(), 5, "should enumerate all changed files when run from a repo subdirectory");
}

#[test]
fn test_file_diff_from_subdirectory_returns_nonempty() {
    let repo = FixtureRepo::new();
    let git = SystemGit::new_at(&repo.path.join("src"));
    let raw = git.file_diff(FixtureRepo::FROM_REF, FixtureRepo::TO_REF, "src/main.rs", WhitespaceMode::None).unwrap();
    assert!(!raw.is_empty(), "file_diff should return content when run from a repo subdirectory");
    assert!(raw.contains("@@"), "should contain unified diff hunk marker");
}

// ── Arbitrary range comparison ────────────────────────────────────────────────

#[test]
fn test_same_ref_returns_no_changes() {
    // Diffing a ref against itself should produce an empty file list.
    let repo = FixtureRepo::new();
    let git = SystemGit::with_dir(&repo.path);
    let files = git.changed_files("HEAD", "HEAD").unwrap();
    assert!(files.is_empty(), "diff of HEAD..HEAD should be empty");
}

#[test]
fn test_explicit_to_ref_matches_implicit_head() {
    // delta main  (to defaults to HEAD)
    // delta HEAD^ HEAD  (to explicit)
    // Both should return the same set of changed files.
    let repo = FixtureRepo::new();
    let git = SystemGit::with_dir(&repo.path);
    let implicit = git.changed_files(FixtureRepo::FROM_REF, "HEAD").unwrap();
    let explicit = git.changed_files(FixtureRepo::FROM_REF, FixtureRepo::TO_REF).unwrap();
    let implicit_paths: std::collections::HashSet<_> =
        implicit.iter().map(|f| &f.path).collect();
    let explicit_paths: std::collections::HashSet<_> =
        explicit.iter().map(|f| &f.path).collect();
    assert_eq!(implicit_paths, explicit_paths);
}

#[test]
fn test_file_diff_with_explicit_to_ref() {
    // file_diff with an explicit to ref should return the same content as with HEAD.
    let repo = FixtureRepo::new();
    let git = SystemGit::with_dir(&repo.path);
    let diff_implicit = git.file_diff(FixtureRepo::FROM_REF, "HEAD", "src/main.rs", WhitespaceMode::None).unwrap();
    let diff_explicit = git.file_diff(FixtureRepo::FROM_REF, FixtureRepo::TO_REF, "src/main.rs", WhitespaceMode::None).unwrap();
    assert_eq!(diff_implicit, diff_explicit);
}

#[test]
fn test_reversed_range_shows_inverse_diff() {
    // Swapping from and to should swap additions and removals.
    let repo = FixtureRepo::new();
    let git = SystemGit::with_dir(&repo.path);

    let forward = git.file_diff(FixtureRepo::FROM_REF, FixtureRepo::TO_REF, "src/main.rs", WhitespaceMode::None).unwrap();
    let backward = git.file_diff(FixtureRepo::TO_REF, FixtureRepo::FROM_REF, "src/main.rs", WhitespaceMode::None).unwrap();

    // Forward diff adds "delta"; backward diff removes it (shows as addition from the inverse perspective)
    assert!(forward.contains("+") && forward.contains("-"));
    assert!(backward.contains("+") && backward.contains("-"));
    // The content that was added in the forward direction is removed in the backward direction
    assert!(forward.contains(r#"+"Hello, delta!"#) || forward.contains("delta"));
    assert_ne!(forward, backward, "reversed range should produce a different diff");
}

// ── Full pipeline (git → parse → app → export) ────────────────────────────────
//
// These tests substitute for a manual TUI smoke test. They exercise the entire
// data pipeline — enumerating files, loading and parsing diffs, driving app
// state, and producing export output — without requiring a real terminal.

#[test]
fn test_pipeline_enumerates_correct_files() {
    let repo = FixtureRepo::new();
    let git = SystemGit::with_dir(&repo.path);
    let files = git.changed_files(FixtureRepo::FROM_REF, FixtureRepo::TO_REF).unwrap();

    assert!(!files.is_empty());
    let paths: Vec<String> = files.iter().map(|f| f.path.to_string_lossy().into()).collect();
    assert!(paths.iter().any(|p| p.contains("main.rs")));
    assert!(paths.iter().any(|p| p.contains("lib.rs")));
    assert!(paths.iter().any(|p| p.contains("new.rs")));
    assert!(paths.iter().any(|p| p.contains("deleted.rs")));
}

#[test]
fn test_pipeline_loads_and_parses_diff_into_app() {
    let repo = FixtureRepo::new();
    let git = SystemGit::with_dir(&repo.path);
    let files = git.changed_files(FixtureRepo::FROM_REF, FixtureRepo::TO_REF).unwrap();

    let mut app = App::new(files, FixtureRepo::FROM_REF.to_string(), FixtureRepo::TO_REF.to_string());

    // Load the diff for the first file manually (as the TUI would on startup)
    let path = app.files[app.selected_file].path.to_string_lossy().to_string();
    let file = app.files[app.selected_file].clone();
    let raw = git.file_diff(FixtureRepo::FROM_REF, FixtureRepo::TO_REF, &path, WhitespaceMode::None).unwrap();
    app.current_diff = Some(parse_diff(&raw, file));

    assert!(app.current_diff.is_some());
    assert!(!app.current_diff.as_ref().unwrap().hunks.is_empty());
}

#[test]
fn test_pipeline_comment_and_markdown_export() {
    let repo = FixtureRepo::new();
    let git = SystemGit::with_dir(&repo.path);
    let files = git.changed_files(FixtureRepo::FROM_REF, FixtureRepo::TO_REF).unwrap();

    let mut app = App::new(files, FixtureRepo::FROM_REF.to_string(), FixtureRepo::TO_REF.to_string());

    // Load diff for src/main.rs
    let main_idx = app.files.iter().position(|f| f.path.ends_with("main.rs")).unwrap();
    app.select_file(main_idx);
    let path = app.files[app.selected_file].path.to_string_lossy().to_string();
    let file = app.files[app.selected_file].clone();
    let raw = git.file_diff(FixtureRepo::FROM_REF, FixtureRepo::TO_REF, &path, WhitespaceMode::None).unwrap();
    app.current_diff = Some(parse_diff(&raw, file));

    // Simulate the user pressing 'c' and submitting a comment
    app.start_comment();
    if let delta::app::Mode::Comment { ref mut input, .. } = app.mode {
        input.push_str("This logging is too verbose");
    }
    app.submit_comment();

    assert_eq!(app.notes.len(), 1);

    // Export and verify structure
    let md = export::to_markdown(&app.notes);
    assert!(md.contains("main.rs"), "export should reference the file");
    assert!(md.contains("This logging is too verbose"), "export should contain the note text");
    assert!(md.contains("@@"), "export should include the hunk header");
}

#[test]
fn test_pipeline_comment_and_json_export() {
    let repo = FixtureRepo::new();
    let git = SystemGit::with_dir(&repo.path);
    let files = git.changed_files(FixtureRepo::FROM_REF, FixtureRepo::TO_REF).unwrap();

    let mut app = App::new(files, FixtureRepo::FROM_REF.to_string(), FixtureRepo::TO_REF.to_string());

    let main_idx = app.files.iter().position(|f| f.path.ends_with("main.rs")).unwrap();
    app.select_file(main_idx);
    let path = app.files[app.selected_file].path.to_string_lossy().to_string();
    let file = app.files[app.selected_file].clone();
    let raw = git.file_diff(FixtureRepo::FROM_REF, FixtureRepo::TO_REF, &path, WhitespaceMode::None).unwrap();
    app.current_diff = Some(parse_diff(&raw, file));

    app.start_comment();
    if let delta::app::Mode::Comment { ref mut input, .. } = app.mode {
        input.push_str("needs a test");
    }
    app.submit_comment();

    let json = export::to_json(&app.notes).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    let note = &parsed["notes"][0];

    assert!(note["file"].as_str().unwrap().contains("main.rs"));
    assert_eq!(note["note"], "needs a test");
}

// ── Whitespace-sensitivity flags ──────────────────────────────────────────────

/// Builds a minimal two-commit repo where `file.rs` changes only in whitespace
/// (indentation doubled in HEAD). Returns the TempDir to keep the repo alive.
fn make_whitespace_only_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let path = dir.path();

    let git = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .current_dir(path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("git failed");
    };

    git(&["init"]);
    git(&["config", "user.email", "test@delta.test"]);
    git(&["config", "user.name", "Delta Test"]);

    // Base: 4-space indent
    std::fs::write(path.join("file.rs"), "fn foo() {\n    let x = 1;\n}\n").unwrap();
    git(&["add", "."]);
    git(&["commit", "-m", "base"]);

    // HEAD: 8-space indent — whitespace-only change
    std::fs::write(path.join("file.rs"), "fn foo() {\n        let x = 1;\n}\n").unwrap();
    git(&["add", "."]);
    git(&["commit", "-m", "whitespace only"]);

    dir
}

#[test]
fn test_file_diff_none_shows_whitespace_changes() {
    let dir = make_whitespace_only_repo();
    let git = SystemGit::with_dir(dir.path());
    let raw = git.file_diff("HEAD^", "HEAD", "file.rs", WhitespaceMode::None).unwrap();
    assert!(raw.contains("@@"), "normal diff should produce a hunk for whitespace changes");
}

#[test]
fn test_file_diff_ignore_all_whitespace_suppresses_changes() {
    let dir = make_whitespace_only_repo();
    let git = SystemGit::with_dir(dir.path());
    let raw = git.file_diff("HEAD^", "HEAD", "file.rs", WhitespaceMode::IgnoreAll).unwrap();
    assert!(
        !raw.contains("@@"),
        "ignore-all-whitespace (-w) should suppress a whitespace-only diff; got: {:?}",
        raw
    );
}

#[test]
fn test_file_diff_ignore_changes_whitespace_suppresses_changes() {
    let dir = make_whitespace_only_repo();
    let git = SystemGit::with_dir(dir.path());
    let raw = git.file_diff("HEAD^", "HEAD", "file.rs", WhitespaceMode::IgnoreChanges).unwrap();
    assert!(
        !raw.contains("@@"),
        "ignore-whitespace-changes (-b) should suppress a whitespace-only diff; got: {:?}",
        raw
    );
}
