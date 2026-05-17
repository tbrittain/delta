use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
};

use crate::app::{App, Mode, Panel, FOLD_THRESHOLD};
use crate::app::layout::{SPLIT_GUTTER, split_column_widths, split_pair_height};
use crate::diff::LineKind;
use crate::segment::{RichHunk, RichLine};
use super::{ACCENT, MUTED, NOTE_FG, SEL_BG};

/// Expand tabs to 4 spaces and strip carriage returns.
fn render_safe(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\t' => out.push_str("    "),
            '\r' => {}
            c    => out.push(c),
        }
    }
    out
}

/// One visual row in the side-by-side layout.
pub(super) enum SideBySideRow<'a> {
    FoldPlaceholder(usize),
    Content { left: Option<&'a RichLine>, right: Option<&'a RichLine> },
}

/// Pair a hunk's `RichLine`s into side-by-side rows.
///
/// Context lines appear on both sides. Consecutive Removed/Added runs are
/// paired 1:1; leftover Removed lines get `right: None` and leftover Added
/// lines get `left: None`.
pub(super) fn pair_hunk_lines<'a>(lines: &'a [RichLine]) -> Vec<SideBySideRow<'a>> {
    let mut rows = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        if lines[i].diff_line.kind == LineKind::Context {
            rows.push(SideBySideRow::Content {
                left:  Some(&lines[i]),
                right: Some(&lines[i]),
            });
            i += 1;
            continue;
        }

        // Collect a run of Removed lines.
        let removed_start = i;
        while i < lines.len() && lines[i].diff_line.kind == LineKind::Removed {
            i += 1;
        }
        let removed_end = i;

        // Collect the immediately following run of Added lines.
        let added_start = i;
        while i < lines.len() && lines[i].diff_line.kind == LineKind::Added {
            i += 1;
        }
        let added_end = i;

        let pair_count = (removed_end - removed_start).min(added_end - added_start);

        // Paired rows.
        for k in 0..pair_count {
            rows.push(SideBySideRow::Content {
                left:  Some(&lines[removed_start + k]),
                right: Some(&lines[added_start + k]),
            });
        }
        // Leftover Removed (right side absent).
        for k in pair_count..(removed_end - removed_start) {
            rows.push(SideBySideRow::Content {
                left:  Some(&lines[removed_start + k]),
                right: None,
            });
        }
        // Leftover Added (left side absent).
        for k in pair_count..(added_end - added_start) {
            rows.push(SideBySideRow::Content {
                left:  None,
                right: Some(&lines[added_start + k]),
            });
        }

        // Guard against stall on Added-before-Removed or other unexpected order.
        if i == removed_start {
            rows.push(SideBySideRow::Content {
                left:  None,
                right: Some(&lines[i]),
            });
            i += 1;
        }
    }

    rows
}

/// Apply context folding to a list of `SideBySideRow`s.
/// Runs of ≥ `FOLD_THRESHOLD` consecutive context pairs are replaced with a
/// `FoldPlaceholder`. Non-context rows and short context runs pass through.
fn fold_rows<'a>(rows: Vec<SideBySideRow<'a>>) -> Vec<SideBySideRow<'a>> {
    let mut out: Vec<SideBySideRow<'a>> = Vec::new();
    let mut ctx_run: Vec<SideBySideRow<'a>> = Vec::new();

    let flush_ctx = |ctx_run: &mut Vec<SideBySideRow<'a>>, out: &mut Vec<SideBySideRow<'a>>| {
        let n = ctx_run.len();
        if n >= FOLD_THRESHOLD {
            out.push(SideBySideRow::FoldPlaceholder(n));
        } else {
            out.append(ctx_run);
        }
        ctx_run.clear();
    };

    for row in rows {
        let is_ctx_pair = matches!(
            &row,
            SideBySideRow::Content { left: Some(l), right: Some(_) }
                if l.diff_line.kind == LineKind::Context
        );
        if is_ctx_pair {
            ctx_run.push(row);
        } else {
            flush_ctx(&mut ctx_run, &mut out);
            out.push(row);
        }
    }
    flush_ctx(&mut ctx_run, &mut out);
    out
}

/// Determine the line-level background for a diff line kind.
fn line_bg(kind: LineKind) -> Option<Color> {
    match kind {
        LineKind::Added   => Some(Color::Rgb(0, 60, 0)),
        LineKind::Removed => Some(Color::Rgb(70, 0, 0)),
        LineKind::Context => None,
    }
}

/// Render one visual sub-row of a column.
///
/// `visual_row_idx` selects which chunk of the line's content to show when
/// the line is long enough to wrap within the column.  `selected` overrides
/// all backgrounds with `SEL_BG` for line-select highlighting.
/// Returns a `Vec<Span>` that fills exactly `col_width` terminal columns.
fn render_column_row(
    rl: Option<&RichLine>,
    visual_row_idx: usize,
    col_width: usize,
    selected: bool,
) -> Vec<Span<'static>> {
    let content_area = col_width.saturating_sub(SPLIT_GUTTER);

    let rl = match rl {
        None => {
            let style = if selected { Style::default().bg(SEL_BG) } else { Style::default() };
            return vec![Span::styled(" ".repeat(col_width), style)];
        }
        Some(r) => r,
    };

    let bg = if selected { Some(SEL_BG) } else { line_bg(rl.diff_line.kind) };
    let gutter_style = match bg {
        Some(b) => Style::default().fg(Color::DarkGray).bg(b),
        None    => Style::default().fg(Color::DarkGray),
    };

    let gutter_str = if visual_row_idx == 0 {
        let lineno = match rl.diff_line.kind {
            LineKind::Removed => rl.diff_line.old_lineno,
            _                 => rl.diff_line.new_lineno,
        };
        match lineno {
            Some(n) => format!("{:>4} ", n),
            None    => "     ".to_string(),
        }
    } else {
        "     ".to_string()
    };

    let mut spans: Vec<Span<'static>> = vec![Span::styled(gutter_str, gutter_style)];

    if content_area == 0 {
        return spans;
    }

    let row_start = visual_row_idx * content_area;
    let row_end   = row_start + content_area;

    // Walk segments, slicing to chars in [row_start, row_end).
    let mut global_char = 0usize;
    let mut emitted_chars = 0usize;

    'seg: for seg in &rl.segments {
        let safe = render_safe(&seg.content);
        let seg_len = safe.chars().count();
        let seg_end = global_char + seg_len;

        let slice_start = row_start.max(global_char);
        let slice_end   = row_end.min(seg_end);

        if slice_start < slice_end {
            let local_start = slice_start - global_char;
            let local_len   = slice_end - slice_start;
            let extracted: String = safe.chars().skip(local_start).take(local_len).collect();
            emitted_chars += extracted.chars().count();

            let style = if selected {
                Style::default().fg(seg.fg).bg(SEL_BG)
            } else {
                match seg.bg {
                    Some(b) => Style::default().fg(seg.fg).bg(b),
                    None    => match bg {
                        Some(b) => Style::default().fg(seg.fg).bg(b),
                        None    => Style::default().fg(seg.fg),
                    },
                }
            };
            spans.push(Span::styled(extracted, style));
        }

        global_char = seg_end;
        if global_char >= row_end { break 'seg; }
    }

    // Pad remaining content area.
    let remaining = content_area.saturating_sub(emitted_chars);
    if remaining > 0 {
        let pad_style = match bg {
            Some(b) => Style::default().bg(b),
            None    => Style::default(),
        };
        // bg already reflects SEL_BG when selected, so pad_style is correct.
        spans.push(Span::styled(" ".repeat(remaining), pad_style));
    }

    spans
}

/// Build the full `Text<'static>` for the side-by-side diff view.
pub(crate) fn build_split_diff_text(app: &App, note_max_chars: usize) -> Text<'static> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let Some(ref diff) = app.current_rich_diff else {
        lines.push(Line::from(Span::styled("Loading…", Style::default().fg(Color::DarkGray))));
        return Text::from(lines);
    };
    if diff.hunks.is_empty() {
        lines.push(Line::from(Span::styled("No diff content.", Style::default().fg(Color::DarkGray))));
        return Text::from(lines);
    }

    let available = app.diff_view_content_width;
    let (left_col, right_col) = split_column_widths(available);
    let cols = ColWidths { left: left_col, right: right_col };

    for (hunk_idx, hunk) in diff.hunks.iter().enumerate() {
        push_split_hunk(app, hunk, hunk_idx, &diff.file.path, note_max_chars, cols, &mut lines);
    }
    Text::from(lines)
}

#[derive(Clone, Copy)]
struct ColWidths { left: usize, right: usize }

fn push_split_hunk(
    app: &App,
    hunk: &RichHunk,
    hunk_idx: usize,
    file_path: &std::path::Path,
    note_max_chars: usize,
    cols: ColWidths,
    out: &mut Vec<Line<'static>>,
) {
    let (left_col, right_col) = (cols.left, cols.right);
    let is_selected = hunk_idx == app.selected_hunk && app.focused_panel == Panel::DiffView;
    let header_style = if is_selected {
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(MUTED)
    };
    let marker_style = Style::default().fg(ACCENT).add_modifier(Modifier::BOLD);

    // Header spans full width.
    if is_selected {
        out.push(Line::from(vec![
            Span::styled("▶ ", marker_style),
            Span::styled(hunk.header.clone(), header_style),
        ]));
    } else {
        out.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(hunk.header.clone(), header_style),
        ]));
    }

    let sel_window: Option<(usize, usize)> = match app.mode {
        Mode::LineSelect { hunk_idx: sel, anchor_line: a, active_line: b } if sel == hunk_idx =>
            Some((a.min(b), a.max(b))),
        _ => None,
    };

    // Return the index of a RichLine within hunk.lines via pointer identity.
    let line_idx = |rl: &RichLine| -> Option<usize> {
        hunk.lines.iter().position(|l| std::ptr::eq(l, rl))
    };
    let is_selected = |rl: Option<&RichLine>| -> bool {
        rl.and_then(line_idx)
            .map(|i| sel_window.is_some_and(|(lo, hi)| i >= lo && i <= hi))
            .unwrap_or(false)
    };

    let raw_rows = pair_hunk_lines(&hunk.lines);
    let rows = if app.expanded_hunks.contains(&hunk_idx) {
        raw_rows
    } else {
        fold_rows(raw_rows)
    };

    let divider = Span::styled("│", Style::default().fg(Color::DarkGray));
    let note_style = Style::default().fg(NOTE_FG).add_modifier(Modifier::ITALIC);

    let push_note = |note_text: &str, out: &mut Vec<Line<'static>>| {
        for (i, line_text) in note_text.lines().enumerate() {
            let prefix = if i == 0 { "  ◎ " } else { "    " };
            let display = if note_max_chars > 0 && line_text.chars().count() > note_max_chars {
                format!("{}…", line_text.chars().take(note_max_chars.saturating_sub(1)).collect::<String>())
            } else {
                line_text.to_string()
            };
            out.push(Line::from(Span::styled(format!("{}{}", prefix, display), note_style)));
        }
    };

    for row in rows {
        match row {
            SideBySideRow::FoldPlaceholder(n) => {
                let fold_style = Style::default().fg(Color::DarkGray);
                out.push(Line::from(Span::styled(
                    format!("  ·· {} lines of context ··", n), fold_style)));
            }
            SideBySideRow::Content { left, right } => {
                let selected = is_selected(left) || is_selected(right);
                let left_content  = left .map(|rl| &rl.diff_line.content as &str);
                let right_content = right.map(|rl| &rl.diff_line.content as &str);
                let height = split_pair_height(left_content, right_content, left_col, right_col);

                for v in 0..height {
                    let mut spans = render_column_row(left, v, left_col, selected);
                    spans.push(divider.clone());
                    spans.extend(render_column_row(right, v, right_col, selected));
                    out.push(Line::from(spans));
                }

                // Inject line-level notes whose range ends on this row's new_lineno.
                // Use new_lineno from the right (new-file) side; fall back to left.
                let lineno = right.and_then(|r| r.diff_line.new_lineno)
                    .or_else(|| left.and_then(|l| l.diff_line.old_lineno));
                if let Some(n) = lineno {
                    for note in app.notes.iter() {
                        if note.file == file_path && note.hunk_header == hunk.header
                            && note.line_range.as_ref().map(|r| r.end == n).unwrap_or(false)
                        {
                            push_note(&note.note, out);
                        }
                    }
                }
            }
        }
    }

    // Footer: whole-hunk notes only.
    for note in &app.notes {
        if note.file == file_path && note.hunk_header == hunk.header && note.line_range.is_none() {
            push_note(&note.note, out);
        }
    }
    out.push(Line::raw(""));
}

/// Total rendered row count for the current diff in split mode.
/// Used by scroll accounting when `app.view_mode == SideBySide`.
pub(crate) fn split_diff_content_lines(app: &App) -> usize {
    let Some(ref diff) = app.current_rich_diff else { return 0 };
    let available = app.diff_view_content_width;
    let (left_col, right_col) = split_column_widths(available);

    diff.hunks.iter().enumerate().map(|(i, hunk)| {
        let is_expanded = app.expanded_hunks.contains(&i);
        let raw_rows = pair_hunk_lines(&hunk.lines);
        let rows = if is_expanded { raw_rows } else { fold_rows(raw_rows) };

        let content_rows: usize = rows.iter().map(|row| match row {
            SideBySideRow::Content { left, right } => {
                let lc = left .map(|rl| rl.diff_line.content.as_str());
                let rc = right.map(|rl| rl.diff_line.content.as_str());
                split_pair_height(lc, rc, left_col, right_col)
            }
            SideBySideRow::FoldPlaceholder(_) => 1,
        }).sum();

        let note_count = app.notes
            .iter()
            .filter(|n| n.file == diff.file.path && n.hunk_header == hunk.header)
            .count();
        1 + content_rows + note_count + 1 // header + rows + notes + blank
    }).sum()
}

/// Scroll offset of `target_hunk` in split mode.
pub(crate) fn split_hunk_scroll_offset(app: &App, target_hunk: usize) -> usize {
    let Some(ref diff) = app.current_rich_diff else { return 0 };
    let available = app.diff_view_content_width;
    let (left_col, right_col) = split_column_widths(available);
    let mut offset = 0;

    for (i, hunk) in diff.hunks.iter().enumerate() {
        if i >= target_hunk { break; }
        let is_expanded = app.expanded_hunks.contains(&i);
        let raw_rows = pair_hunk_lines(&hunk.lines);
        let rows = if is_expanded { raw_rows } else { fold_rows(raw_rows) };

        let content_rows: usize = rows.iter().map(|row| match row {
            SideBySideRow::Content { left, right } => {
                let lc = left .map(|rl| rl.diff_line.content.as_str());
                let rc = right.map(|rl| rl.diff_line.content.as_str());
                split_pair_height(lc, rc, left_col, right_col)
            }
            SideBySideRow::FoldPlaceholder(_) => 1,
        }).sum();

        let note_count = app.notes
            .iter()
            .filter(|n| n.file == diff.file.path && n.hunk_header == hunk.header)
            .count();
        offset += 1 + content_rows + note_count + 1;
    }
    offset
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test_helpers::{app_with_diff, make_rich_line, make_files};
    use crate::app::{App, ViewMode};
    use crate::diff::{DiffLine, LineKind};
    use crate::segment::{RichDiffFile, RichHunk};

    fn text_str(text: &Text<'static>) -> String {
        text.lines.iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect::<String>())
            .collect::<Vec<_>>().join("\n")
    }

    fn rich_line(kind: LineKind, content: &str, old_no: Option<u32>, new_no: Option<u32>) -> RichLine {
        make_rich_line(DiffLine {
            old_lineno: old_no,
            new_lineno: new_no,
            kind,
            content: content.to_string(),
        })
    }

    fn ctx(content: &str, n: u32) -> RichLine {
        rich_line(LineKind::Context, content, Some(n), Some(n))
    }

    fn rem(content: &str, n: u32) -> RichLine {
        rich_line(LineKind::Removed, content, Some(n), None)
    }

    fn add(content: &str, n: u32) -> RichLine {
        rich_line(LineKind::Added, content, None, Some(n))
    }

    // ── pair_hunk_lines ───────────────────────────────────────────────────────

    fn count_kinds(rows: &[SideBySideRow<'_>]) -> (usize, usize, usize) {
        // (paired, left_only, right_only)
        let mut paired = 0;
        let mut left_only = 0;
        let mut right_only = 0;
        for row in rows {
            if let SideBySideRow::Content { left, right } = row {
                match (left, right) {
                    (Some(_), Some(_)) => paired += 1,
                    (Some(_), None)    => left_only += 1,
                    (None,    Some(_)) => right_only += 1,
                    (None,    None)    => {}
                }
            }
        }
        (paired, left_only, right_only)
    }

    #[test]
    fn test_pair_context_both_sides() {
        let lines = vec![ctx("a", 1), ctx("b", 2)];
        let rows = pair_hunk_lines(&lines);
        let (p, l, r) = count_kinds(&rows);
        assert_eq!((p, l, r), (2, 0, 0));
        // Both sides are the same reference for context
        if let SideBySideRow::Content { left, right } = &rows[0] {
            assert!(left.is_some() && right.is_some());
            assert_eq!(left.unwrap().diff_line.content, "a");
            assert_eq!(right.unwrap().diff_line.content, "a");
        } else { panic!("expected Content"); }
    }

    #[test]
    fn test_pair_equal_removed_added() {
        let lines = vec![rem("old1", 1), rem("old2", 2), add("new1", 1), add("new2", 2)];
        let rows = pair_hunk_lines(&lines);
        assert_eq!(count_kinds(&rows), (2, 0, 0));
    }

    #[test]
    fn test_pair_more_removed_than_added() {
        let lines = vec![rem("r1", 1), rem("r2", 2), rem("r3", 3), add("a1", 1)];
        let rows = pair_hunk_lines(&lines);
        assert_eq!(count_kinds(&rows), (1, 2, 0));
    }

    #[test]
    fn test_pair_more_added_than_removed() {
        let lines = vec![rem("r1", 1), add("a1", 1), add("a2", 2), add("a3", 3)];
        let rows = pair_hunk_lines(&lines);
        assert_eq!(count_kinds(&rows), (1, 0, 2));
    }

    #[test]
    fn test_pair_pure_addition() {
        let lines = vec![add("a1", 1), add("a2", 2)];
        let rows = pair_hunk_lines(&lines);
        assert_eq!(count_kinds(&rows), (0, 0, 2));
    }

    #[test]
    fn test_pair_pure_deletion() {
        let lines = vec![rem("r1", 1), rem("r2", 2)];
        let rows = pair_hunk_lines(&lines);
        assert_eq!(count_kinds(&rows), (0, 2, 0));
    }

    #[test]
    fn test_pair_context_resets_block() {
        let lines = vec![
            rem("r1", 1), add("a1", 1),
            ctx("c",  2),
            rem("r2", 3), add("a2", 2),
        ];
        let rows = pair_hunk_lines(&lines);
        // 2 paired-changed + 1 paired-context = 3 rows; count_kinds sees all 3 as "paired"
        assert_eq!(rows.len(), 3);
        assert_eq!(count_kinds(&rows), (3, 0, 0));
        // Middle is the context row (both sides show same context line)
        if let SideBySideRow::Content { left, right } = &rows[1] {
            assert_eq!(left.unwrap().diff_line.kind, LineKind::Context);
            assert_eq!(right.unwrap().diff_line.kind, LineKind::Context);
        } else { panic!("expected Content for context row"); }
    }

    // ── fold_rows ─────────────────────────────────────────────────────────────

    #[test]
    fn test_fold_short_context_run_not_folded() {
        let lines: Vec<RichLine> = (0..3).map(|i| ctx("c", i as u32 + 1)).collect();
        let rows = pair_hunk_lines(&lines);
        let folded = fold_rows(rows);
        assert!(folded.iter().all(|r| !matches!(r, SideBySideRow::FoldPlaceholder(_))));
        assert_eq!(folded.len(), 3);
    }

    #[test]
    fn test_fold_long_context_run_becomes_placeholder() {
        let lines: Vec<RichLine> = (0..FOLD_THRESHOLD).map(|i| ctx("c", i as u32 + 1)).collect();
        let rows = pair_hunk_lines(&lines);
        let folded = fold_rows(rows);
        assert_eq!(folded.len(), 1);
        assert!(matches!(folded[0], SideBySideRow::FoldPlaceholder(n) if n == FOLD_THRESHOLD));
    }

    #[test]
    fn test_fold_preserves_changed_lines() {
        let mut lines: Vec<RichLine> = (0..FOLD_THRESHOLD).map(|i| ctx("c", i as u32 + 1)).collect();
        lines.push(rem("removed", 10));
        let rows = pair_hunk_lines(&lines);
        let folded = fold_rows(rows);
        // FoldPlaceholder + 1 Content(left_only)
        assert_eq!(folded.len(), 2);
        assert!(matches!(folded[0], SideBySideRow::FoldPlaceholder(_)));
    }

    // ── build_split_diff_text ─────────────────────────────────────────────────

    fn app_with_split_diff(hunk_count: usize) -> App {
        let mut app = app_with_diff(hunk_count);
        app.view_mode = ViewMode::SideBySide;
        app.focused_panel = crate::app::Panel::DiffView;
        app.diff_view_content_width = 80;
        app
    }

    #[test]
    fn test_split_text_contains_divider() {
        let app = app_with_split_diff(1);
        let text = build_split_diff_text(&app, 1000);
        let full = text_str(&text);
        assert!(full.contains('│'), "split view must contain a divider character");
    }

    #[test]
    fn test_split_text_both_contents_present() {
        let files = make_files(1);
        let mut app = App::new(files.clone(), "main".to_string(), "HEAD".to_string());
        app.view_mode = ViewMode::SideBySide;
        app.focused_panel = crate::app::Panel::DiffView;
        app.diff_view_content_width = 80;
        app.current_rich_diff = Some(RichDiffFile {
            file: files[0].clone(),
            hunks: vec![RichHunk {
                header: "@@ -1,1 +1,1 @@".to_string(),
                old_start: 1, new_start: 1,
                lines: vec![
                    rem("old content", 1),
                    add("new content", 1),
                ],
            }],
        });
        let full = text_str(&build_split_diff_text(&app, 1000));
        assert!(full.contains("old content"), "left column must show removed content");
        assert!(full.contains("new content"), "right column must show added content");
    }

    #[test]
    fn test_split_fold_placeholder_appears() {
        use crate::app::FOLD_THRESHOLD;
        let files = make_files(1);
        let mut app = App::new(files.clone(), "main".to_string(), "HEAD".to_string());
        app.view_mode = ViewMode::SideBySide;
        app.focused_panel = crate::app::Panel::DiffView;
        app.diff_view_content_width = 80;

        let mut rich_lines = vec![rem("changed", 1), add("changed2", 1)];
        for i in 0..FOLD_THRESHOLD {
            rich_lines.push(ctx(&format!("ctx{}", i), i as u32 + 2));
        }
        rich_lines.push(rem("another", 10));

        app.current_rich_diff = Some(RichDiffFile {
            file: files[0].clone(),
            hunks: vec![RichHunk {
                header: "@@ -1,10 +1,10 @@".to_string(),
                old_start: 1, new_start: 1,
                lines: rich_lines,
            }],
        });
        let full = text_str(&build_split_diff_text(&app, 1000));
        assert!(full.contains("lines of context"), "long context run must be folded");
        assert!(!full.contains("ctx0"), "folded context lines must not be individually shown");
    }

    #[test]
    fn test_split_submitted_note_shown() {
        use crate::app::Mode;
        let mut app = app_with_split_diff(1);
        app.mode = Mode::Comment { hunk_idx: 0, input: "my split note".to_string(), cursor: 0, original: None, line_range: None };
        app.submit_comment();
        let full = text_str(&build_split_diff_text(&app, 1000));
        assert!(full.contains("my split note"), "note text should appear in split view");
        assert!(full.contains("◎"), "note marker should appear in split view");
    }

    #[test]
    fn test_split_loading_when_no_diff() {
        let files = make_files(1);
        let mut app = App::new(files, "main".to_string(), "HEAD".to_string());
        app.view_mode = ViewMode::SideBySide;
        app.diff_view_content_width = 80;
        let full = text_str(&build_split_diff_text(&app, 1000));
        assert!(full.contains("Loading"));
    }

    // ── split_diff_content_lines ──────────────────────────────────────────────

    #[test]
    fn test_split_content_lines_equal_sides() {
        // 1 removed + 1 added → 1 pair row; hunk = 1 header + 1 pair + 0 notes + 1 blank = 3
        let files = make_files(1);
        let mut app = App::new(files.clone(), "main".to_string(), "HEAD".to_string());
        app.view_mode = ViewMode::SideBySide;
        app.diff_view_content_width = 0; // no wrap
        app.current_rich_diff = Some(RichDiffFile {
            file: files[0].clone(),
            hunks: vec![RichHunk {
                header: "@@ -1,1 +1,1 @@".to_string(),
                old_start: 1, new_start: 1,
                lines: vec![rem("r", 1), add("a", 1)],
            }],
        });
        assert_eq!(split_diff_content_lines(&app), 3);
    }

    #[test]
    fn test_split_content_lines_mismatched_sides() {
        // 3 removed + 1 added → 1 paired + 2 left_only = 3 content rows
        // hunk = 1 header + 3 rows + 0 notes + 1 blank = 5
        let files = make_files(1);
        let mut app = App::new(files.clone(), "main".to_string(), "HEAD".to_string());
        app.view_mode = ViewMode::SideBySide;
        app.diff_view_content_width = 0;
        app.current_rich_diff = Some(RichDiffFile {
            file: files[0].clone(),
            hunks: vec![RichHunk {
                header: "@@ -1,3 +1,1 @@".to_string(),
                old_start: 1, new_start: 1,
                lines: vec![rem("r1", 1), rem("r2", 2), rem("r3", 3), add("a1", 1)],
            }],
        });
        assert_eq!(split_diff_content_lines(&app), 5);
    }

    #[test]
    fn test_split_hunk_scroll_offset_first_hunk_is_zero() {
        let mut app = app_with_diff(3);
        app.view_mode = ViewMode::SideBySide;
        app.diff_view_content_width = 0;
        assert_eq!(split_hunk_scroll_offset(&app, 0), 0);
    }

    #[test]
    fn test_split_hunk_scroll_offset_second_hunk() {
        // make_rich_hunk lines: [Added, Removed, Context]
        // pair_hunk_lines produces: right_only(Added), left_only(Removed), paired(Context) = 3 rows
        // hunk0: 1 header + 3 content rows + 0 notes + 1 blank = 5
        let mut app = app_with_diff(3);
        app.view_mode = ViewMode::SideBySide;
        app.diff_view_content_width = 0;
        assert_eq!(split_hunk_scroll_offset(&app, 1), 5);
    }
}
