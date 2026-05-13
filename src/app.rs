use std::path::PathBuf;

use crate::diff::{ChangedFile, DiffFile, LineKind};

#[derive(Debug, Clone, Default, PartialEq)]
pub enum Panel {
    #[default]
    FileList,
    DiffView,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub enum Mode {
    #[default]
    Normal,
    Comment {
        hunk_idx: usize,
        input: String,
    },
}

#[derive(Debug, Clone)]
pub struct FeedbackNote {
    pub file: PathBuf,
    pub hunk_header: String,
    pub hunk_content: String,
    pub note: String,
}

pub struct App {
    pub base: String,
    pub files: Vec<ChangedFile>,
    pub selected_file: usize,
    pub focused_panel: Panel,
    pub current_diff: Option<DiffFile>,
    pub diff_scroll: usize,
    pub selected_hunk: usize,
    pub notes: Vec<FeedbackNote>,
    pub mode: Mode,
}

impl App {
    pub fn new(files: Vec<ChangedFile>, base: String) -> Self {
        Self {
            base,
            files,
            selected_file: 0,
            focused_panel: Panel::FileList,
            current_diff: None,
            diff_scroll: 0,
            selected_hunk: 0,
            notes: Vec::new(),
            mode: Mode::Normal,
        }
    }

    pub fn select_file(&mut self, idx: usize) {
        if idx < self.files.len() {
            self.selected_file = idx;
            self.diff_scroll = 0;
            self.selected_hunk = 0;
            self.current_diff = None;
        }
    }

    pub fn file_list_up(&mut self) {
        if self.selected_file > 0 {
            self.select_file(self.selected_file - 1);
        }
    }

    pub fn file_list_down(&mut self) {
        if self.selected_file + 1 < self.files.len() {
            self.select_file(self.selected_file + 1);
        }
    }

    pub fn diff_scroll_up(&mut self) {
        self.diff_scroll = self.diff_scroll.saturating_sub(3);
    }

    /// Scroll the diff view down, capped so we never scroll past the content.
    /// `viewport_height` is the number of visible lines in the diff panel.
    pub fn diff_scroll_down(&mut self, viewport_height: usize) {
        let max_scroll = self.diff_content_lines().saturating_sub(viewport_height);
        self.diff_scroll = (self.diff_scroll + 3).min(max_scroll);
    }

    pub fn next_hunk(&mut self) {
        if let Some(ref diff) = self.current_diff {
            if self.selected_hunk + 1 < diff.hunks.len() {
                self.selected_hunk += 1;
                self.scroll_to_selected_hunk();
            }
        }
    }

    pub fn prev_hunk(&mut self) {
        if self.selected_hunk > 0 {
            self.selected_hunk -= 1;
            self.scroll_to_selected_hunk();
        }
    }

    /// Scroll the diff view so the selected hunk is at the top.
    pub fn scroll_to_selected_hunk(&mut self) {
        self.diff_scroll = self.hunk_scroll_offset(self.selected_hunk);
    }

    /// Compute the rendered line offset of `target_hunk` within the diff view.
    /// Used to scroll the view when jumping between hunks.
    fn hunk_scroll_offset(&self, target_hunk: usize) -> usize {
        let Some(ref diff) = self.current_diff else { return 0 };
        let mut offset = 0;
        for (i, hunk) in diff.hunks.iter().enumerate() {
            if i >= target_hunk {
                break;
            }
            let note_count = self.notes
                .iter()
                .filter(|n| n.file == diff.file.path && n.hunk_header == hunk.header)
                .count();
            offset += 1 + hunk.lines.len() + note_count + 1; // header + lines + notes + blank
        }
        offset
    }

    /// Total rendered line count for the current diff, used to cap scroll.
    fn diff_content_lines(&self) -> usize {
        let Some(ref diff) = self.current_diff else { return 0 };
        diff.hunks.iter().map(|h| {
            let note_count = self.notes
                .iter()
                .filter(|n| n.file == diff.file.path && n.hunk_header == h.header)
                .count();
            1 + h.lines.len() + note_count + 1 // header + lines + notes + blank
        }).sum()
    }

    /// Returns (file_path, hunk_header) for the currently selected hunk, or None.
    /// Used as a lookup key for notes on the current hunk.
    fn current_hunk_identity(&self) -> Option<(PathBuf, String)> {
        let diff = self.current_diff.as_ref()?;
        let hunk = diff.hunks.get(self.selected_hunk)?;
        Some((diff.file.path.clone(), hunk.header.clone()))
    }

    pub fn current_hunk_has_note(&self) -> bool {
        match self.current_hunk_identity() {
            Some((file, header)) => self.notes.iter().any(|n| n.file == file && n.hunk_header == header),
            None => false,
        }
    }

    pub fn delete_note_for_current_hunk(&mut self) {
        let Some((file, header)) = self.current_hunk_identity() else { return };
        self.notes.retain(|n| !(n.file == file && n.hunk_header == header));
    }

    /// Remove the existing note for the current hunk and re-open the comment
    /// input pre-populated with the old text so the user can revise it.
    pub fn edit_note_for_current_hunk(&mut self) {
        let Some((file, header)) = self.current_hunk_identity() else { return };
        let existing = self.notes
            .iter()
            .find(|n| n.file == file && n.hunk_header == header)
            .map(|n| n.note.clone());
        if let Some(text) = existing {
            self.notes.retain(|n| !(n.file == file && n.hunk_header == header));
            self.mode = Mode::Comment {
                hunk_idx: self.selected_hunk,
                input: text,
            };
        }
    }

    pub fn start_comment(&mut self) {
        if self.current_diff.as_ref().map(|d| !d.hunks.is_empty()).unwrap_or(false) {
            if self.current_hunk_has_note() {
                self.edit_note_for_current_hunk();
            } else {
                self.mode = Mode::Comment {
                    hunk_idx: self.selected_hunk,
                    input: String::new(),
                };
            }
        }
    }

    pub fn submit_comment(&mut self) {
        if let Mode::Comment { hunk_idx, ref input } = self.mode.clone() {
            let trimmed = input.trim().to_string();
            if !trimmed.is_empty() {
                if let Some(ref diff) = self.current_diff {
                    if let Some(hunk) = diff.hunks.get(hunk_idx) {
                        let hunk_content = hunk
                            .lines
                            .iter()
                            .map(|l| {
                                let prefix = match l.kind {
                                    LineKind::Added => "+",
                                    LineKind::Removed => "-",
                                    LineKind::Context => " ",
                                };
                                format!("{}{}", prefix, l.content)
                            })
                            .collect::<Vec<_>>()
                            .join("\n");

                        self.notes.push(FeedbackNote {
                            file: diff.file.path.clone(),
                            hunk_header: hunk.header.clone(),
                            hunk_content,
                            note: trimmed,
                        });
                    }
                }
            }
        }
        self.mode = Mode::Normal;
    }

    pub fn cancel_comment(&mut self) {
        self.mode = Mode::Normal;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::{DiffFile, DiffLine, FileStatus, Hunk, LineKind};

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn make_files(n: usize) -> Vec<ChangedFile> {
        (0..n)
            .map(|i| ChangedFile {
                path: PathBuf::from(format!("src/file_{}.rs", i)),
                status: FileStatus::Modified,
            })
            .collect()
    }

    fn make_hunk(header: &str) -> Hunk {
        Hunk {
            header: header.to_string(),
            old_start: 1,
            new_start: 1,
            lines: vec![
                DiffLine {
                    old_lineno: None,
                    new_lineno: Some(1),
                    kind: LineKind::Added,
                    content: "new line".to_string(),
                },
                DiffLine {
                    old_lineno: Some(1),
                    new_lineno: None,
                    kind: LineKind::Removed,
                    content: "old line".to_string(),
                },
                DiffLine {
                    old_lineno: Some(2),
                    new_lineno: Some(2),
                    kind: LineKind::Context,
                    content: "context".to_string(),
                },
            ],
        }
    }

    fn app_with_diff(hunk_count: usize) -> App {
        let files = make_files(1);
        let mut app = App::new(files.clone(), "main".to_string());
        app.current_diff = Some(DiffFile {
            file: files[0].clone(),
            hunks: (0..hunk_count)
                .map(|i| make_hunk(&format!("@@ -{},3 +{},4 @@", i * 10 + 1, i * 10 + 1)))
                .collect(),
        });
        app
    }

    // ── File list navigation ──────────────────────────────────────────────────

    #[test]
    fn test_file_list_down_navigates() {
        let mut app = App::new(make_files(3), "main".to_string());
        app.file_list_down();
        assert_eq!(app.selected_file, 1);
    }

    #[test]
    fn test_file_list_down_clamps_at_end() {
        let mut app = App::new(make_files(3), "main".to_string());
        app.selected_file = 2;
        app.file_list_down();
        assert_eq!(app.selected_file, 2);
    }

    #[test]
    fn test_file_list_up_navigates() {
        let mut app = App::new(make_files(3), "main".to_string());
        app.selected_file = 2;
        app.file_list_up();
        assert_eq!(app.selected_file, 1);
    }

    #[test]
    fn test_file_list_up_clamps_at_start() {
        let mut app = App::new(make_files(3), "main".to_string());
        app.file_list_up();
        assert_eq!(app.selected_file, 0);
    }

    #[test]
    fn test_select_file_resets_scroll_and_hunk() {
        let mut app = app_with_diff(3);
        app.diff_scroll = 10;
        app.selected_hunk = 2;
        app.select_file(0);
        assert_eq!(app.diff_scroll, 0);
        assert_eq!(app.selected_hunk, 0);
    }

    // ── Hunk navigation ───────────────────────────────────────────────────────

    #[test]
    fn test_next_hunk_advances() {
        let mut app = app_with_diff(3);
        app.next_hunk();
        assert_eq!(app.selected_hunk, 1);
    }

    #[test]
    fn test_next_hunk_clamps_at_last() {
        let mut app = app_with_diff(3);
        app.selected_hunk = 2;
        app.next_hunk();
        assert_eq!(app.selected_hunk, 2);
    }

    #[test]
    fn test_prev_hunk_retreats() {
        let mut app = app_with_diff(3);
        app.selected_hunk = 2;
        app.prev_hunk();
        assert_eq!(app.selected_hunk, 1);
    }

    #[test]
    fn test_prev_hunk_clamps_at_zero() {
        let mut app = app_with_diff(3);
        app.prev_hunk();
        assert_eq!(app.selected_hunk, 0);
    }

    #[test]
    fn test_next_hunk_no_op_without_diff() {
        let mut app = App::new(make_files(1), "main".to_string());
        app.next_hunk(); // should not panic
        assert_eq!(app.selected_hunk, 0);
    }

    // ── Comment flow ──────────────────────────────────────────────────────────

    #[test]
    fn test_start_comment_enters_comment_mode() {
        let mut app = app_with_diff(2);
        app.start_comment();
        assert!(matches!(app.mode, Mode::Comment { hunk_idx: 0, .. }));
    }

    #[test]
    fn test_start_comment_no_op_without_diff() {
        let mut app = App::new(make_files(1), "main".to_string());
        app.start_comment();
        assert_eq!(app.mode, Mode::Normal);
    }

    #[test]
    fn test_start_comment_no_op_with_empty_hunks() {
        let mut app = App::new(make_files(1), "main".to_string());
        app.current_diff = Some(DiffFile {
            file: make_files(1).remove(0),
            hunks: vec![],
        });
        app.start_comment();
        assert_eq!(app.mode, Mode::Normal);
    }

    #[test]
    fn test_start_comment_redirects_to_edit_when_note_exists() {
        let mut app = app_with_note_on_hunk(0);
        app.start_comment();
        // Should enter comment mode pre-populated with the existing note, not a blank input
        assert!(matches!(
            &app.mode,
            Mode::Comment { input, .. } if input == "original note"
        ));
    }

    #[test]
    fn test_start_comment_does_not_create_duplicate() {
        let mut app = app_with_note_on_hunk(0);
        app.start_comment(); // redirects to edit — old note removed
        if let Mode::Comment { ref mut input, .. } = app.mode {
            *input = "original note".to_string(); // re-submit same text
        }
        app.submit_comment();
        assert_eq!(app.notes.len(), 1, "should have exactly one note, not two");
    }

    #[test]
    fn test_cancel_comment_returns_to_normal() {
        let mut app = app_with_diff(1);
        app.start_comment();
        app.cancel_comment();
        assert_eq!(app.mode, Mode::Normal);
    }

    #[test]
    fn test_submit_comment_creates_note() {
        let mut app = app_with_diff(1);
        app.mode = Mode::Comment {
            hunk_idx: 0,
            input: "This looks wrong".to_string(),
        };
        app.submit_comment();

        assert_eq!(app.notes.len(), 1);
        assert_eq!(app.notes[0].note, "This looks wrong");
        assert_eq!(app.notes[0].file, PathBuf::from("src/file_0.rs"));
    }

    #[test]
    fn test_submit_comment_resets_to_normal() {
        let mut app = app_with_diff(1);
        app.mode = Mode::Comment {
            hunk_idx: 0,
            input: "some note".to_string(),
        };
        app.submit_comment();
        assert_eq!(app.mode, Mode::Normal);
    }

    #[test]
    fn test_submit_comment_ignores_blank_input() {
        let mut app = app_with_diff(1);
        app.mode = Mode::Comment {
            hunk_idx: 0,
            input: "   ".to_string(),
        };
        app.submit_comment();
        assert!(app.notes.is_empty());
    }

    #[test]
    fn test_submit_comment_captures_hunk_content() {
        let mut app = app_with_diff(1);
        app.mode = Mode::Comment {
            hunk_idx: 0,
            input: "check this".to_string(),
        };
        app.submit_comment();

        let content = &app.notes[0].hunk_content;
        assert!(content.contains("+new line"));
        assert!(content.contains("-old line"));
        assert!(content.contains(" context"));
    }

    #[test]
    fn test_submit_comment_on_second_hunk() {
        let mut app = app_with_diff(2);
        app.selected_hunk = 1;
        app.mode = Mode::Comment {
            hunk_idx: 1,
            input: "note on second hunk".to_string(),
        };
        app.submit_comment();

        assert_eq!(app.notes.len(), 1);
        assert!(app.notes[0].hunk_header.contains("11")); // second hunk starts at 11
    }

    // ── Hunk scroll offset ────────────────────────────────────────────────────

    // Each test hunk has: 1 header + 3 lines (added, removed, context) + 1 blank = 5 lines.

    #[test]
    fn test_hunk_scroll_offset_first_hunk_is_zero() {
        let app = app_with_diff(3);
        assert_eq!(app.hunk_scroll_offset(0), 0);
    }

    #[test]
    fn test_hunk_scroll_offset_second_hunk() {
        let app = app_with_diff(3);
        // hunk 0: 1 header + 3 lines + 0 notes + 1 blank = 5
        assert_eq!(app.hunk_scroll_offset(1), 5);
    }

    #[test]
    fn test_hunk_scroll_offset_third_hunk() {
        let app = app_with_diff(3);
        // hunk 0 + hunk 1 = 5 + 5 = 10
        assert_eq!(app.hunk_scroll_offset(2), 10);
    }

    #[test]
    fn test_hunk_scroll_offset_accounts_for_notes() {
        let mut app = app_with_diff(2);
        // Add a note on hunk 0
        app.mode = Mode::Comment { hunk_idx: 0, input: "a note".to_string() };
        app.submit_comment();
        // hunk 0: 1 header + 3 lines + 1 note + 1 blank = 6
        assert_eq!(app.hunk_scroll_offset(1), 6);
    }

    #[test]
    fn test_scroll_to_selected_hunk_sets_diff_scroll() {
        let mut app = app_with_diff(3);
        app.selected_hunk = 2;
        app.scroll_to_selected_hunk();
        assert_eq!(app.diff_scroll, 10);
    }

    #[test]
    fn test_next_hunk_scrolls_view() {
        let mut app = app_with_diff(3);
        app.next_hunk();
        assert_eq!(app.selected_hunk, 1);
        assert_eq!(app.diff_scroll, 5); // scrolled to hunk 1
    }

    #[test]
    fn test_prev_hunk_scrolls_view() {
        let mut app = app_with_diff(3);
        app.selected_hunk = 2;
        app.diff_scroll = 10;
        app.prev_hunk();
        assert_eq!(app.selected_hunk, 1);
        assert_eq!(app.diff_scroll, 5);
    }

    // ── Diff scroll capping ───────────────────────────────────────────────────

    #[test]
    fn test_diff_content_lines() {
        let app = app_with_diff(3);
        // 3 hunks * (1 header + 3 lines + 0 notes + 1 blank) = 3 * 5 = 15
        assert_eq!(app.diff_content_lines(), 15);
    }

    #[test]
    fn test_diff_content_lines_no_diff() {
        let app = App::new(make_files(1), "main".to_string());
        assert_eq!(app.diff_content_lines(), 0);
    }

    #[test]
    fn test_diff_scroll_down_caps_at_content_boundary() {
        let mut app = app_with_diff(1);
        // 1 hunk: 1 + 3 + 1 = 5 lines of content
        // viewport of 3 → max_scroll = 5 - 3 = 2
        app.diff_scroll_down(3);
        app.diff_scroll_down(3);
        app.diff_scroll_down(3); // should be capped
        assert!(app.diff_scroll <= 2);
    }

    #[test]
    fn test_diff_scroll_down_no_scroll_when_content_fits() {
        let mut app = app_with_diff(1);
        // content = 5 lines, viewport = 20 → max_scroll = 0
        app.diff_scroll_down(20);
        assert_eq!(app.diff_scroll, 0);
    }

    // ── Edit / delete notes ───────────────────────────────────────────────────

    fn app_with_note_on_hunk(hunk_idx: usize) -> App {
        let mut app = app_with_diff(3);
        app.selected_hunk = hunk_idx;
        app.mode = Mode::Comment {
            hunk_idx,
            input: "original note".to_string(),
        };
        app.submit_comment();
        app.selected_hunk = hunk_idx;
        app
    }

    #[test]
    fn test_current_hunk_has_note_true_when_note_exists() {
        let app = app_with_note_on_hunk(0);
        assert!(app.current_hunk_has_note());
    }

    #[test]
    fn test_current_hunk_has_note_false_when_no_note() {
        let app = app_with_diff(2);
        assert!(!app.current_hunk_has_note());
    }

    #[test]
    fn test_current_hunk_has_note_false_without_diff() {
        let app = App::new(make_files(1), "main".to_string());
        assert!(!app.current_hunk_has_note());
    }

    #[test]
    fn test_delete_note_removes_it() {
        let mut app = app_with_note_on_hunk(0);
        app.delete_note_for_current_hunk();
        assert!(app.notes.is_empty());
    }

    #[test]
    fn test_delete_note_only_removes_current_hunk() {
        let mut app = app_with_diff(3);
        // Add notes on hunks 0 and 1
        for hunk_idx in [0, 1] {
            app.selected_hunk = hunk_idx;
            app.mode = Mode::Comment { hunk_idx, input: format!("note {}", hunk_idx) };
            app.submit_comment();
        }
        // Delete note on hunk 0 only
        app.selected_hunk = 0;
        app.delete_note_for_current_hunk();

        assert_eq!(app.notes.len(), 1);
        assert!(app.notes[0].note.contains("note 1"));
    }

    #[test]
    fn test_delete_note_no_op_when_no_note() {
        let mut app = app_with_diff(2);
        app.delete_note_for_current_hunk(); // should not panic
        assert!(app.notes.is_empty());
    }

    #[test]
    fn test_delete_note_no_op_without_diff() {
        let mut app = App::new(make_files(1), "main".to_string());
        app.delete_note_for_current_hunk(); // should not panic
    }

    #[test]
    fn test_edit_note_enters_comment_mode_with_existing_text() {
        let mut app = app_with_note_on_hunk(0);
        app.edit_note_for_current_hunk();
        assert!(matches!(
            &app.mode,
            Mode::Comment { input, .. } if input == "original note"
        ));
    }

    #[test]
    fn test_edit_note_removes_old_note_before_editing() {
        let mut app = app_with_note_on_hunk(0);
        app.edit_note_for_current_hunk();
        // Note should be gone — it will be re-added on submit
        assert!(app.notes.is_empty());
    }

    #[test]
    fn test_edit_note_no_op_when_no_note() {
        let mut app = app_with_diff(2);
        app.edit_note_for_current_hunk();
        assert_eq!(app.mode, Mode::Normal);
    }

    #[test]
    fn test_edit_then_submit_updates_note() {
        let mut app = app_with_note_on_hunk(0);
        app.edit_note_for_current_hunk();
        // Simulate the user clearing the input and typing a new note
        if let Mode::Comment { ref mut input, .. } = app.mode {
            *input = "revised note".to_string();
        }
        app.submit_comment();

        assert_eq!(app.notes.len(), 1);
        assert_eq!(app.notes[0].note, "revised note");
    }
}
