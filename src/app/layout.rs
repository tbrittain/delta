use crate::diff::LineKind;
use crate::segment::RichLine;
use super::{App, FeedbackNote, Mode, FOLD_THRESHOLD};

/// Visual rows occupied by one note entry in the notes panel.
/// Collapsed: header + first-line-of-note + blank = 3 rows.
/// Expanded: header + all note lines + blank.
pub(super) fn note_visual_rows(note: &FeedbackNote, expanded: bool) -> usize {
    if expanded {
        1 + note.note.lines().count().max(1) + 1
    } else {
        3
    }
}

/// Number of visual (screen) rows a single diff line occupies when the diff panel
/// wraps at `panel_width` columns. `panel_width` is the full inner width of the diff
/// panel including the 6-column gutter (4 lineno + 1 space + 1 prefix).
/// Returns 1 when `panel_width` is 0 (wrap accounting disabled).
pub(crate) fn visual_rows_for_diff_line(content: &str, panel_width: usize) -> usize {
    if panel_width == 0 { return 1; }
    let total = 6 + content.chars().count(); // gutter always occupies 6 chars on the first row
    total.div_ceil(panel_width)
}

/// Split the available inner panel width into `(left_col, right_col)`,
/// reserving 1 column for the `│` divider between the two halves.
pub(crate) fn split_column_widths(available: usize) -> (usize, usize) {
    let left  = available.saturating_sub(1) / 2;
    let right = available.saturating_sub(1) - left;
    (left, right)
}

/// Gutter width for one column in split view: 4-digit line number + 1 space.
pub(crate) const SPLIT_GUTTER: usize = 5;

/// Visual rows a content string occupies in one split column of `col_width`.
/// Returns 1 when `col_width` is too narrow to hold any content (guards against divide-by-zero).
pub(crate) fn visual_rows_for_split_content(content: &str, col_width: usize) -> usize {
    let area = col_width.saturating_sub(SPLIT_GUTTER);
    if area == 0 { return 1; }
    content.chars().count().div_ceil(area).max(1)
}

/// Visual height of a paired side-by-side row: `max(left_rows, right_rows)`.
/// An absent side contributes 1 row (blank line).
pub(crate) fn split_pair_height(
    left:  Option<&str>,
    right: Option<&str>,
    left_col: usize,
    right_col: usize,
) -> usize {
    let lh = left .map(|s| visual_rows_for_split_content(s, left_col) ).unwrap_or(1);
    let rh = right.map(|s| visual_rows_for_split_content(s, right_col)).unwrap_or(1);
    lh.max(rh)
}

/// Count the visual lines a slice of rich lines occupies when context runs are folded.
/// Runs of context lines >= FOLD_THRESHOLD collapse to a single placeholder line.
/// `panel_width` is passed to `visual_rows_for_diff_line` for non-context (changed) lines;
/// context lines are assumed short and counted as 1 row each.
pub(super) fn context_run_visual_lines(lines: &[RichLine], panel_width: usize) -> usize {
    let mut count = 0;
    let mut ctx_run = 0;
    for line in lines {
        if line.diff_line.kind == LineKind::Context {
            ctx_run += 1;
        } else {
            count += if ctx_run >= FOLD_THRESHOLD { 1 } else { ctx_run };
            ctx_run = 0;
            count += visual_rows_for_diff_line(&line.diff_line.content, panel_width);
        }
    }
    count += if ctx_run >= FOLD_THRESHOLD { 1 } else { ctx_run };
    count
}

/// True if the given lines contain at least one context run long enough to fold.
pub(super) fn hunk_has_foldable_context(lines: &[RichLine]) -> bool {
    let mut ctx_run = 0;
    for line in lines {
        if line.diff_line.kind == LineKind::Context {
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
        visual_row += if char_count == 0 { 1 } else { char_count.div_ceil(cw) };
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

impl App {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_helpers::{app_with_diff, make_rich_lines};
    use crate::app::Mode;
    use crate::diff::LineKind;
    use crate::app::FOLD_THRESHOLD;

    // ── visual_row_for_cursor ─────────────────────────────────────────────────

    #[test]
    fn test_visual_row_no_wrap() {
        assert_eq!(visual_row_for_cursor("hello\nworld", 0,   100), 0);
        assert_eq!(visual_row_for_cursor("hello\nworld", 5,   100), 0);
        assert_eq!(visual_row_for_cursor("hello\nworld", 6,   100), 1);
        assert_eq!(visual_row_for_cursor("hello\nworld", 11,  100), 1);
    }

    #[test]
    fn test_visual_row_with_wrap() {
        assert_eq!(visual_row_for_cursor("hellothere", 0, 5), 0);
        assert_eq!(visual_row_for_cursor("hellothere", 4, 5), 0);
        assert_eq!(visual_row_for_cursor("hellothere", 5, 5), 1);
        assert_eq!(visual_row_for_cursor("hellothere", 10, 5), 1);
    }

    #[test]
    fn test_visual_row_multiline_with_wrap() {
        let input = "hi\nhellothere";
        assert_eq!(visual_row_for_cursor(input, 0,  5), 0);
        assert_eq!(visual_row_for_cursor(input, 2,  5), 0);
        assert_eq!(visual_row_for_cursor(input, 3,  5), 1);
        assert_eq!(visual_row_for_cursor(input, 7,  5), 1);
        assert_eq!(visual_row_for_cursor(input, 8,  5), 2);
        assert_eq!(visual_row_for_cursor(input, 13, 5), 2);
    }

    // ── Comment popup scrolling ───────────────────────────────────────────────

    #[test]
    fn test_scroll_comment_to_cursor_scrolls_down_when_cursor_below_viewport() {
        let mut app = app_with_diff(1);
        let input = "a\nb\nc\nd\ne".to_string();
        let cursor = input.len();
        app.mode = Mode::Comment { hunk_idx: 0, input, cursor, original: None };
        app.scroll_comment_to_cursor(3, 100);
        assert_eq!(app.comment_scroll, 2);
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

    // ── Context folding ───────────────────────────────────────────────────────

    #[test]
    fn test_context_run_visual_lines_short_run_shown_as_is() {
        let lines = make_rich_lines(&[LineKind::Context; 3]);
        assert_eq!(context_run_visual_lines(&lines, 0), 3);
    }

    #[test]
    fn test_context_run_visual_lines_long_run_folds_to_one() {
        let lines = make_rich_lines(&[LineKind::Context; FOLD_THRESHOLD]);
        assert_eq!(context_run_visual_lines(&lines, 0), 1);
    }

    #[test]
    fn test_context_run_visual_lines_mixed() {
        let mut kinds = vec![LineKind::Added];
        kinds.extend(vec![LineKind::Context; 2]);
        kinds.push(LineKind::Added);
        kinds.extend(vec![LineKind::Context; FOLD_THRESHOLD]);
        kinds.push(LineKind::Added);
        let lines = make_rich_lines(&kinds);
        // visual: 1 + 2 + 1 + 1(fold) + 1 = 6
        assert_eq!(context_run_visual_lines(&lines, 0), 6);
    }

    #[test]
    fn test_hunk_has_foldable_context_false_when_below_threshold() {
        let lines = make_rich_lines(&[LineKind::Context; FOLD_THRESHOLD - 1]);
        assert!(!hunk_has_foldable_context(&lines));
    }

    #[test]
    fn test_hunk_has_foldable_context_true_at_threshold() {
        let lines = make_rich_lines(&[LineKind::Context; FOLD_THRESHOLD]);
        assert!(hunk_has_foldable_context(&lines));
    }

    // ── split_column_widths ───────────────────────────────────────────────────

    #[test]
    fn test_split_column_widths_even_available() {
        assert_eq!(split_column_widths(81), (40, 40));
    }

    #[test]
    fn test_split_column_widths_odd_available() {
        // 80 - 1 divider = 79; left = 39, right = 40
        assert_eq!(split_column_widths(80), (39, 40));
    }

    #[test]
    fn test_split_column_widths_zero_available() {
        assert_eq!(split_column_widths(0), (0, 0));
    }

    #[test]
    fn test_split_column_widths_one_available() {
        // 1 - 1 = 0; both zero
        assert_eq!(split_column_widths(1), (0, 0));
    }

    // ── visual_rows_for_split_content ─────────────────────────────────────────

    #[test]
    fn test_split_content_short_line_fits_in_one_row() {
        // col_width=20, area=15, content=10 chars → 1 row
        assert_eq!(visual_rows_for_split_content("0123456789", 20), 1);
    }

    #[test]
    fn test_split_content_exactly_fills_area() {
        // col_width=20, area=15; 15 chars → 1 row
        let s = "x".repeat(15);
        assert_eq!(visual_rows_for_split_content(&s, 20), 1);
    }

    #[test]
    fn test_split_content_one_char_over_wraps() {
        let s = "x".repeat(16);
        assert_eq!(visual_rows_for_split_content(&s, 20), 2);
    }

    #[test]
    fn test_split_content_zero_col_width_returns_one() {
        assert_eq!(visual_rows_for_split_content("hello", 0), 1);
    }

    #[test]
    fn test_split_content_empty_string_returns_one() {
        assert_eq!(visual_rows_for_split_content("", 20), 1);
    }

    // ── split_pair_height ─────────────────────────────────────────────────────

    #[test]
    fn test_split_pair_height_both_fit_one_row() {
        assert_eq!(split_pair_height(Some("short"), Some("short"), 20, 20), 1);
    }

    #[test]
    fn test_split_pair_height_left_taller() {
        let long = "x".repeat(16);
        assert_eq!(split_pair_height(Some(&long), Some("short"), 20, 20), 2);
    }

    #[test]
    fn test_split_pair_height_right_taller() {
        let long = "x".repeat(16);
        assert_eq!(split_pair_height(Some("short"), Some(&long), 20, 20), 2);
    }

    #[test]
    fn test_split_pair_height_absent_left_counts_as_one() {
        assert_eq!(split_pair_height(None, Some("short"), 20, 20), 1);
    }

    #[test]
    fn test_split_pair_height_absent_right_counts_as_one() {
        assert_eq!(split_pair_height(Some("short"), None, 20, 20), 1);
    }

    #[test]
    fn test_split_pair_height_both_absent_returns_one() {
        assert_eq!(split_pair_height(None::<&str>, None::<&str>, 20, 20), 1);
    }

    // ── visual_rows_for_diff_line ─────────────────────────────────────────────

    #[test]
    fn test_visual_rows_zero_width_returns_one() {
        assert_eq!(visual_rows_for_diff_line("long line content", 0), 1);
    }

    #[test]
    fn test_visual_rows_short_line_fits_in_one_row() {
        assert_eq!(visual_rows_for_diff_line("0123456789", 80), 1);
    }

    #[test]
    fn test_visual_rows_exactly_fills_panel() {
        let content = "x".repeat(74);
        assert_eq!(visual_rows_for_diff_line(&content, 80), 1);
    }

    #[test]
    fn test_visual_rows_one_char_over_wraps_to_two() {
        let content = "x".repeat(75);
        assert_eq!(visual_rows_for_diff_line(&content, 80), 2);
    }

    #[test]
    fn test_visual_rows_double_panel_width_gives_two_rows() {
        let content = "x".repeat(154);
        assert_eq!(visual_rows_for_diff_line(&content, 80), 2);
    }

    // ── note_visual_rows ─────────────────────────────────────────────────────

    #[test]
    fn test_note_visual_rows_collapsed() {
        use crate::app::FeedbackNote;
        use std::path::PathBuf;
        let note = FeedbackNote {
            file: PathBuf::from("src/foo.rs"),
            hunk_header: "@@ -1,1 +1,1 @@".to_string(),
            hunk_content: String::new(),
            note: "single line".to_string(),
        };
        assert_eq!(note_visual_rows(&note, false), 3);
    }

    #[test]
    fn test_note_visual_rows_expanded_multiline() {
        use crate::app::FeedbackNote;
        use std::path::PathBuf;
        let note = FeedbackNote {
            file: PathBuf::from("src/foo.rs"),
            hunk_header: "@@ -1,1 +1,1 @@".to_string(),
            hunk_content: String::new(),
            note: "line one\nline two\nline three".to_string(),
        };
        // header(1) + 3 lines + blank(1) = 5
        assert_eq!(note_visual_rows(&note, true), 5);
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
        let (s, c) = delete_selection(input, 5, Some(6)).unwrap();
        assert_eq!(s, "line1line2\nline3");
        assert_eq!(c, 5);
    }

    #[test]
    fn test_delete_selection_multiline_span() {
        let input = "hello\nworld";
        let (s, c) = delete_selection(input, 8, Some(3)).unwrap();
        assert_eq!(s, "helrld");
        assert_eq!(c, 3);
    }
}
