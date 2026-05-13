use std::path::Path;
use std::process::{Command, Stdio};
use tempfile::{TempDir, tempdir};

// ── Fixture file contents ─────────────────────────────────────────────────────

const BASE_MAIN: &str = r#"fn main() {
    println!("Hello, world!");
}
"#;

const HEAD_MAIN: &str = r#"fn main() {
    let message = "Hello, delta!";
    println!("{}", message);
}
"#;

const BASE_LIB: &str = r#"pub fn add(a: i32, b: i32) -> i32 {
    a + b
}
"#;

const HEAD_LIB: &str = r#"pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

pub fn subtract(a: i32, b: i32) -> i32 {
    a - b
}
"#;

const HEAD_NEW: &str = r#"pub fn multiply(a: i32, b: i32) -> i32 {
    a * b
}
"#;

const BASE_DELETED: &str = r#"pub fn deprecated() {}
"#;

const BASE_TO_RENAME: &str = r#"pub fn before_rename() {}
"#;

// ── Fixture repo ──────────────────────────────────────────────────────────────

/// A temporary git repository with a known two-commit history:
///
/// - `HEAD^` (base): src/main.rs, src/lib.rs, src/deleted.rs, src/old_name.rs
/// - `HEAD`  (tip):  src/main.rs (modified), src/lib.rs (modified),
///                   src/new.rs (added), src/deleted.rs (deleted),
///                   src/old_name.rs → src/renamed.rs (renamed)
///
/// Use `BASE_REF` as the base argument to `SystemGit` methods.
pub struct FixtureRepo {
    /// The temp directory — kept alive for the lifetime of the fixture.
    _dir: TempDir,
    pub path: std::path::PathBuf,
}

impl FixtureRepo {
    pub const FROM_REF: &'static str = "HEAD^";
    pub const TO_REF: &'static str = "HEAD";

    pub fn new() -> Self {
        let dir = tempdir().expect("failed to create temp dir");
        let path = dir.path().to_path_buf();

        git(&["init"], &path);
        git(&["config", "user.email", "test@delta.test"], &path);
        git(&["config", "user.name", "Delta Test"], &path);

        // Base commit
        std::fs::create_dir_all(path.join("src")).unwrap();
        write_file(&path, "src/main.rs", BASE_MAIN);
        write_file(&path, "src/lib.rs", BASE_LIB);
        write_file(&path, "src/deleted.rs", BASE_DELETED);
        write_file(&path, "src/old_name.rs", BASE_TO_RENAME);
        git(&["add", "."], &path);
        git(&["commit", "-m", "base commit"], &path);

        // Feature commit (HEAD)
        write_file(&path, "src/main.rs", HEAD_MAIN);
        write_file(&path, "src/lib.rs", HEAD_LIB);
        write_file(&path, "src/new.rs", HEAD_NEW);
        std::fs::remove_file(path.join("src/deleted.rs")).unwrap();
        git(&["mv", "src/old_name.rs", "src/renamed.rs"], &path);
        git(&["add", "."], &path);
        git(&["commit", "-m", "feature changes"], &path);

        FixtureRepo { _dir: dir, path }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn git(args: &[&str], dir: &Path) {
    let status = Command::new("git")
        .args(args)
        .current_dir(dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap_or_else(|_| panic!("failed to spawn git {:?}", args));
    assert!(status.success(), "git {:?} failed in {:?}", args, dir);
}

fn write_file(base: &Path, rel: &str, content: &str) {
    std::fs::write(base.join(rel), content).unwrap();
}
