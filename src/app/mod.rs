use std::collections::HashSet;
use std::path::PathBuf;

use crate::diff::ChangedFile;
use crate::filetree::{TreeItem, build_tree};
use crate::git::WhitespaceMode;
use crate::segment::RichDiffFile;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum ViewMode {
    #[default]
    Inline,
    SideBySide,
}

pub(crate) mod layout;
mod navigation;
mod notes;

pub(crate) use layout::{delete_selection, selected_range};

/// Context runs of this many lines or more are folded by default.
pub(crate) const FOLD_THRESHOLD: usize = 6;

/// Inner width (in terminal columns) of the file-list panel.
/// File list is rendered at Constraint::Length(32); minus 2 borders = 30.
pub(crate) const FILE_LIST_INNER_WIDTH: usize = 30;

#[derive(Debug, Clone, Default, PartialEq)]
pub enum Panel {
    #[default]
    FileList,
    DiffView,
    NotesView,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineRange {
    pub start: u32,
    pub end: u32,
}

impl LineRange {
    pub fn new(a: u32, b: u32) -> Self {
        if a <= b { Self { start: a, end: b } } else { Self { start: b, end: a } }
    }
    pub fn contains(&self, n: u32) -> bool { n >= self.start && n <= self.end }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub enum Mode {
    #[default]
    Normal,
    LineSelect {
        hunk_idx: usize,
        /// Index into hunk.lines; fixed when `v` is pressed.
        anchor_line: usize,
        /// Index into hunk.lines; moves with ↑/↓.
        active_line: usize,
    },
    Comment {
        hunk_idx: usize,
        input: String,
        /// Byte offset of the insertion cursor, always on a char boundary.
        cursor: usize,
        /// The note being replaced, if this is an edit rather than a new comment.
        /// Restored on Esc so cancelling an edit never loses the original.
        original: Option<FeedbackNote>,
        /// When Some, this comment targets a line range rather than the full hunk.
        line_range: Option<LineRange>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct FeedbackNote {
    pub file: PathBuf,
    pub hunk_header: String,
    pub hunk_content: String,
    pub note: String,
    pub line_range: Option<LineRange>,
}

pub struct App {
    pub from: String,
    pub to: String,
    pub files: Vec<ChangedFile>,
    pub selected_file: usize,
    pub focused_panel: Panel,
    pub current_rich_diff: Option<RichDiffFile>,
    pub highlighter: crate::highlight::SyntaxHighlighter,
    pub diff_scroll: usize,
    pub selected_hunk: usize,
    pub notes: Vec<FeedbackNote>,
    pub mode: Mode,
    /// Hunk indices whose context runs have been expanded by the user.
    /// All other hunks have long context runs folded by default.
    pub expanded_hunks: HashSet<usize>,
    /// Which note is selected in the Notes panel.
    pub selected_note: usize,
    /// Which notes are expanded to show full text in the Notes panel.
    pub expanded_notes: HashSet<usize>,
    /// Scroll offset within the comment popup (logical lines).
    pub comment_scroll: usize,
    /// Byte offset of the selection anchor; `None` means no active selection.
    /// The selected range is `min(cursor, anchor)..max(cursor, anchor)`.
    pub comment_anchor: Option<usize>,
    /// Scroll offset (visual rows) for the notes panel. Updated after every navigation action
    /// so the selected note is always visible within the panel viewport.
    pub notes_scroll: usize,
    /// Inner width of the diff panel (terminal columns, excluding file-list panel and borders).
    /// Updated by the event loop before each draw. Used to compute accurate visual row counts
    /// when wrap is enabled. Zero means "no wrap accounting" (treats every line as 1 row).
    pub diff_view_content_width: usize,
    /// Directories the user has collapsed in the file tree. An empty set means all dirs are expanded.
    pub collapsed_dirs: HashSet<PathBuf>,
    /// Cursor position within the visible file tree (index into `tree_items()`).
    pub file_tree_cursor: usize,
    /// Horizontal scroll offset (in character columns) for the scrollable name portion
    /// of file-list items. Adjusted with ←/→ when the file-list panel is focused.
    pub file_list_h_scroll: usize,
    /// Active whitespace-sensitivity mode for `git diff`. Cycled with `w` in the diff view.
    pub whitespace_mode: WhitespaceMode,
    /// Whether the diff panel shows inline (unified) or side-by-side layout. Toggled with `s`.
    pub view_mode: ViewMode,
}

impl App {
    pub fn new(files: Vec<ChangedFile>, from: String, to: String) -> Self {
        Self {
            from,
            to,
            files,
            selected_file: 0,
            focused_panel: Panel::FileList,
            current_rich_diff: None,
            highlighter: crate::highlight::SyntaxHighlighter::new(),
            diff_scroll: 0,
            selected_hunk: 0,
            notes: Vec::new(),
            mode: Mode::Normal,
            expanded_hunks: HashSet::new(),
            selected_note: 0,
            expanded_notes: HashSet::new(),
            comment_scroll: 0,
            comment_anchor: None,
            notes_scroll: 0,
            diff_view_content_width: 0,
            collapsed_dirs: HashSet::new(),
            file_tree_cursor: 0,
            file_list_h_scroll: 0,
            whitespace_mode: WhitespaceMode::None,
            view_mode: ViewMode::Inline,
        }
    }

    /// Build the visible file tree from the current files, notes, and collapsed state.
    pub fn tree_items(&self) -> Vec<TreeItem> {
        let noted: HashSet<PathBuf> = self.notes.iter().map(|n| n.file.clone()).collect();
        build_tree(&self.files, &noted, &self.collapsed_dirs)
    }

    pub fn select_file(&mut self, idx: usize) {
        if idx < self.files.len() {
            self.selected_file = idx;
            self.diff_scroll = 0;
            self.selected_hunk = 0;
            self.current_rich_diff = None;
            self.expanded_hunks.clear();
        }
    }
}

/// Shared helpers available to all in-module test blocks within the `app` subtree.
#[cfg(test)]
pub(crate) mod test_helpers {
    use super::{App, Mode};
    use crate::diff::{ChangedFile, DiffLine, FileStatus, LineKind};
    use crate::segment::{RichDiffFile, RichHunk, RichLine, Segment};
    use ratatui::style::Color;
    use std::path::PathBuf;

    pub(crate) fn make_files(n: usize) -> Vec<ChangedFile> {
        (0..n)
            .map(|i| ChangedFile {
                path: PathBuf::from(format!("file_{}.rs", i)),
                status: FileStatus::Modified,
                old_path: None,
            })
            .collect()
    }

    /// Build a `RichLine` with a single segment (no syntax splitting).
    /// Used by test helpers that don't need accurate syntax coloring.
    pub(crate) fn make_rich_line(dl: DiffLine) -> RichLine {
        let fg = match dl.kind {
            LineKind::Added   => Color::Green,
            LineKind::Removed => Color::Red,
            LineKind::Context => Color::Gray,
        };
        let bg = match dl.kind {
            LineKind::Added   => Some(Color::Rgb(0, 60, 0)),
            LineKind::Removed => Some(Color::Rgb(70, 0, 0)),
            LineKind::Context => None,
        };
        let seg = Segment { content: dl.content.clone(), fg, bg };
        RichLine { diff_line: dl, segments: vec![seg] }
    }

    pub(crate) fn make_rich_hunk(header: &str) -> RichHunk {
        let lines = vec![
            DiffLine { old_lineno: None,    new_lineno: Some(1), kind: LineKind::Added,   content: "new line".to_string() },
            DiffLine { old_lineno: Some(1), new_lineno: None,    kind: LineKind::Removed, content: "old line".to_string() },
            DiffLine { old_lineno: Some(2), new_lineno: Some(2), kind: LineKind::Context, content: "context".to_string()  },
        ];
        RichHunk {
            header: header.to_string(),
            old_start: 1,
            new_start: 1,
            lines: lines.into_iter().map(make_rich_line).collect(),
        }
    }

    pub(crate) fn app_with_diff(hunk_count: usize) -> App {
        let files = make_files(1);
        let mut app = App::new(files.clone(), "main".to_string(), "HEAD".to_string());
        app.current_rich_diff = Some(RichDiffFile {
            file: files[0].clone(),
            hunks: (0..hunk_count)
                .map(|i| make_rich_hunk(&format!("@@ -{},3 +{},4 @@", i * 10 + 1, i * 10 + 1)))
                .collect(),
        });
        app
    }

    pub(crate) fn app_with_note_on_hunk(hunk_idx: usize) -> App {
        let mut app = app_with_diff(3);
        app.selected_hunk = hunk_idx;
        app.mode = Mode::Comment {
            hunk_idx,
            input: "original note".to_string(),
            cursor: 0,
            original: None,
            line_range: None,
        };
        app.submit_comment();
        app.selected_hunk = hunk_idx;
        app
    }

    pub(crate) fn app_at_file(file_idx: usize) -> App {
        let mut app = App::new(make_files(2), "main".to_string(), "HEAD".to_string());
        app.file_tree_cursor = file_idx;
        app.selected_file = file_idx;
        app
    }

    pub(crate) fn app_with_two_file_notes() -> App {
        let mut app = app_with_diff(2);
        app.mode = Mode::Comment {
            hunk_idx: 0,
            input: "first note".to_string(),
            cursor: 0,
            original: None,
            line_range: None,
        };
        app.submit_comment();
        app.selected_hunk = 1;
        app.mode = Mode::Comment {
            hunk_idx: 1,
            input: "second note".to_string(),
            cursor: 0,
            original: None,
            line_range: None,
        };
        app.submit_comment();
        app.selected_note = 0;
        app
    }

    pub(crate) fn app_with_many_notes(n: usize) -> App {
        let mut app = app_with_diff(3);
        for hunk_idx in 0..n.min(3) {
            app.selected_hunk = hunk_idx;
            app.mode = Mode::Comment {
                hunk_idx,
                input: format!("note {}", hunk_idx),
                cursor: 0,
                original: None,
                line_range: None,
            };
            app.submit_comment();
        }
        app.selected_note = 0;
        app
    }

    pub(crate) fn dir_files() -> Vec<ChangedFile> {
        vec![
            ChangedFile { path: PathBuf::from("src/a.rs"), status: FileStatus::Modified, old_path: None },
            ChangedFile { path: PathBuf::from("src/b.rs"), status: FileStatus::Modified, old_path: None },
        ]
    }

    /// Build `RichLine`s from a list of `LineKind`s with placeholder content.
    /// Used by layout tests that only care about line kinds and visual row counts.
    pub(crate) fn make_rich_lines(kinds: &[LineKind]) -> Vec<RichLine> {
        kinds.iter().map(|k| {
            make_rich_line(DiffLine {
                old_lineno: Some(1),
                new_lineno: Some(1),
                kind: *k,
                content: "x".to_string(),
            })
        }).collect()
    }

    #[cfg(test)]
    mod line_range_tests {
        use super::super::LineRange;

        #[test]
        fn test_line_range_new_normalizes_order() {
            let r = LineRange::new(10, 5);
            assert_eq!(r.start, 5);
            assert_eq!(r.end, 10);
        }

        #[test]
        fn test_line_range_new_forward_unchanged() {
            let r = LineRange::new(3, 7);
            assert_eq!(r.start, 3);
            assert_eq!(r.end, 7);
        }

        #[test]
        fn test_line_range_contains_start() {
            let r = LineRange::new(5, 10);
            assert!(r.contains(5));
        }

        #[test]
        fn test_line_range_contains_end() {
            let r = LineRange::new(5, 10);
            assert!(r.contains(10));
        }

        #[test]
        fn test_line_range_contains_middle() {
            let r = LineRange::new(5, 10);
            assert!(r.contains(7));
        }

        #[test]
        fn test_line_range_contains_outside_below() {
            let r = LineRange::new(5, 10);
            assert!(!r.contains(4));
        }

        #[test]
        fn test_line_range_contains_outside_above() {
            let r = LineRange::new(5, 10);
            assert!(!r.contains(11));
        }
    }
}
