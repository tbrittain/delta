use std::path::PathBuf;

use crate::diff::LineKind;
use super::{App, FeedbackNote, Mode};
use super::layout::note_visual_rows;

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
        self.notes_scroll = 0;
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

    // ── Comment flow ─────────────────────────────────────────────────────────

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

#[cfg(test)]
mod tests {
    use crate::app::{App, Mode};
    use crate::app::test_helpers::*;
    use crate::diff::DiffFile;
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
            app.mode = Mode::Comment { hunk_idx, input: format!("note {}", hunk_idx), cursor: 0, original: None };
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
}
