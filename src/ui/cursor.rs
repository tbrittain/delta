use ratatui::{
    style::{Color, Style},
    text::Span,
};

use super::SEL_BG;

// ── Visual-line helpers ───────────────────────────────────────────────────────

/// One screen row of the comment editor after line wrapping.
#[derive(Debug, Clone)]
pub(super) struct VisualLine {
    pub(super) text: String,
    /// Byte offset of this row's first character in the full input string.
    pub(super) byte_start: usize,
    /// True if this is the last visual row of its logical (`\n`-terminated) line,
    /// meaning the cursor may sit at `byte_start + text.len()`.
    pub(super) is_eol: bool,
}

/// Split `input` into visual lines of at most `content_width` characters.
pub(super) fn compute_visual_lines(input: &str, content_width: usize) -> Vec<VisualLine> {
    let cw = content_width.max(1);
    let mut result = Vec::new();
    let mut byte_pos = 0usize;

    for logical_line in input.split('\n') {
        if logical_line.is_empty() {
            result.push(VisualLine { text: String::new(), byte_start: byte_pos, is_eol: true });
        } else {
            let chars: Vec<(usize, char)> = logical_line.char_indices().collect();
            let char_count = chars.len();
            let mut char_start = 0;
            while char_start < char_count {
                let char_end = (char_start + cw).min(char_count);
                let byte_start = byte_pos + chars[char_start].0;
                let text: String = chars[char_start..char_end].iter().map(|(_, c)| *c).collect();
                let is_eol = char_end == char_count;
                result.push(VisualLine { text, byte_start, is_eol });
                char_start = char_end;
            }
        }
        byte_pos += logical_line.len() + 1; // +1 for '\n'
    }

    result
}

/// Return `(visual_row, visual_col)` for `cursor` within pre-computed visual lines.
pub(super) fn visual_row_and_col(cursor: usize, visual_lines: &[VisualLine]) -> (usize, usize) {
    for (vrow, vl) in visual_lines.iter().enumerate() {
        let vl_end = vl.byte_start + vl.text.len();
        let on_line = if vl.is_eol {
            cursor >= vl.byte_start && cursor <= vl_end
        } else {
            cursor >= vl.byte_start && cursor < vl_end
        };
        if on_line {
            let col_bytes = (cursor - vl.byte_start).min(vl.text.len());
            return (vrow, vl.text[..col_bytes].chars().count());
        }
    }
    let last = visual_lines.len().saturating_sub(1);
    let last_col = visual_lines.last().map(|vl| vl.text.chars().count()).unwrap_or(0);
    (last, last_col)
}

/// Move the cursor to the visual row above, preserving visual column.
pub(super) fn cursor_up_visual(input: &str, cursor: usize, content_width: usize) -> usize {
    let vls = compute_visual_lines(input, content_width);
    let (vrow, vcol) = visual_row_and_col(cursor, &vls);
    if vrow == 0 { return cursor; }
    let prev = &vls[vrow - 1];
    let target = vcol.min(prev.text.chars().count());
    prev.byte_start + prev.text.char_indices().nth(target).map(|(b, _)| b).unwrap_or(prev.text.len())
}

/// Move the cursor to the visual row below, preserving visual column.
pub(super) fn cursor_down_visual(input: &str, cursor: usize, content_width: usize) -> usize {
    let vls = compute_visual_lines(input, content_width);
    let (vrow, vcol) = visual_row_and_col(cursor, &vls);
    if vrow + 1 >= vls.len() { return cursor; }
    let next = &vls[vrow + 1];
    let target = vcol.min(next.text.chars().count());
    next.byte_start + next.text.char_indices().nth(target).map(|(b, _)| b).unwrap_or(next.text.len())
}

// ── Character-level cursor movement ──────────────────────────────────────────

pub(super) fn cursor_prev(s: &str, cursor: usize) -> usize {
    if cursor == 0 { return 0; }
    let mut pos = cursor - 1;
    while pos > 0 && !s.is_char_boundary(pos) { pos -= 1; }
    pos
}

pub(super) fn cursor_next(s: &str, cursor: usize) -> usize {
    if cursor >= s.len() { return s.len(); }
    let mut pos = cursor + 1;
    while pos < s.len() && !s.is_char_boundary(pos) { pos += 1; }
    pos
}

pub(super) fn cursor_home(input: &str, cursor: usize) -> usize {
    input[..cursor].rfind('\n').map(|p| p + 1).unwrap_or(0)
}

pub(super) fn cursor_end(input: &str, cursor: usize) -> usize {
    input[cursor..].find('\n').map(|p| cursor + p).unwrap_or(input.len())
}

pub(super) fn cursor_word_left(input: &str, cursor: usize) -> usize {
    if cursor == 0 { return 0; }
    let chars: Vec<(usize, char)> = input[..cursor].char_indices().collect();
    let n = chars.len();
    let mut i = n;
    while i > 0 && !is_word_char(chars[i - 1].1) { i -= 1; }
    while i > 0 && is_word_char(chars[i - 1].1) { i -= 1; }
    if i == 0 { 0 } else { chars[i].0 }
}

pub(super) fn cursor_word_right(input: &str, cursor: usize) -> usize {
    if cursor >= input.len() { return input.len(); }
    let chars: Vec<(usize, char)> = input[cursor..].char_indices().collect();
    let n = chars.len();
    let mut i = 0;
    while i < n && is_word_char(chars[i].1) { i += 1; }
    while i < n && !is_word_char(chars[i].1) { i += 1; }
    cursor + if i < n { chars[i].0 } else { input[cursor..].len() }
}

pub(super) fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

// ── Selection-aware line rendering ────────────────────────────────────────────

/// Build styled spans for one visual line. Handles selection highlighting only;
/// the cursor itself is rendered by `frame.set_cursor_position`.
pub(crate) fn line_spans(
    line_text: &str,
    line_start_byte: usize,
    selection: Option<(usize, usize)>,
) -> Vec<Span<'static>> {
    let (ls, le) = match selection {
        None => (0, 0),
        Some((s, e)) => {
            let ls = if s <= line_start_byte { 0 } else { (s - line_start_byte).min(line_text.len()) };
            let le = if e <= line_start_byte { 0 } else { (e - line_start_byte).min(line_text.len()) };
            (ls, le)
        }
    };
    if ls >= le {
        return vec![Span::raw(line_text.to_string())];
    }
    let sel_style = Style::default().bg(SEL_BG).fg(Color::White);
    let mut spans: Vec<Span<'static>> = Vec::new();
    if ls > 0            { spans.push(Span::raw(line_text[..ls].to_string())); }
    spans.push(Span::styled(line_text[ls..le].to_string(), sel_style));
    if le < line_text.len() { spans.push(Span::raw(line_text[le..].to_string())); }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spans_text(spans: &[Span<'static>]) -> String {
        spans.iter().map(|s| s.content.as_ref()).collect()
    }

    fn has_sel_bg(span: &Span<'static>) -> bool {
        span.style.bg == Some(SEL_BG)
    }

    // ── compute_visual_lines ──────────────────────────────────────────────────

    #[test]
    fn test_visual_lines_short_no_wrap() {
        let vls = compute_visual_lines("hello", 10);
        assert_eq!(vls.len(), 1);
        assert_eq!(vls[0].text, "hello");
        assert_eq!(vls[0].byte_start, 0);
        assert!(vls[0].is_eol);
    }

    #[test]
    fn test_visual_lines_wraps_long_line() {
        let vls = compute_visual_lines("hellothere", 5);
        assert_eq!(vls.len(), 2);
        assert_eq!(vls[0].text, "hello"); assert_eq!(vls[0].byte_start, 0); assert!(!vls[0].is_eol);
        assert_eq!(vls[1].text, "there"); assert_eq!(vls[1].byte_start, 5); assert!(vls[1].is_eol);
    }

    #[test]
    fn test_visual_lines_empty_line() {
        let vls = compute_visual_lines("", 5);
        assert_eq!(vls.len(), 1);
        assert_eq!(vls[0].text, "");
        assert!(vls[0].is_eol);
    }

    #[test]
    fn test_visual_lines_multiline() {
        let vls = compute_visual_lines("hi\nhellothere", 5);
        assert_eq!(vls.len(), 3);
        assert_eq!(vls[0].text, "hi");    assert_eq!(vls[0].byte_start, 0); assert!(vls[0].is_eol);
        assert_eq!(vls[1].text, "hello"); assert_eq!(vls[1].byte_start, 3); assert!(!vls[1].is_eol);
        assert_eq!(vls[2].text, "there"); assert_eq!(vls[2].byte_start, 8); assert!(vls[2].is_eol);
    }

    #[test]
    fn test_visual_lines_byte_starts() {
        let vls = compute_visual_lines("abc\ndefghi", 3);
        assert_eq!(vls[1].byte_start, 4); // "def" starts at byte 4 ("abc\n" = 4 bytes)
        assert_eq!(vls[2].byte_start, 7); // "ghi" starts at byte 7
    }

    // ── visual_row_and_col ────────────────────────────────────────────────────

    #[test]
    fn test_vrow_col_start() {
        let vls = compute_visual_lines("hello", 5);
        assert_eq!(visual_row_and_col(0, &vls), (0, 0));
    }

    #[test]
    fn test_vrow_col_end_of_line() {
        let vls = compute_visual_lines("hello", 5);
        assert_eq!(visual_row_and_col(5, &vls), (0, 5));
    }

    #[test]
    fn test_vrow_col_second_visual_row() {
        let vls = compute_visual_lines("hellothere", 5);
        assert_eq!(visual_row_and_col(7, &vls), (1, 2)); // 't','h' + cursor = col 2 in "there"
    }

    #[test]
    fn test_vrow_col_wrap_boundary() {
        let vls = compute_visual_lines("hellothere", 5);
        assert_eq!(visual_row_and_col(5, &vls), (1, 0)); // start of "there"
    }

    // ── cursor_up/down_visual ─────────────────────────────────────────────────

    #[test]
    fn test_cursor_up_visual_logical_lines() {
        assert_eq!(cursor_up_visual("hello\nworld", 6, 10), 0);
    }

    #[test]
    fn test_cursor_up_visual_across_wrap() {
        // "hellothere" width=5: row0="hello", row1="there"
        // cursor at byte 7 (col 2 in "there") → col 2 in "hello" = byte 2
        assert_eq!(cursor_up_visual("hellothere", 7, 5), 2);
    }

    #[test]
    fn test_cursor_up_visual_first_row_no_op() {
        assert_eq!(cursor_up_visual("hello", 3, 10), 3);
    }

    #[test]
    fn test_cursor_down_visual_logical_lines() {
        assert_eq!(cursor_down_visual("hello\nworld", 0, 10), 6);
    }

    #[test]
    fn test_cursor_down_visual_across_wrap() {
        assert_eq!(cursor_down_visual("hellothere", 2, 5), 7);
    }

    #[test]
    fn test_cursor_down_visual_last_row_no_op() {
        assert_eq!(cursor_down_visual("hello", 3, 10), 3);
    }

    #[test]
    fn test_cursor_down_visual_clamps_to_short_row() {
        // "hellothere\nhi" width=5: visual rows "hello","there","hi"
        // cursor at col 4 of "there" (byte 9) → clamp to end of "hi" (byte 13)
        assert_eq!(cursor_down_visual("hellothere\nhi", 9, 5), 13);
    }

    // ── line_spans ────────────────────────────────────────────────────────────

    #[test]
    fn test_line_spans_no_selection() {
        let spans = line_spans("hello", 0, None);
        assert_eq!(spans_text(&spans), "hello");
        assert!(spans.iter().all(|s| !has_sel_bg(s)));
    }

    #[test]
    fn test_line_spans_no_block_cursor_char() {
        assert!(!spans_text(&line_spans("hello", 0, None)).contains("█"));
    }

    #[test]
    fn test_line_spans_selection_middle() {
        let spans = line_spans("hello", 0, Some((1, 4)));
        assert_eq!(spans_text(&spans), "hello");
        assert!(has_sel_bg(spans.iter().find(|s| s.content.as_ref() == "ell").unwrap()));
    }

    #[test]
    fn test_line_spans_fully_selected() {
        let spans = line_spans("world", 6, Some((3, 20)));
        assert_eq!(spans_text(&spans), "world");
        assert!(spans.iter().all(has_sel_bg));
    }

    #[test]
    fn test_line_spans_selection_not_on_this_line() {
        let spans = line_spans("hello", 0, Some((10, 20)));
        assert!(spans.iter().all(|s| !has_sel_bg(s)));
    }

    #[test]
    fn test_line_spans_empty_line() {
        assert_eq!(spans_text(&line_spans("", 0, None)), "");
    }

    // ── cursor helpers ────────────────────────────────────────────────────────

    #[test] fn test_cursor_home_to_start()     { assert_eq!(cursor_home("hello\nworld", 9), 6); }
    #[test] fn test_cursor_home_first_line()   { assert_eq!(cursor_home("hello", 3), 0); }
    #[test] fn test_cursor_end_to_end()        { assert_eq!(cursor_end("hello\nworld", 0), 5); }
    #[test] fn test_cursor_end_last_line()     { assert_eq!(cursor_end("hello\nworld", 8), 11); }
    #[test] fn test_word_left_to_start()       { assert_eq!(cursor_word_left("foo bar baz", 11), 8); }
    #[test] fn test_word_left_skips_space()    { assert_eq!(cursor_word_left("foo bar", 4), 0); }
    #[test] fn test_word_right_past_word()     { assert_eq!(cursor_word_right("foo bar baz", 0), 4); }
    #[test] fn test_word_right_from_middle()   { assert_eq!(cursor_word_right("foo bar", 1), 4); }
}
