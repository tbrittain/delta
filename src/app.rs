use std::collections::HashSet;
use std::path::PathBuf;

use crate::diff::{ChangedFile, DiffFile, DiffLine, LineKind};

/// Context runs of this many lines or more are folded by default.
pub(crate) const FOLD_THRESHOLD: usize = 6;

#[derive(Debug, Clone, Default, PartialEq)]
pub enum Panel {
    #[default]
    FileList,
    DiffView,
    NotesView,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub enum Mode {
    #[default]
    Normal,
    Comment {
        hunk_idx: usize,
        input: String,
        /// Byte offset of the insertion cursor, always on a char boundary.
        cursor: usize,
        /// The note being replaced, if this is an edit rather than a new comment.
        /// Restored on Esc so cancelling an edit never loses the original.
        original: Option<FeedbackNote>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct FeedbackNote {
    pub file: PathBuf,
    pub hunk_header: String,
    pub hunk_content: String,
    pub note: String,
}

pub struct App {
    pub from: String,
    pub to: String,
    pub files: Vec<ChangedFile>,
    pub selected_file: usize,
    pub focused_panel: Panel,
    pub current_diff: Option<DiffFile>,
    pub current_highlights: Option<crate::highlight::DiffHighlights>,
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
}

impl App {
    pub fn new(files: Vec<ChangedFile>, from: String, to: String) -> Self {
        Self {
            from,
            to,
            files,
            selected_file: 0,
            focused_panel: Panel::FileList,
            current_diff: None,
            current_highlights: None,
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
        }
    }

    pub fn select_file(&mut self, idx: usize) {
        if idx < self.files.len() {
            self.selected_file = idx;
            self.diff_scroll = 0;
            self.selected_hunk = 0;
            self.current_diff = None;
            self.current_highlights = None;
            self.expanded_hunks.clear();
        }
    }

    // ── Notes panel ──────────────────────────────────────────────────────────

    pub fn notes_up(&mut self) {
        self.selected_note = self.selected_note.saturating_sub(1);
    }

    pub fn notes_down(&mut self) {
        if self.selected_note + 1 < self.notes.len() {
            self.selected_note += 1;
        }
    }

    pub fn toggle_note_expand(&mut self) {
        if self.expanded_notes.contains(&self.selected_note) {
            self.expanded_notes.remove(&self.selected_note);
        } else {
            self.expanded_notes.insert(self.selected_note);
        }
    }

    /// Returns the index into `self.files` for the note currently selected in the Notes panel.
    pub fn selected_note_file_idx(&self) -> Option<usize> {
        let note = self.notes.get(self.selected_note)?;
        self.files.iter().position(|f| f.path == note.file)
    }

    /// Deletes the note currently selected in the Notes panel.
    pub fn delete_selected_note(&mut self) {
        if self.selected_note >= self.notes.len() {
            return;
        }
        self.notes.remove(self.selected_note);
        self.expanded_notes.clear();
        if self.selected_note > 0 && self.selected_note >= self.notes.len() {
            self.selected_note -= 1;
        }
    }

    pub fn toggle_hunk_fold(&mut self) {
        if self.expanded_hunks.contains(&self.selected_hunk) {
            self.expanded_hunks.remove(&self.selected_hunk);
        } else {
            self.expanded_hunks.insert(self.selected_hunk);
        }
    }

    /// True if the selected hunk has any context run long enough to fold.
    pub fn selected_hunk_is_foldable(&self) -> bool {
        let Some(ref diff) = self.current_diff else { return false };
        let Some(hunk) = diff.hunks.get(self.selected_hunk) else { return false };
        hunk_has_foldable_context(&hunk.lines)
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
    /// Accounts for folded context runs so hunk-jump scrolls to the right position.
    fn hunk_scroll_offset(&self, target_hunk: usize) -> usize {
        let Some(ref diff) = self.current_diff else { return 0 };
        let mut offset = 0;
        for (i, hunk) in diff.hunks.iter().enumerate() {
            if i >= target_hunk {
                break;
            }
            let is_expanded = self.expanded_hunks.contains(&i);
            let content_lines = if is_expanded {
                hunk.lines.len()
            } else {
                context_run_visual_lines(&hunk.lines)
            };
            let note_count = self.notes
                .iter()
                .filter(|n| n.file == diff.file.path && n.hunk_header == hunk.header)
                .count();
            offset += 1 + content_lines + note_count + 1; // header + lines + notes + blank
        }
        offset
    }

    /// Total rendered line count for the current diff, used to cap scroll.
    /// Accounts for folded context runs.
    fn diff_content_lines(&self) -> usize {
        let Some(ref diff) = self.current_diff else { return 0 };
        diff.hunks.iter().enumerate().map(|(i, h)| {
            let is_expanded = self.expanded_hunks.contains(&i);
            let content_lines = if is_expanded {
                h.lines.len()
            } else {
                context_run_visual_lines(&h.lines)
            };
            let note_count = self.notes
                .iter()
                .filter(|n| n.file == diff.file.path && n.hunk_header == h.header)
                .count();
            1 + content_lines + note_count + 1 // header + lines + notes + blank
        }).sum()
    }

    /// Adjust `comment_scroll` so the cursor stays visible within the popup viewport.
    /// `content_width` is the number of characters per visual line (popup width minus borders);
    /// it is used to compute the correct visual row when long lines wrap.
    pub fn scroll_comment_to_cursor(&mut self, viewport_height: usize, content_width: usize) {
        let cursor_visual_row = match &self.mode {
            Mode::Comment { input, cursor, .. } => visual_row_for_cursor(input, *cursor, content_width),
            _ => return,
        };
        if cursor_visual_row < self.comment_scroll {
            self.comment_scroll = cursor_visual_row;
        } else if viewport_height > 0 && cursor_visual_row + 1 > self.comment_scroll + viewport_height {
            self.comment_scroll = cursor_visual_row + 1 - viewport_height;
        }
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
        self.expanded_notes.clear();
        if self.selected_note >= self.notes.len() && !self.notes.is_empty() {
            self.selected_note = self.notes.len() - 1;
        }
    }

    /// Remove the existing note for the current hunk and re-open the comment
    /// input pre-populated with the old text so the user can revise it.
    pub fn edit_note_for_current_hunk(&mut self) {
        let Some((file, header)) = self.current_hunk_identity() else { return };
        if let Some(original) = self.notes.iter()
            .find(|n| n.file == file && n.hunk_header == header)
            .cloned()
        {
            self.notes.retain(|n| !(n.file == file && n.hunk_header == header));
            self.expanded_notes.clear();
            if self.selected_note >= self.notes.len() && !self.notes.is_empty() {
                self.selected_note = self.notes.len() - 1;
            }
            let cursor = original.note.len();
            let input = original.note.clone();
            self.comment_scroll = 0;
            self.comment_anchor = None;
            self.mode = Mode::Comment {
                hunk_idx: self.selected_hunk,
                input,
                cursor,
                original: Some(original),
            };
        }
    }

    pub fn start_comment(&mut self) {
        if self.current_diff.as_ref().map(|d| !d.hunks.is_empty()).unwrap_or(false) {
            if self.current_hunk_has_note() {
                self.edit_note_for_current_hunk();
            } else {
                self.comment_scroll = 0;
                self.comment_anchor = None;
                self.mode = Mode::Comment {
                    hunk_idx: self.selected_hunk,
                    input: String::new(),
                    cursor: 0,
                    original: None,
                };
            }
        }
    }

    pub fn submit_comment(&mut self) {
        if let Mode::Comment { hunk_idx, ref input, .. } = self.mode.clone() {
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
        self.comment_scroll = 0;
        self.comment_anchor = None;
        self.mode = Mode::Normal;
    }

    pub fn cancel_comment(&mut self) {
        // If editing an existing note, restore the original so Esc never loses it.
        if let Mode::Comment { ref original, .. } = self.mode.clone() {
            if let Some(note) = original.clone() {
                self.notes.push(note);
            }
        }
        self.comment_scroll = 0;
        self.comment_anchor = None;
        self.mode = Mode::Normal;
    }
}

/// Count the visual lines a slice of diff lines occupies when context runs are folded.
/// Runs of context lines >= FOLD_THRESHOLD collapse to a single placeholder line.
fn context_run_visual_lines(lines: &[DiffLine]) -> usize {
    let mut count = 0;
    let mut ctx_run = 0;
    for line in lines {
        if line.kind == LineKind::Context {
            ctx_run += 1;
        } else {
            count += if ctx_run >= FOLD_THRESHOLD { 1 } else { ctx_run };
            ctx_run = 0;
            count += 1;
        }
    }
    count += if ctx_run >= FOLD_THRESHOLD { 1 } else { ctx_run };
    count
}

/// True if the given lines contain at least one context run long enough to fold.
fn hunk_has_foldable_context(lines: &[DiffLine]) -> bool {
    let mut ctx_run = 0;
    for line in lines {
        if line.kind == LineKind::Context {
            ctx_run += 1;
            if ctx_run >= FOLD_THRESHOLD {
                return true;
            }
        } else {
            ctx_run = 0;
        }
    }
    false
}

/// Returns the visual row index of `cursor` within `input`, accounting for line wrapping
/// at `content_width` characters. Used by `scroll_comment_to_cursor`.
pub(crate) fn visual_row_for_cursor(input: &str, cursor: usize, content_width: usize) -> usize {
    let cw = content_width.max(1);
    let mut visual_row = 0usize;
    let mut byte_pos = 0usize;
    for logical_line in input.split('\n') {
        let char_count = logical_line.chars().count();
        let line_byte_end = byte_pos + logical_line.len();
        if cursor >= byte_pos && cursor <= line_byte_end {
            let char_offset = logical_line[..cursor - byte_pos].chars().count();
            // Cursor at end of line (char_offset == char_count) belongs to the last visual row,
            // not the (out-of-range) row after it.
            let clamped = if char_count == 0 { 0 } else { char_offset.min(char_count - 1) };
            return visual_row + clamped / cw;
        }
        visual_row += if char_count == 0 { 1 } else { (char_count + cw - 1) / cw };
        byte_pos = line_byte_end + 1;
    }
    visual_row.saturating_sub(1)
}

/// Returns `Some((start, end))` where `start < end` if there is a non-empty selection,
/// `None` otherwise.
pub(crate) fn selected_range(cursor: usize, anchor: Option<usize>) -> Option<(usize, usize)> {
    let a = anchor?;
    let start = cursor.min(a);
    let end = cursor.max(a);
    if start < end { Some((start, end)) } else { None }
}

/// Delete the selected byte range from `input` and return `(new_input, new_cursor)`.
/// Returns `None` if there is no non-empty selection.
pub(crate) fn delete_selection(
    input: &str,
    cursor: usize,
    anchor: Option<usize>,
) -> Option<(String, usize)> {
    let (start, end) = selected_range(cursor, anchor)?;
    let mut new_input = input.to_string();
    new_input.drain(start..end);
    Some((new_input, start))
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
        let mut app = App::new(files.clone(), "main".to_string(), "HEAD".to_string());
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
        let mut app = App::new(make_files(3), "main".to_string(), "HEAD".to_string());
        app.file_list_down();
        assert_eq!(app.selected_file, 1);
    }

    #[test]
    fn test_file_list_down_clamps_at_end() {
        let mut app = App::new(make_files(3), "main".to_string(), "HEAD".to_string());
        app.selected_file = 2;
        app.file_list_down();
        assert_eq!(app.selected_file, 2);
    }

    #[test]
    fn test_file_list_up_navigates() {
        let mut app = App::new(make_files(3), "main".to_string(), "HEAD".to_string());
        app.selected_file = 2;
        app.file_list_up();
        assert_eq!(app.selected_file, 1);
    }

    #[test]
    fn test_file_list_up_clamps_at_start() {
        let mut app = App::new(make_files(3), "main".to_string(), "HEAD".to_string());
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
        let mut app = App::new(make_files(1), "main".to_string(), "HEAD".to_string());
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
        let mut app = App::new(make_files(1), "main".to_string(), "HEAD".to_string());
        app.start_comment();
        assert_eq!(app.mode, Mode::Normal);
    }

    #[test]
    fn test_start_comment_no_op_with_empty_hunks() {
        let mut app = App::new(make_files(1), "main".to_string(), "HEAD".to_string());
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
    fn test_cancel_edit_restores_original_note() {
        let mut app = app_with_note_on_hunk(0);
        app.edit_note_for_current_hunk();
        // Escape without submitting — original note must be restored
        app.cancel_comment();
        assert_eq!(app.notes.len(), 1);
        assert_eq!(app.notes[0].note, "original note");
    }

    #[test]
    fn test_cancel_new_comment_does_not_create_note() {
        let mut app = app_with_diff(1);
        app.start_comment();
        app.cancel_comment();
        assert!(app.notes.is_empty());
    }

    #[test]
    fn test_submit_comment_creates_note() {
        let mut app = app_with_diff(1);
        app.mode = Mode::Comment {
            hunk_idx: 0,
            input: "This looks wrong".to_string(),
            cursor: 0,
            original: None,
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
            cursor: 0,
            original: None,
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
            cursor: 0,
            original: None,
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
            cursor: 0,
            original: None,
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
            cursor: 0,
            original: None,
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
        app.mode = Mode::Comment { hunk_idx: 0, input: "a note".to_string(), cursor: 0, original: None };
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
        let app = App::new(make_files(1), "main".to_string(), "HEAD".to_string());
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

    // ── visual_row_for_cursor ─────────────────────────────────────────────────

    #[test]
    fn test_visual_row_no_wrap() {
        // Width > line length: no wrapping, rows == logical line indices.
        assert_eq!(visual_row_for_cursor("hello\nworld", 0,   100), 0); // start of "hello"
        assert_eq!(visual_row_for_cursor("hello\nworld", 5,   100), 0); // end of "hello"
        assert_eq!(visual_row_for_cursor("hello\nworld", 6,   100), 1); // start of "world"
        assert_eq!(visual_row_for_cursor("hello\nworld", 11,  100), 1); // end of "world"
    }

    #[test]
    fn test_visual_row_with_wrap() {
        // "hellothere" with width=5 wraps to ["hello"(row 0), "there"(row 1)]
        assert_eq!(visual_row_for_cursor("hellothere", 0, 5), 0);
        assert_eq!(visual_row_for_cursor("hellothere", 4, 5), 0); // in "hello"
        assert_eq!(visual_row_for_cursor("hellothere", 5, 5), 1); // start of "there"
        assert_eq!(visual_row_for_cursor("hellothere", 10, 5), 1); // end of "there"
    }

    #[test]
    fn test_visual_row_multiline_with_wrap() {
        // "hi\nhellothere" with width=5:
        //   row 0: "hi"        (bytes 0..2)
        //   row 1: "hello"     (bytes 3..7 in full input = chars 0..4 of "hellothere")
        //   row 2: "there"     (bytes 8..12 in full input = chars 5..9 of "hellothere")
        let input = "hi\nhellothere";
        assert_eq!(visual_row_for_cursor(input, 0,  5), 0); // 'h' of "hi"
        assert_eq!(visual_row_for_cursor(input, 2,  5), 0); // end of "hi"
        assert_eq!(visual_row_for_cursor(input, 3,  5), 1); // 'h' = first char of "hellothere"
        assert_eq!(visual_row_for_cursor(input, 7,  5), 1); // 'o' = char 4 of "hellothere"
        assert_eq!(visual_row_for_cursor(input, 8,  5), 2); // 't' = char 5 of "hellothere", first of "there"
        assert_eq!(visual_row_for_cursor(input, 13, 5), 2); // end of "hellothere"
    }

    // ── Comment popup scrolling ───────────────────────────────────────────────

    #[test]
    fn test_scroll_comment_to_cursor_scrolls_down_when_cursor_below_viewport() {
        let mut app = app_with_diff(1);
        let input = "a\nb\nc\nd\ne".to_string();
        let cursor = input.len(); // cursor on visual row 4 (width=100: no wrap)
        app.mode = Mode::Comment { hunk_idx: 0, input, cursor, original: None };
        app.scroll_comment_to_cursor(3, 100);
        assert_eq!(app.comment_scroll, 2); // 4+1-3 = 2
    }

    #[test]
    fn test_scroll_comment_to_cursor_no_scroll_when_cursor_visible() {
        let mut app = app_with_diff(1);
        let input = "line1\nline2".to_string();
        app.mode = Mode::Comment { hunk_idx: 0, input, cursor: 5, original: None };
        app.scroll_comment_to_cursor(5, 100);
        assert_eq!(app.comment_scroll, 0);
    }

    #[test]
    fn test_scroll_comment_to_cursor_scrolls_up_when_cursor_above_viewport() {
        let mut app = app_with_diff(1);
        let input = "a\nb\nc\nd\ne".to_string();
        app.mode = Mode::Comment { hunk_idx: 0, input, cursor: 0, original: None };
        app.comment_scroll = 3;
        app.scroll_comment_to_cursor(3, 100);
        assert_eq!(app.comment_scroll, 0);
    }

    #[test]
    fn test_scroll_comment_to_cursor_no_op_outside_comment_mode() {
        let mut app = app_with_diff(1);
        app.comment_scroll = 5;
        app.scroll_comment_to_cursor(10, 100);
        assert_eq!(app.comment_scroll, 5);
    }

    #[test]
    fn test_scroll_comment_to_cursor_accounts_for_wrap() {
        // Single long line that wraps: "aaaaaaaaaa" (10 a's) with content_width=5
        // Visual rows: row 0 = "aaaaa", row 1 = "aaaaa"
        // cursor at byte 7 (in second visual row) with viewport=1 should scroll to row 1
        let mut app = app_with_diff(1);
        app.mode = Mode::Comment {
            hunk_idx: 0,
            input: "aaaaaaaaaa".to_string(),
            cursor: 7,
            original: None,
        };
        app.scroll_comment_to_cursor(1, 5);
        assert_eq!(app.comment_scroll, 1);
    }

    // ── Edit / delete notes ───────────────────────────────────────────────────

    fn app_with_note_on_hunk(hunk_idx: usize) -> App {
        let mut app = app_with_diff(3);
        app.selected_hunk = hunk_idx;
        app.mode = Mode::Comment {
            hunk_idx,
            input: "original note".to_string(),
            cursor: 0,
            original: None,
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
        let app = App::new(make_files(1), "main".to_string(), "HEAD".to_string());
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
            app.mode = Mode::Comment { hunk_idx, input: format!("note {}", hunk_idx), cursor: 0, original: None };
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
        let mut app = App::new(make_files(1), "main".to_string(), "HEAD".to_string());
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

    // ── Multi-line comment input ──────────────────────────────────────────────

    #[test]
    fn test_submit_comment_preserves_internal_newlines() {
        let mut app = app_with_diff(1);
        app.mode = Mode::Comment {
            hunk_idx: 0,
            input: "line one\nline two\nline three".to_string(),
            cursor: 0,
            original: None,
        };
        app.submit_comment();
        assert_eq!(app.notes[0].note, "line one\nline two\nline three");
    }

    #[test]
    fn test_submit_comment_trims_surrounding_newlines() {
        let mut app = app_with_diff(1);
        app.mode = Mode::Comment {
            hunk_idx: 0,
            input: "\n\nline one\nline two\n\n".to_string(),
            cursor: 0,
            original: None,
        };
        app.submit_comment();
        assert_eq!(app.notes[0].note, "line one\nline two");
    }

    #[test]
    fn test_submit_comment_blank_multiline_is_ignored() {
        let mut app = app_with_diff(1);
        app.mode = Mode::Comment {
            hunk_idx: 0,
            input: "\n\n\n".to_string(),
            cursor: 0,
            original: None,
        };
        app.submit_comment();
        assert!(app.notes.is_empty(), "all-whitespace multi-line input should not create a note");
    }

    // ── Context folding ───────────────────────────────────────────────────────

    fn make_lines(kinds: &[LineKind]) -> Vec<crate::diff::DiffLine> {
        kinds.iter().map(|k| crate::diff::DiffLine {
            old_lineno: Some(1),
            new_lineno: Some(1),
            kind: k.clone(),
            content: "x".to_string(),
        }).collect()
    }

    #[test]
    fn test_context_run_visual_lines_short_run_shown_as_is() {
        // 3 context lines < FOLD_THRESHOLD — should count as 3, not 1
        let lines = make_lines(&[LineKind::Context; 3]);
        assert_eq!(context_run_visual_lines(&lines), 3);
    }

    #[test]
    fn test_context_run_visual_lines_long_run_folds_to_one() {
        let lines = make_lines(&[LineKind::Context; FOLD_THRESHOLD]);
        assert_eq!(context_run_visual_lines(&lines), 1);
    }

    #[test]
    fn test_context_run_visual_lines_mixed() {
        // added + 2 context + added + FOLD_THRESHOLD context + added
        let mut kinds = vec![LineKind::Added];
        kinds.extend(vec![LineKind::Context; 2]);
        kinds.push(LineKind::Added);
        kinds.extend(vec![LineKind::Context; FOLD_THRESHOLD]);
        kinds.push(LineKind::Added);
        let lines = make_lines(&kinds);
        // visual: 1 + 2 + 1 + 1(fold) + 1 = 6
        assert_eq!(context_run_visual_lines(&lines), 6);
    }

    #[test]
    fn test_hunk_has_foldable_context_false_when_below_threshold() {
        let lines = make_lines(&[LineKind::Context; FOLD_THRESHOLD - 1]);
        assert!(!hunk_has_foldable_context(&lines));
    }

    #[test]
    fn test_hunk_has_foldable_context_true_at_threshold() {
        let lines = make_lines(&[LineKind::Context; FOLD_THRESHOLD]);
        assert!(hunk_has_foldable_context(&lines));
    }

    #[test]
    fn test_toggle_hunk_fold_expands_then_collapses() {
        let mut app = app_with_diff(2);
        assert!(!app.expanded_hunks.contains(&0));
        app.toggle_hunk_fold();
        assert!(app.expanded_hunks.contains(&0));
        app.toggle_hunk_fold();
        assert!(!app.expanded_hunks.contains(&0));
    }

    #[test]
    fn test_select_file_clears_expanded_hunks() {
        let mut app = App::new(make_files(2), "main".to_string(), "HEAD".to_string());
        app.expanded_hunks.insert(0);
        app.select_file(1);
        assert!(app.expanded_hunks.is_empty());
    }

    // ── Notes panel ───────────────────────────────────────────────────────────

    fn app_with_two_file_notes() -> App {
        let mut app = app_with_diff(2);
        // Note on hunk 0
        app.mode = Mode::Comment { hunk_idx: 0, input: "first note".to_string(), cursor: 0, original: None };
        app.submit_comment();
        // Note on hunk 1
        app.selected_hunk = 1;
        app.mode = Mode::Comment { hunk_idx: 1, input: "second note".to_string(), cursor: 0, original: None };
        app.submit_comment();
        app.selected_note = 0;
        app
    }

    #[test]
    fn test_notes_down_navigates() {
        let mut app = app_with_two_file_notes();
        app.notes_down();
        assert_eq!(app.selected_note, 1);
    }

    #[test]
    fn test_notes_down_clamps_at_end() {
        let mut app = app_with_two_file_notes();
        app.selected_note = 1;
        app.notes_down();
        assert_eq!(app.selected_note, 1);
    }

    #[test]
    fn test_notes_up_navigates() {
        let mut app = app_with_two_file_notes();
        app.selected_note = 1;
        app.notes_up();
        assert_eq!(app.selected_note, 0);
    }

    #[test]
    fn test_notes_up_clamps_at_zero() {
        let mut app = app_with_two_file_notes();
        app.notes_up();
        assert_eq!(app.selected_note, 0);
    }

    #[test]
    fn test_toggle_note_expand() {
        let mut app = app_with_two_file_notes();
        assert!(!app.expanded_notes.contains(&0));
        app.toggle_note_expand();
        assert!(app.expanded_notes.contains(&0));
        app.toggle_note_expand();
        assert!(!app.expanded_notes.contains(&0));
    }

    #[test]
    fn test_selected_note_file_idx_found() {
        let app = app_with_two_file_notes();
        // Both notes are on src/file_0.rs (same file in app_with_diff)
        assert_eq!(app.selected_note_file_idx(), Some(0));
    }

    #[test]
    fn test_selected_note_file_idx_none_when_no_notes() {
        let app = app_with_diff(1);
        assert_eq!(app.selected_note_file_idx(), None);
    }

    #[test]
    fn test_delete_selected_note_removes_it() {
        let mut app = app_with_two_file_notes();
        app.delete_selected_note();
        assert_eq!(app.notes.len(), 1);
        assert_eq!(app.notes[0].note, "second note");
    }

    #[test]
    fn test_delete_selected_note_adjusts_index_when_at_end() {
        let mut app = app_with_two_file_notes();
        app.selected_note = 1;
        app.delete_selected_note();
        assert_eq!(app.notes.len(), 1);
        assert_eq!(app.selected_note, 0);
    }

    #[test]
    fn test_delete_selected_note_clears_expanded_notes() {
        let mut app = app_with_two_file_notes();
        app.expanded_notes.insert(0);
        app.delete_selected_note();
        assert!(app.expanded_notes.is_empty());
    }

    #[test]
    fn test_delete_selected_note_noop_when_empty() {
        let mut app = app_with_diff(1);
        app.delete_selected_note(); // should not panic
        assert!(app.notes.is_empty());
    }

    // ── selected_range ────────────────────────────────────────────────────────

    #[test]
    fn test_selected_range_forward() {
        assert_eq!(selected_range(8, Some(3)), Some((3, 8)));
    }

    #[test]
    fn test_selected_range_backward() {
        assert_eq!(selected_range(3, Some(8)), Some((3, 8)));
    }

    #[test]
    fn test_selected_range_no_anchor() {
        assert_eq!(selected_range(5, None), None);
    }

    #[test]
    fn test_selected_range_empty_when_cursor_equals_anchor() {
        assert_eq!(selected_range(5, Some(5)), None);
    }

    // ── delete_selection ─────────────────────────────────────────────────────

    #[test]
    fn test_delete_selection_forward_range() {
        // "hello world", delete bytes 3..8 = "lo wo"
        let (s, c) = delete_selection("hello world", 8, Some(3)).unwrap();
        assert_eq!(s, "helrld");
        assert_eq!(c, 3);
    }

    #[test]
    fn test_delete_selection_backward_range() {
        let (s, c) = delete_selection("hello world", 3, Some(8)).unwrap();
        assert_eq!(s, "helrld");
        assert_eq!(c, 3);
    }

    #[test]
    fn test_delete_selection_no_anchor_returns_none() {
        assert!(delete_selection("hello", 3, None).is_none());
    }

    #[test]
    fn test_delete_selection_empty_range_returns_none() {
        assert!(delete_selection("hello", 3, Some(3)).is_none());
    }

    #[test]
    fn test_delete_selection_full_text() {
        let (s, c) = delete_selection("hello", 5, Some(0)).unwrap();
        assert_eq!(s, "");
        assert_eq!(c, 0);
    }

    #[test]
    fn test_delete_selection_across_newline() {
        let input = "line1\nline2\nline3";
        // delete the '\n' at byte 5
        let (s, c) = delete_selection(input, 5, Some(6)).unwrap();
        assert_eq!(s, "line1line2\nline3");
        assert_eq!(c, 5);
    }

    #[test]
    fn test_delete_selection_multiline_span() {
        let input = "hello\nworld";
        // delete bytes 3..8 = "lo\nwo"
        let (s, c) = delete_selection(input, 8, Some(3)).unwrap();
        assert_eq!(s, "helrld");
        assert_eq!(c, 3);
    }
}
