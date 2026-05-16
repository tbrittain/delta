//! File-tree construction for the file-list panel.
//!
//! Given a flat `&[ChangedFile]` (from the git backend), this module builds
//! an in-memory directory trie and flattens it into a `Vec<TreeItem>` that
//! the UI can render with indentation and expand/collapse support.

mod node;

use std::collections::HashSet;
use std::path::PathBuf;

use crate::diff::ChangedFile;

use node::{insert_file, sort_nodes, flatten};

// ── Public API ────────────────────────────────────────────────────────────────

/// One visible row in the file-tree panel.
#[derive(Debug, Clone, PartialEq)]
pub enum TreeItem {
    /// A directory node.
    Dir {
        /// Full relative path (used as key in `collapsed_dirs`).
        path: PathBuf,
        /// Short name shown in the panel (last component + "/").
        display_name: String,
        /// Nesting depth (0 = top-level).
        depth: usize,
        /// Total changed-file count anywhere under this directory.
        file_count: usize,
        /// True when any file under this directory has a reviewer note.
        has_notes: bool,
        /// True when the user has collapsed this directory.
        collapsed: bool,
    },
    /// A changed file.
    File {
        /// Index into `App.files`.
        file_idx: usize,
        /// Filename shown in the panel.
        display_name: String,
        depth: usize,
        has_notes: bool,
    },
}

impl TreeItem {
    pub fn depth(&self) -> usize {
        match self {
            Self::Dir  { depth, .. } | Self::File { depth, .. } => *depth,
        }
    }

    pub fn is_dir(&self) -> bool {
        matches!(self, Self::Dir { .. })
    }

    pub fn file_idx(&self) -> Option<usize> {
        match self {
            Self::File { file_idx, .. } => Some(*file_idx),
            Self::Dir  { .. }           => None,
        }
    }

    pub fn dir_path(&self) -> Option<&PathBuf> {
        match self {
            Self::Dir  { path, .. } => Some(path),
            Self::File { .. }       => None,
        }
    }
}

/// Build the flat visible tree from a set of changed files.
///
/// `noted_files` — paths that have at least one reviewer note (passed as a
/// `HashSet<PathBuf>` to avoid depending on `app::FeedbackNote` here).
///
/// Directories in `collapsed_dirs` appear with their contents hidden.
/// All other directories are expanded.
///
/// Within each level, directories are sorted before files; both groups are
/// sorted alphabetically.
pub fn build_tree(
    files: &[ChangedFile],
    noted_files: &HashSet<PathBuf>,
    collapsed_dirs: &HashSet<PathBuf>,
) -> Vec<TreeItem> {
    let mut root = Vec::new();
    for (idx, file) in files.iter().enumerate() {
        let comps: Vec<_> = file.path.components().collect();
        insert_file(&mut root, idx, &comps, &PathBuf::new());
    }
    sort_nodes(&mut root, files);

    let mut result = Vec::new();
    flatten(&root, files, noted_files, collapsed_dirs, 0, &mut result);
    result
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::FileStatus;

    fn file(path: &str) -> ChangedFile {
        ChangedFile { path: PathBuf::from(path), status: FileStatus::Modified, old_path: None }
    }

    fn no_notes() -> HashSet<PathBuf> { HashSet::new() }
    fn no_collapse() -> HashSet<PathBuf> { HashSet::new() }

    // ── Flat lists (no directories) ───────────────────────────────────────────

    #[test]
    fn test_flat_files_at_root() {
        let files = vec![file("app.rs"), file("main.rs")];
        let items = build_tree(&files, &no_notes(), &no_collapse());
        assert_eq!(items.len(), 2);
        assert!(matches!(&items[0], TreeItem::File { display_name, .. } if display_name == "app.rs"));
        assert!(matches!(&items[1], TreeItem::File { display_name, .. } if display_name == "main.rs"));
    }

    #[test]
    fn test_flat_files_preserve_file_idx() {
        let files = vec![file("z.rs"), file("a.rs")];
        let items = build_tree(&files, &no_notes(), &no_collapse());
        // sorted alphabetically: a.rs first
        assert_eq!(items[0].file_idx(), Some(1)); // a.rs = index 1
        assert_eq!(items[1].file_idx(), Some(0)); // z.rs = index 0
    }

    // ── Directory grouping ────────────────────────────────────────────────────

    #[test]
    fn test_files_in_one_dir() {
        let files = vec![file("src/a.rs"), file("src/b.rs")];
        let items = build_tree(&files, &no_notes(), &no_collapse());
        // Dir + 2 files = 3 items
        assert_eq!(items.len(), 3);
        assert!(matches!(&items[0], TreeItem::Dir { display_name, .. } if display_name == "src/"));
        assert_eq!(items[0].depth(), 0);
        assert_eq!(items[1].depth(), 1);
        assert_eq!(items[2].depth(), 1);
    }

    #[test]
    fn test_dirs_sorted_before_files() {
        let files = vec![file("CLAUDE.md"), file("src/app.rs")];
        let items = build_tree(&files, &no_notes(), &no_collapse());
        // Directory (src/) should come before root file (CLAUDE.md)
        assert!(items[0].is_dir());
        assert!(!items[items.len() - 1].is_dir());
    }

    #[test]
    fn test_nested_dirs() {
        let files = vec![file("src/ui/mod.rs"), file("src/app.rs")];
        let items = build_tree(&files, &no_notes(), &no_collapse());
        // dirs-first sort within each level:
        // src/ (depth 0), ui/ (depth 1), mod.rs (depth 2), app.rs (depth 1)
        assert_eq!(items.len(), 4);
        assert_eq!(items[0].depth(), 0); // src/
        assert_eq!(items[1].depth(), 1); // ui/ (dir before file)
        assert_eq!(items[2].depth(), 2); // mod.rs
        assert_eq!(items[3].depth(), 1); // app.rs
    }

    #[test]
    fn test_dir_file_count() {
        let files = vec![file("src/a.rs"), file("src/b.rs"), file("root.rs")];
        let items = build_tree(&files, &no_notes(), &no_collapse());
        if let TreeItem::Dir { file_count, .. } = &items[0] {
            assert_eq!(*file_count, 2);
        } else {
            panic!("expected dir as first item");
        }
    }

    // ── Collapse / expand ─────────────────────────────────────────────────────

    #[test]
    fn test_collapsed_dir_hides_children() {
        let files = vec![file("src/a.rs"), file("src/b.rs")];
        let mut collapsed = HashSet::new();
        collapsed.insert(PathBuf::from("src"));
        let items = build_tree(&files, &no_notes(), &collapsed);
        assert_eq!(items.len(), 1); // only the dir node, no children
        assert!(matches!(&items[0], TreeItem::Dir { collapsed: true, .. }));
    }

    #[test]
    fn test_expanded_dir_shows_children() {
        let files = vec![file("src/a.rs"), file("src/b.rs")];
        let items = build_tree(&files, &no_notes(), &no_collapse());
        assert_eq!(items.len(), 3);
        assert!(matches!(&items[0], TreeItem::Dir { collapsed: false, .. }));
    }

    #[test]
    fn test_collapsing_parent_hides_nested_dirs() {
        let files = vec![file("src/ui/mod.rs"), file("src/app.rs")];
        let mut collapsed = HashSet::new();
        collapsed.insert(PathBuf::from("src"));
        let items = build_tree(&files, &no_notes(), &collapsed);
        assert_eq!(items.len(), 1); // only src/, its subtree hidden
    }

    // ── Notes propagation ─────────────────────────────────────────────────────

    #[test]
    fn test_file_has_notes_marker() {
        let files = vec![file("a.rs"), file("b.rs")];
        let mut noted = HashSet::new();
        noted.insert(PathBuf::from("a.rs"));
        let items = build_tree(&files, &noted, &no_collapse());
        assert!(matches!(&items[0], TreeItem::File { has_notes: true, .. }));
        assert!(matches!(&items[1], TreeItem::File { has_notes: false, .. }));
    }

    #[test]
    fn test_dir_has_notes_when_child_does() {
        let files = vec![file("src/a.rs"), file("src/b.rs")];
        let mut noted = HashSet::new();
        noted.insert(PathBuf::from("src/b.rs"));
        let items = build_tree(&files, &noted, &no_collapse());
        // Dir should inherit the note marker from its child
        assert!(matches!(&items[0], TreeItem::Dir { has_notes: true, .. }));
    }

    #[test]
    fn test_dir_no_notes_when_no_child_has_note() {
        let files = vec![file("src/a.rs"), file("src/b.rs")];
        let items = build_tree(&files, &no_notes(), &no_collapse());
        assert!(matches!(&items[0], TreeItem::Dir { has_notes: false, .. }));
    }

    // ── Rename display ────────────────────────────────────────────────────────

    #[test]
    fn test_renamed_file_shows_old_and_new_name() {
        let files = vec![ChangedFile {
            path: PathBuf::from("src/new_name.rs"),
            status: FileStatus::Renamed,
            old_path: Some(PathBuf::from("src/old_name.rs")),
        }];
        let items = build_tree(&files, &no_notes(), &no_collapse());
        // File is inside src/ → tree has Dir + File
        let file_item = items.iter().find(|i| i.file_idx().is_some()).unwrap();
        if let TreeItem::File { display_name, .. } = file_item {
            assert!(display_name.contains("old_name.rs"), "should show old name; got: {display_name}");
            assert!(display_name.contains("→"),           "should contain arrow; got: {display_name}");
            assert!(display_name.contains("new_name.rs"), "should show new name; got: {display_name}");
        } else {
            panic!("expected File item");
        }
    }

    #[test]
    fn test_non_renamed_file_shows_only_filename() {
        let files = vec![file("src/main.rs")];
        let items = build_tree(&files, &no_notes(), &no_collapse());
        let file_item = items.iter().find(|i| i.file_idx().is_some()).unwrap();
        if let TreeItem::File { display_name, .. } = file_item {
            assert_eq!(display_name, "main.rs");
            assert!(!display_name.contains("→"));
        } else {
            panic!("expected File item");
        }
    }
}
