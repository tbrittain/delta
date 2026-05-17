use std::path::PathBuf;

use crate::diff::LineKind;
use crate::segment::RichHunk;
use super::{App, FeedbackNote, LineRange, Mode};
use super::layout::note_visual_rows;

/// Given a hunk and two line indices (anchor and active), compute the file-line
/// range using `new_lineno` with fallback to `old_lineno` for removed-only lines.
pub(crate) fn line_indices_to_range(hunk: &RichHunk, idx_a: usize, idx_b: usize) -> Option<LineRange> {
    let lo = idx_a.min(idx_b);
    let hi = idx_a.max(idx_b);
    let lineno_for = |idx: usize| -> Option<u32> {
        let rl = hunk.lines.get(idx)?;
        rl.diff_line.new_lineno.or(rl.diff_line.old_lineno)
    };
    let start = lineno_for(lo)?;
    let end   = lineno_for(hi)?;
    Some(LineRange::new(start, end))
}

impl App {
    // ── Notes panel ──────────────────────────────────────────────────────────

    pub fn notes_up(&mut self) {
        self.selected_note = self.selected_note.saturating_sub(1);
    }

    pub fn notes_down(&mut self) {
        if self.selected_note + 1 < self.notes.len() {
            self.selected_note += 1;
        }
    }

    /// Adjust `notes_scroll` so the selected note is within the viewport.
    /// `viewport_height` is the inner content height of the notes panel (panel rows − 2 borders).
    pub fn scroll_notes_to_selected(&mut self, viewport_height: usize) {
        if self.notes.is_empty() || viewport_height == 0 { return; }
        let note_start: usize = self.notes[..self.selected_note].iter().enumerate()
            .map(|(i, n)| note_visual_rows(n, self.expanded_notes.contains(&i)))
            .sum();
        let note_h = note_visual_rows(
            &self.notes[self.selected_note],
            self.expanded_notes.contains(&self.selected_note),
        );
        if note_start < self.notes_scroll {
            self.notes_scroll = note_start;
        } else if note_start + note_h > self.notes_scroll + viewport_height {
            self.notes_scroll = (note_start + note_h).saturating_sub(viewport_height);
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
        if self.selected_note >= self.notes.len() { return; }
        self.notes.remove(self.selected_note);
        self.expanded_notes.clear();
        self.notes_scroll = 0;
        if self.selected_note > 0 && self.selected_note >= self.notes.len() {
            self.selected_note -= 1;
        }
    }

    // ── Note CRUD on current hunk ────────────────────────────────────────────

    /// Returns (file_path, hunk_header) for the currently selected hunk, or None.
    /// Used as a lookup key for notes on the current hunk.
    fn current_hunk_identity(&self) -> Option<(PathBuf, String)> {
        let diff = self.current_rich_diff.as_ref()?;
        let hunk = diff.hunks.get(self.selected_hunk)?;
        Some((diff.file.path.clone(), hunk.header.clone()))
    }

    pub fn current_hunk_has_note(&self) -> bool {
        match self.current_hunk_identity() {
            Some((file, header)) => self.notes.iter().any(|n| n.file == file && n.hunk_header == header && n.line_range.is_none()),
            None => false,
        }
    }

    pub fn delete_note_for_current_hunk(&mut self) {
        let Some((file, header)) = self.current_hunk_identity() else { return };
        self.notes.retain(|n| !(n.file == file && n.hunk_header == header && n.line_range.is_none()));
        self.expanded_notes.clear();
        self.notes_scroll = 0;
        if self.selected_note >= self.notes.len() && !self.notes.is_empty() {
            self.selected_note = self.notes.len() - 1;
        }
    }

    /// Remove the existing whole-hunk note and re-open the comment input
    /// pre-populated with the old text so the user can revise it.
    pub fn edit_note_for_current_hunk(&mut self) {
        let Some((file, header)) = self.current_hunk_identity() else { return };
        if let Some(original) = self.notes.iter()
            .find(|n| n.file == file && n.hunk_header == header && n.line_range.is_none())
            .cloned()
        {
            self.notes.retain(|n| !(n.file == file && n.hunk_header == header && n.line_range.is_none()));
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
                line_range: None,
            };
        }
    }

    // ── Comment flow ─────────────────────────────────────────────────────────

    pub fn start_comment(&mut self) {
        if self.current_rich_diff.as_ref().map(|d| !d.hunks.is_empty()).unwrap_or(false) {
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
                    line_range: None,
                };
            }
        }
    }

    pub fn submit_comment(&mut self) {
        if let Mode::Comment { hunk_idx, ref input, ref line_range, .. } = self.mode.clone() {
            let trimmed = input.trim().to_string();
            if !trimmed.is_empty()
                && let Some(ref diff) = self.current_rich_diff
                && let Some(hunk) = diff.hunks.get(hunk_idx)
            {
                let hunk_content = hunk
                    .lines
                    .iter()
                    .filter(|rl| match line_range {
                        Some(r) => {
                            let lineno = rl.diff_line.new_lineno.or(rl.diff_line.old_lineno);
                            lineno.map(|n| r.contains(n)).unwrap_or(false)
                        }
                        None => true,
                    })
                    .map(|rl| {
                        let prefix = match rl.diff_line.kind {
                            LineKind::Added   => "+",
                            LineKind::Removed => "-",
                            LineKind::Context => " ",
                        };
                        format!("{}{}", prefix, rl.diff_line.content)
                    })
                    .collect::<Vec<_>>()
                    .join("\n");

                self.notes.push(FeedbackNote {
                    file: diff.file.path.clone(),
                    hunk_header: hunk.header.clone(),
                    hunk_content,
                    note: trimmed,
                    line_range: line_range.clone(),
                });
            }
        }
        self.comment_scroll = 0;
        self.comment_anchor = None;
        self.mode = Mode::Normal;
    }

    pub fn cancel_comment(&mut self) {
        // If editing an existing note, restore the original so Esc never loses it.
        if let Mode::Comment { ref original, .. } = self.mode.clone()
            && let Some(note) = original.clone()
        {
            self.notes.push(note);
        }
        self.comment_scroll = 0;
        self.comment_anchor = None;
        self.mode = Mode::Normal;
    }

    // ── Line-select mode ─────────────────────────────────────────────────────

    pub fn enter_line_select(&mut self) {
        let has_lines = self.current_rich_diff.as_ref()
            .and_then(|d| d.hunks.get(self.selected_hunk))
            .map(|h| !h.lines.is_empty())
            .unwrap_or(false);
        if has_lines {
            let start = self.first_visible_hunk_line_idx();
            self.mode = Mode::LineSelect {
                hunk_idx: self.selected_hunk,
                anchor_line: start,
                active_line: start,
            };
        }
    }

    /// Move the cursor up, repositioning both anchor and active (no selection change).
    pub fn line_select_up(&mut self) {
        if let Mode::LineSelect { ref mut anchor_line, ref mut active_line, .. } = self.mode {
            let new_pos = active_line.saturating_sub(1);
            *anchor_line = new_pos;
            *active_line = new_pos;
        }
        self.scroll_line_select_into_view();
    }

    /// Move the cursor down, repositioning both anchor and active (no selection change).
    pub fn line_select_down(&mut self) {
        if let Mode::LineSelect { hunk_idx, ref mut anchor_line, ref mut active_line, .. } = self.mode {
            let max = self.current_rich_diff.as_ref()
                .and_then(|d| d.hunks.get(hunk_idx))
                .map(|h| h.lines.len().saturating_sub(1))
                .unwrap_or(0);
            let new_pos = (*active_line + 1).min(max);
            *anchor_line = new_pos;
            *active_line = new_pos;
        }
        self.scroll_line_select_into_view();
    }

    /// Extend the selection upward (anchor stays, active moves up).
    pub fn line_select_extend_up(&mut self) {
        if let Mode::LineSelect { ref mut active_line, .. } = self.mode {
            *active_line = active_line.saturating_sub(1);
        }
        self.scroll_line_select_into_view();
    }

    /// Extend the selection downward (anchor stays, active moves down).
    pub fn line_select_extend_down(&mut self) {
        if let Mode::LineSelect { hunk_idx, ref mut active_line, .. } = self.mode {
            let max = self.current_rich_diff.as_ref()
                .and_then(|d| d.hunks.get(hunk_idx))
                .map(|h| h.lines.len().saturating_sub(1))
                .unwrap_or(0);
            *active_line = (*active_line + 1).min(max);
        }
        self.scroll_line_select_into_view();
    }

    pub fn selected_range_has_note(&self) -> bool {
        let Mode::LineSelect { hunk_idx, anchor_line, active_line } = self.mode else { return false };
        let Some(ref diff) = self.current_rich_diff else { return false };
        let Some(hunk) = diff.hunks.get(hunk_idx) else { return false };
        let Some(range) = line_indices_to_range(hunk, anchor_line, active_line) else { return false };
        self.notes.iter().any(|n| {
            n.file == diff.file.path && n.hunk_header == hunk.header && n.line_range.as_ref() == Some(&range)
        })
    }

    pub fn start_comment_for_selection(&mut self) {
        let Mode::LineSelect { hunk_idx, anchor_line, active_line } = self.mode else { return };
        let Some(ref diff) = self.current_rich_diff else { return };
        let Some(hunk) = diff.hunks.get(hunk_idx) else { return };
        let Some(range) = line_indices_to_range(hunk, anchor_line, active_line) else { return };

        let file = diff.file.path.clone();
        let header = hunk.header.clone();

        let existing = self.notes.iter()
            .find(|n| n.file == file && n.hunk_header == header && n.line_range.as_ref() == Some(&range))
            .cloned();

        if let Some(original) = existing {
            self.notes.retain(|n| !(n.file == file && n.hunk_header == header && n.line_range.as_ref() == Some(&range)));
            self.expanded_notes.clear();
            if self.selected_note >= self.notes.len() && !self.notes.is_empty() {
                self.selected_note = self.notes.len() - 1;
            }
            let cursor = original.note.len();
            let input = original.note.clone();
            self.comment_scroll = 0;
            self.comment_anchor = None;
            self.mode = Mode::Comment {
                hunk_idx,
                input,
                cursor,
                original: Some(original),
                line_range: Some(range),
            };
        } else {
            self.comment_scroll = 0;
            self.comment_anchor = None;
            self.mode = Mode::Comment {
                hunk_idx,
                input: String::new(),
                cursor: 0,
                original: None,
                line_range: Some(range),
            };
        }
    }

    pub fn delete_note_for_selection(&mut self) {
        let Mode::LineSelect { hunk_idx, anchor_line, active_line } = self.mode else { return };
        let Some(ref diff) = self.current_rich_diff else { return };
        let Some(hunk) = diff.hunks.get(hunk_idx) else { return };
        let Some(range) = line_indices_to_range(hunk, anchor_line, active_line) else { return };

        let file = diff.file.path.clone();
        let header = hunk.header.clone();
        self.notes.retain(|n| !(n.file == file && n.hunk_header == header && n.line_range.as_ref() == Some(&range)));
        self.expanded_notes.clear();
        self.notes_scroll = 0;
        if self.selected_note >= self.notes.len() && !self.notes.is_empty() {
            self.selected_note = self.notes.len() - 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::app::{App, Mode};
    use crate::app::test_helpers::*;
    use crate::segment::RichDiffFile;
    use std::path::PathBuf;

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
        app.current_rich_diff = Some(RichDiffFile {
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
        assert!(matches!(
            &app.mode,
            Mode::Comment { input, .. } if input == "original note"
        ));
    }

    #[test]
    fn test_start_comment_does_not_create_duplicate() {
        let mut app = app_with_note_on_hunk(0);
        app.start_comment();
        if let Mode::Comment { ref mut input, .. } = app.mode {
            *input = "original note".to_string();
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
            line_range: None,
        };
        app.submit_comment();
        assert_eq!(app.notes.len(), 1);
        assert_eq!(app.notes[0].note, "This looks wrong");
        assert_eq!(app.notes[0].file, PathBuf::from("file_0.rs"));
    }

    #[test]
    fn test_submit_comment_resets_to_normal() {
        let mut app = app_with_diff(1);
        app.mode = Mode::Comment {
            hunk_idx: 0,
            input: "some note".to_string(),
            cursor: 0,
            original: None,
            line_range: None,
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
            line_range: None,
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
            line_range: None,
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
            line_range: None,
        };
        app.submit_comment();
        assert_eq!(app.notes.len(), 1);
        assert!(app.notes[0].hunk_header.contains("11"));
    }

    // ── Edit / delete notes ───────────────────────────────────────────────────

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
        for hunk_idx in [0, 1] {
            app.selected_hunk = hunk_idx;
            app.mode = Mode::Comment { hunk_idx, input: format!("note {}", hunk_idx), cursor: 0, original: None, line_range: None };
            app.submit_comment();
        }
        app.selected_hunk = 0;
        app.delete_note_for_current_hunk();
        assert_eq!(app.notes.len(), 1);
        assert!(app.notes[0].note.contains("note 1"));
    }

    #[test]
    fn test_delete_note_no_op_when_no_note() {
        let mut app = app_with_diff(2);
        app.delete_note_for_current_hunk();
        assert!(app.notes.is_empty());
    }

    #[test]
    fn test_delete_note_no_op_without_diff() {
        let mut app = App::new(make_files(1), "main".to_string(), "HEAD".to_string());
        app.delete_note_for_current_hunk();
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
            line_range: None,
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
            line_range: None,
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
            line_range: None,
        };
        app.submit_comment();
        assert!(app.notes.is_empty(), "all-whitespace multi-line input should not create a note");
    }

    // ── Notes panel ───────────────────────────────────────────────────────────

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

    // ── Notes panel scrolling ─────────────────────────────────────────────────

    #[test]
    fn test_scroll_notes_no_op_when_selected_visible() {
        let mut app = app_with_many_notes(2);
        app.selected_note = 0;
        app.scroll_notes_to_selected(8);
        assert_eq!(app.notes_scroll, 0);
    }

    #[test]
    fn test_scroll_notes_scrolls_down_when_below_viewport() {
        let mut app = app_with_many_notes(3);
        // 3 collapsed notes = 9 visual rows; viewport=6
        app.selected_note = 2;
        app.scroll_notes_to_selected(6);
        // note 2 ends at row 9, viewport=6 → scroll = 9-6 = 3
        assert_eq!(app.notes_scroll, 3);
    }

    #[test]
    fn test_scroll_notes_scrolls_up_when_above_viewport() {
        let mut app = app_with_many_notes(3);
        app.notes_scroll = 6;
        app.selected_note = 0;
        app.scroll_notes_to_selected(6);
        assert_eq!(app.notes_scroll, 0);
    }

    #[test]
    fn test_delete_selected_note_resets_scroll() {
        let mut app = app_with_many_notes(2);
        app.notes_scroll = 3;
        app.delete_selected_note();
        assert_eq!(app.notes_scroll, 0);
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
        app.delete_selected_note();
        assert!(app.notes.is_empty());
    }

    // ── line_indices_to_range ─────────────────────────────────────────────────

    #[test]
    fn test_line_indices_to_range_forward() {
        use crate::app::notes::line_indices_to_range;
        use crate::app::LineRange;
        let hunk = make_rich_hunk("@@ -1,3 +1,3 @@");
        // hunk.lines[0] is Added (new_lineno=Some(1)), lines[2] is Context (new_lineno=Some(2))
        let r = line_indices_to_range(&hunk, 0, 2).unwrap();
        assert_eq!(r, LineRange::new(1, 2));
    }

    #[test]
    fn test_line_indices_to_range_reversed_normalizes() {
        use crate::app::notes::line_indices_to_range;
        use crate::app::LineRange;
        let hunk = make_rich_hunk("@@ -1,3 +1,3 @@");
        let r = line_indices_to_range(&hunk, 2, 0).unwrap();
        assert_eq!(r, LineRange::new(1, 2));
    }

    #[test]
    fn test_line_indices_to_range_single_line() {
        use crate::app::notes::line_indices_to_range;
        let hunk = make_rich_hunk("@@ -1,3 +1,3 @@");
        let r = line_indices_to_range(&hunk, 0, 0).unwrap();
        assert_eq!(r.start, r.end);
    }

    #[test]
    fn test_line_indices_to_range_removed_fallback_to_old_lineno() {
        use crate::app::notes::line_indices_to_range;
        use crate::diff::{DiffLine, LineKind};
        use crate::segment::{RichHunk, RichLine, Segment};
        use ratatui::style::Color;
        // Build a hunk with only Removed lines (no new_lineno)
        let dl = DiffLine { old_lineno: Some(5), new_lineno: None, kind: LineKind::Removed, content: "x".to_string() };
        let rl = RichLine { diff_line: dl, segments: vec![Segment { content: "x".to_string(), fg: Color::Red, bg: None }] };
        let hunk = RichHunk { header: "@@ -5,1 +0,0 @@".to_string(), old_start: 5, new_start: 0, lines: vec![rl] };
        let r = line_indices_to_range(&hunk, 0, 0).unwrap();
        assert_eq!(r.start, 5);
    }

    // ── enter_line_select / line_select_up / line_select_down ─────────────────

    #[test]
    fn test_enter_line_select_sets_mode() {
        let mut app = app_with_diff(1);
        app.enter_line_select();
        assert!(matches!(app.mode, Mode::LineSelect { hunk_idx: 0, anchor_line: 0, active_line: 0 }));
    }

    #[test]
    fn test_enter_line_select_no_op_without_diff() {
        let mut app = App::new(make_files(1), "main".to_string(), "HEAD".to_string());
        app.enter_line_select();
        assert_eq!(app.mode, Mode::Normal);
    }

    #[test]
    fn test_line_select_down_moves_cursor_both_anchor_and_active() {
        let mut app = app_with_diff(1);
        app.enter_line_select();
        app.line_select_down();
        assert!(matches!(app.mode, Mode::LineSelect { anchor_line: 1, active_line: 1, .. }));
    }

    #[test]
    fn test_line_select_down_clamps_at_last_line() {
        let mut app = app_with_diff(1);
        app.enter_line_select();
        for _ in 0..100 { app.line_select_down(); }
        let last = app.current_rich_diff.as_ref().unwrap().hunks[0].lines.len() - 1;
        assert!(matches!(app.mode, Mode::LineSelect { active_line, .. } if active_line == last));
    }

    #[test]
    fn test_line_select_up_moves_cursor_both_anchor_and_active() {
        let mut app = app_with_diff(1);
        app.enter_line_select();
        app.line_select_down();
        app.line_select_up();
        assert!(matches!(app.mode, Mode::LineSelect { anchor_line: 0, active_line: 0, .. }));
    }

    #[test]
    fn test_line_select_up_clamps_at_zero() {
        let mut app = app_with_diff(1);
        app.enter_line_select();
        app.line_select_up();
        assert!(matches!(app.mode, Mode::LineSelect { active_line: 0, .. }));
    }

    #[test]
    fn test_line_select_extend_down_moves_only_active() {
        let mut app = app_with_diff(1);
        app.enter_line_select();
        app.line_select_extend_down();
        assert!(matches!(app.mode, Mode::LineSelect { anchor_line: 0, active_line: 1, .. }));
    }

    #[test]
    fn test_line_select_extend_up_moves_only_active() {
        let mut app = app_with_diff(1);
        app.enter_line_select();
        app.line_select_extend_down();
        app.line_select_extend_down();
        app.line_select_extend_up();
        assert!(matches!(app.mode, Mode::LineSelect { anchor_line: 0, active_line: 1, .. }));
    }

    #[test]
    fn test_reposition_then_extend_gives_non_zero_start_range() {
        use crate::app::notes::line_indices_to_range;
        let mut app = app_with_diff(1);
        app.enter_line_select();
        app.line_select_down(); // cursor at index 1 (anchor=1, active=1)
        app.line_select_extend_down(); // extend to index 2 (anchor=1, active=2)
        if let Mode::LineSelect { hunk_idx, anchor_line, active_line } = app.mode {
            let hunk = &app.current_rich_diff.as_ref().unwrap().hunks[hunk_idx];
            let range = line_indices_to_range(hunk, anchor_line, active_line).unwrap();
            assert!(range.start > 0, "range should not start at line 0 after repositioning");
        } else {
            panic!("expected LineSelect mode");
        }
    }

    // ── start_comment_for_selection / delete_note_for_selection ───────────────

    #[test]
    fn test_start_comment_for_selection_enters_comment_mode_with_range() {
        let mut app = app_with_diff(1);
        app.enter_line_select();
        app.line_select_extend_down(); // extend selection to active_line=1
        app.start_comment_for_selection();
        assert!(matches!(app.mode, Mode::Comment { line_range: Some(_), .. }));
    }

    #[test]
    fn test_start_comment_for_selection_edit_path_pre_populates_text() {
        let mut app = app_with_diff(1);
        // Create a line-level note first
        app.enter_line_select();
        app.start_comment_for_selection();
        if let Mode::Comment { ref mut input, .. } = app.mode { *input = "line note".to_string(); }
        app.submit_comment();
        assert_eq!(app.notes.len(), 1);
        // Re-enter line select and start_comment → edit path
        app.enter_line_select();
        app.start_comment_for_selection();
        assert!(matches!(&app.mode, Mode::Comment { input, .. } if input == "line note"));
    }

    #[test]
    fn test_delete_note_for_selection_removes_note() {
        let mut app = app_with_diff(1);
        app.enter_line_select();
        app.start_comment_for_selection();
        if let Mode::Comment { ref mut input, .. } = app.mode { *input = "line note".to_string(); }
        app.submit_comment();
        assert_eq!(app.notes.len(), 1);
        app.enter_line_select();
        app.delete_note_for_selection();
        assert!(app.notes.is_empty());
    }

    #[test]
    fn test_delete_note_for_selection_leaves_other_range_notes() {
        let mut app = app_with_diff(1);
        // Create note on line 0 only (file line 1)
        app.enter_line_select();
        app.start_comment_for_selection();
        if let Mode::Comment { ref mut input, .. } = app.mode { *input = "note on line 0".to_string(); }
        app.submit_comment();
        // Create note on lines 0-2 (file lines 1-2) — must reach index 2 to get a different line number
        app.enter_line_select();
        app.line_select_down();
        app.line_select_down(); // active_line=2, file lineno=2
        app.start_comment_for_selection();
        if let Mode::Comment { ref mut input, .. } = app.mode { *input = "note on lines 0-2".to_string(); }
        app.submit_comment();
        assert_eq!(app.notes.len(), 2);
        // Delete only the line-0 note
        app.enter_line_select();
        app.delete_note_for_selection();
        assert_eq!(app.notes.len(), 1);
        assert!(app.notes[0].note.contains("0-2"));
    }

    #[test]
    fn test_submit_comment_with_line_range_stores_range() {
        let mut app = app_with_diff(1);
        app.enter_line_select();
        app.start_comment_for_selection();
        if let Mode::Comment { ref mut input, .. } = app.mode { *input = "targeted".to_string(); }
        app.submit_comment();
        assert!(app.notes[0].line_range.is_some());
    }

    #[test]
    fn test_submit_comment_whole_hunk_stores_none_range() {
        let mut app = app_with_diff(1);
        app.start_comment();
        if let Mode::Comment { ref mut input, .. } = app.mode { *input = "whole hunk".to_string(); }
        app.submit_comment();
        assert!(app.notes[0].line_range.is_none());
    }

    #[test]
    fn test_submit_comment_line_range_hunk_content_scoped() {
        let mut app = app_with_diff(1);
        app.enter_line_select(); // anchor=0, active=0 → just line 0 (Added, new_lineno=Some(1))
        app.start_comment_for_selection();
        if let Mode::Comment { ref mut input, .. } = app.mode { *input = "scoped".to_string(); }
        app.submit_comment();
        // hunk_content should include the first line but not all lines
        let content = &app.notes[0].hunk_content;
        assert!(content.contains("+new line"), "selected added line should be in content");
    }

    #[test]
    fn test_multiple_notes_same_hunk_different_range() {
        let mut app = app_with_diff(1);
        // Note on line 0 only (file line 1)
        app.enter_line_select();
        app.start_comment_for_selection();
        if let Mode::Comment { ref mut input, .. } = app.mode { *input = "note a".to_string(); }
        app.submit_comment();
        // Note on lines 0-2 (file lines 1-2) — index 2 gives a new file line number
        app.enter_line_select();
        app.line_select_down();
        app.line_select_down(); // active_line=2
        app.start_comment_for_selection();
        if let Mode::Comment { ref mut input, .. } = app.mode { *input = "note b".to_string(); }
        app.submit_comment();
        assert_eq!(app.notes.len(), 2);
        assert_ne!(app.notes[0].line_range, app.notes[1].line_range);
    }

    #[test]
    fn test_selected_range_has_note_true() {
        let mut app = app_with_diff(1);
        app.enter_line_select();
        app.start_comment_for_selection();
        if let Mode::Comment { ref mut input, .. } = app.mode { *input = "hi".to_string(); }
        app.submit_comment();
        app.enter_line_select();
        assert!(app.selected_range_has_note());
    }

    #[test]
    fn test_selected_range_has_note_false_different_range() {
        let mut app = app_with_diff(1);
        app.enter_line_select();
        app.start_comment_for_selection();
        if let Mode::Comment { ref mut input, .. } = app.mode { *input = "hi".to_string(); }
        app.submit_comment();
        // Select a different range: go to index 2 which gives a different file line number
        app.enter_line_select();
        app.line_select_down();
        app.line_select_down(); // active_line=2, file line=2 → range is {1,2}, not {1,1}
        assert!(!app.selected_range_has_note());
    }

    #[test]
    fn test_selected_range_has_note_false_in_normal_mode() {
        let app = app_with_diff(1);
        assert!(!app.selected_range_has_note());
    }

    #[test]
    fn test_cancel_comment_restores_line_level_note() {
        let mut app = app_with_diff(1);
        // Create line-level note
        app.enter_line_select();
        app.start_comment_for_selection();
        if let Mode::Comment { ref mut input, .. } = app.mode { *input = "original".to_string(); }
        app.submit_comment();
        // Edit it
        app.enter_line_select();
        app.start_comment_for_selection();
        // Cancel — original should be restored
        app.cancel_comment();
        assert_eq!(app.notes.len(), 1);
        assert_eq!(app.notes[0].note, "original");
        assert!(app.notes[0].line_range.is_some());
    }
}
