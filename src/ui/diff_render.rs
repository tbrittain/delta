use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
};

use crate::app::{App, Mode, Panel, FOLD_THRESHOLD};
use crate::diff::LineKind;
use crate::segment::{RichHunk, RichLine};
use super::{ACCENT, MUTED, NOTE_FG, SEL_BG};

/// Expand tabs to 4 spaces and strip carriage returns so that control
/// characters in source-code content do not corrupt terminal layout.
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

/// Build the gutter prefix (line number + diff sigil) for a `RichLine`.
fn gutter(rl: &RichLine) -> (String, String, Option<Color>) {
    let (prefix, bg) = match rl.diff_line.kind {
        LineKind::Added   => ("+", Some(Color::Rgb(0, 60, 0))),
        LineKind::Removed => ("-", Some(Color::Rgb(70, 0, 0))),
        LineKind::Context => (" ", None),
    };
    let lineno = match rl.diff_line.kind {
        LineKind::Removed => rl.diff_line.old_lineno,
        _                 => rl.diff_line.new_lineno,
    };
    let lineno_str = match lineno {
        Some(n) => format!("{:>4}", n),
        None    => "    ".to_string(),
    };
    (lineno_str, prefix.to_string(), bg)
}

/// Push one `RichLine` into `out` as a ratatui `Line`.
///
/// When `selected` is true, all spans use `SEL_BG` to indicate line-select
/// highlighting, overriding the normal diff background.
pub(super) fn push_diff_line(rl: &RichLine, selected: bool, out: &mut Vec<Line<'static>>) {
    let (lineno_str, prefix, gutter_bg) = gutter(rl);
    let gutter_style = if selected {
        Style::default().fg(Color::DarkGray).bg(SEL_BG)
    } else {
        match gutter_bg {
            Some(b) => Style::default().fg(Color::DarkGray).bg(b),
            None    => Style::default().fg(Color::DarkGray),
        }
    };

    let mut spans = vec![
        Span::styled(lineno_str, gutter_style),
        Span::styled(" ",        gutter_style),
        Span::styled(prefix,     gutter_style),
    ];

    for seg in &rl.segments {
        let style = if selected {
            Style::default().fg(seg.fg).bg(SEL_BG)
        } else {
            match seg.bg {
                Some(b) => Style::default().fg(seg.fg).bg(b),
                None    => Style::default().fg(seg.fg),
            }
        };
        spans.push(Span::styled(render_safe(&seg.content), style));
    }

    out.push(Line::from(spans));
}

fn push_note_marker(
    note_text: &str,
    note_max_chars: usize,
    out: &mut Vec<Line<'static>>,
) {
    let note_style = Style::default().fg(NOTE_FG).add_modifier(Modifier::ITALIC);
    for (i, line_text) in note_text.lines().enumerate() {
        let prefix = if i == 0 { "  ◎ " } else { "    " };
        let display = if note_max_chars > 0 && line_text.chars().count() > note_max_chars {
            format!("{}…", line_text.chars().take(note_max_chars.saturating_sub(1)).collect::<String>())
        } else {
            line_text.to_string()
        };
        out.push(Line::from(Span::styled(format!("{}{}", prefix, display), note_style)));
    }
}

/// Push the lines of a hunk, folding long context runs into a placeholder.
/// `sel_window` is `Some((lo, hi))` of hunk-line indices to highlight as selected.
/// After each rendered line, `note_fn` is called with the file line number so that
/// callers can inject note markers inline.
pub(super) fn push_diff_lines_folded(
    lines: &[RichLine],
    sel_window: Option<(usize, usize)>,
    mut note_fn: impl FnMut(u32, &mut Vec<Line<'static>>),
    out: &mut Vec<Line<'static>>,
) {
    let fold_style = Style::default().fg(Color::DarkGray);
    let mut ctx_start = 0;
    let mut i = 0;
    while i <= lines.len() {
        let is_context = i < lines.len() && lines[i].diff_line.kind == LineKind::Context;
        if !is_context {
            let ctx_count = i - ctx_start;
            if ctx_count >= FOLD_THRESHOLD {
                out.push(Line::from(Span::styled(
                    format!("  ·· {} lines of context ··", ctx_count), fold_style)));
            } else {
                for (j, rl) in lines.iter().enumerate().take(i).skip(ctx_start) {
                    let selected = sel_window.map(|(lo, hi)| j >= lo && j <= hi).unwrap_or(false);
                    push_diff_line(rl, selected, out);
                    if let Some(n) = rl.diff_line.new_lineno {
                        note_fn(n, out);
                    }
                }
            }
            if i < lines.len() {
                let rl = &lines[i];
                let selected = sel_window.map(|(lo, hi)| i >= lo && i <= hi).unwrap_or(false);
                push_diff_line(rl, selected, out);
                if let Some(n) = rl.diff_line.new_lineno {
                    note_fn(n, out);
                }
            }
            ctx_start = i + 1;
        }
        i += 1;
    }
}

pub(crate) fn build_diff_text(app: &App, note_max_chars: usize) -> Text<'static> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let Some(ref diff) = app.current_rich_diff else {
        lines.push(Line::from(Span::styled("Loading…", Style::default().fg(Color::DarkGray))));
        return Text::from(lines);
    };
    if diff.hunks.is_empty() {
        lines.push(Line::from(Span::styled("No diff content.", Style::default().fg(Color::DarkGray))));
        return Text::from(lines);
    }
    for (hunk_idx, hunk) in diff.hunks.iter().enumerate() {
        push_hunk(app, hunk, hunk_idx, &diff.file.path, note_max_chars, &mut lines);
    }
    Text::from(lines)
}

fn push_hunk(
    app: &App,
    hunk: &RichHunk,
    hunk_idx: usize,
    file_path: &std::path::Path,
    note_max_chars: usize,
    out: &mut Vec<Line<'static>>,
) {
    let is_selected = hunk_idx == app.selected_hunk && app.focused_panel == Panel::DiffView;
    let header_style = if is_selected {
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(MUTED)
    };
    let marker_style = Style::default().fg(ACCENT).add_modifier(Modifier::BOLD);
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

    // Closure that injects line-level note markers after each rendered line.
    let notes = &app.notes;
    let hunk_header = hunk.header.clone();
    let inject_notes = |lineno: u32, out: &mut Vec<Line<'static>>| {
        for note in notes.iter() {
            if note.file == file_path && note.hunk_header == hunk_header
                && note.line_range.as_ref().map(|r| r.end == lineno).unwrap_or(false)
            {
                push_note_marker(&note.note, note_max_chars, out);
            }
        }
    };

    if app.expanded_hunks.contains(&hunk_idx) {
        for (i, rl) in hunk.lines.iter().enumerate() {
            let selected = sel_window.map(|(lo, hi)| i >= lo && i <= hi).unwrap_or(false);
            push_diff_line(rl, selected, out);
            if let Some(n) = rl.diff_line.new_lineno {
                inject_notes(n, out);
            }
        }
    } else {
        push_diff_lines_folded(&hunk.lines, sel_window, |n, out| inject_notes(n, out), out);
    }

    // Whole-hunk notes render at the hunk footer.
    for note in &app.notes {
        if note.file == file_path && note.hunk_header == hunk.header && note.line_range.is_none() {
            push_note_marker(&note.note, note_max_chars, out);
        }
    }
    out.push(Line::raw(""));
}

#[cfg(test)]
mod tests {
    use super::build_diff_text;
    use crate::app::{App, Mode};
    use crate::app::test_helpers::*;
    use crate::diff::{DiffLine, FileStatus, LineKind};
    use crate::segment::{RichDiffFile, RichHunk, RichLine, Segment};
    use crate::diff::ChangedFile;
    use ratatui::style::Color;
    use std::path::PathBuf;

    fn text_str(text: &ratatui::text::Text<'static>) -> String {
        text.lines.iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect::<String>())
            .collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn test_selected_hunk_has_marker() {
        let mut app = app_with_diff(2);
        app.focused_panel = crate::app::Panel::DiffView;
        assert!(text_str(&build_diff_text(&app, 1000)).contains("▶ "));
    }

    #[test]
    fn test_selecting_second_hunk_moves_marker() {
        let mut app = app_with_diff(2);
        app.focused_panel = crate::app::Panel::DiffView;
        app.selected_hunk = 1;
        let content = text_str(&build_diff_text(&app, 1000));
        assert_eq!(content.matches("▶").count(), 1);
        let pos = content.find("▶").unwrap();
        assert!(content[pos + "▶ ".len()..].starts_with("@@ -11,"));
    }

    #[test]
    fn test_non_selected_hunk_indent() {
        let mut app = app_with_diff(2);
        app.focused_panel = crate::app::Panel::DiffView;
        app.selected_hunk = 1;
        let text = build_diff_text(&app, 1000);
        let first = text.lines.iter().find(|l| l.spans.iter().any(|s| s.content.contains("@@"))).unwrap();
        assert_eq!(first.spans[0].content.as_ref(), "  ");
    }

    #[test]
    fn test_loading_when_no_diff() {
        let files = vec![ChangedFile { path: PathBuf::from("src/main.rs"), status: FileStatus::Modified, old_path: None }];
        let app = App::new(files, "main".to_string(), "HEAD".to_string());
        assert!(text_str(&build_diff_text(&app, 1000)).contains("Loading"));
    }

    #[test]
    fn test_comment_not_in_diff_text() {
        let mut app = app_with_diff(1);
        app.mode = Mode::Comment { hunk_idx: 0, input: "secret note".to_string(), cursor: 0, original: None, line_range: None };
        assert!(!text_str(&build_diff_text(&app, 1000)).contains("secret note"));
    }

    #[test]
    fn test_submitted_note_shown_inline() {
        let mut app = app_with_diff(1);
        app.mode = Mode::Comment { hunk_idx: 0, input: "my note".to_string(), cursor: 0, original: None, line_range: None };
        app.submit_comment();
        assert!(text_str(&build_diff_text(&app, 1000)).contains("my note"));
    }

    #[test]
    fn test_inline_note_truncated() {
        let mut app = app_with_diff(1);
        app.mode = Mode::Comment { hunk_idx: 0, input: "a".repeat(60), cursor: 0, original: None, line_range: None };
        app.submit_comment();
        let c = text_str(&build_diff_text(&app, 20));
        assert!(c.contains("…") && !c.contains(&"a".repeat(21)));
    }

    #[test]
    fn test_tab_chars_expanded_in_segment_content() {
        let mut app = app_with_diff(1);
        if let Some(ref mut diff) = app.current_rich_diff {
            diff.hunks[0].lines[0].segments[0].content = "\t\tsome\tcontent".to_string();
        }
        let text = build_diff_text(&app, 1000);
        let full: String = text.lines.iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
            .collect();
        assert!(!full.contains('\t'), "tab chars must not reach the terminal");
        assert!(full.contains("        some    content"), "tabs should expand to 4 spaces each");
    }

    #[test]
    fn test_carriage_returns_stripped() {
        let mut app = app_with_diff(1);
        if let Some(ref mut diff) = app.current_rich_diff {
            diff.hunks[0].lines[0].segments[0].content = "content\r".to_string();
        }
        let text = build_diff_text(&app, 1000);
        let full: String = text.lines.iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
            .collect();
        assert!(!full.contains('\r'), "carriage returns must be stripped before terminal output");
    }

    #[test]
    fn test_multi_segment_line_renders_all_content() {
        // A RichLine with two segments (as syntax highlighting would produce)
        let files = vec![ChangedFile { path: PathBuf::from("src/main.rs"), status: FileStatus::Modified, old_path: None }];
        let mut app = App::new(files.clone(), "main".to_string(), "HEAD".to_string());
        app.focused_panel = crate::app::Panel::DiffView;
        let dl = DiffLine {
            old_lineno: None, new_lineno: Some(1),
            kind: LineKind::Added, content: "fn hello()".to_string(),
        };
        app.current_rich_diff = Some(RichDiffFile {
            file: files[0].clone(),
            hunks: vec![RichHunk {
                header: "@@ -1,1 +1,1 @@".to_string(), old_start: 1, new_start: 1,
                lines: vec![RichLine {
                    diff_line: dl,
                    segments: vec![
                        Segment { content: "fn ".to_string(),    fg: Color::Red,   bg: Some(Color::Rgb(0, 60, 0)) },
                        Segment { content: "hello()".to_string(), fg: Color::White, bg: Some(Color::Rgb(0, 60, 0)) },
                    ],
                }],
            }],
        });
        let content = text_str(&build_diff_text(&app, 1000));
        assert!(content.contains("fn ") && content.contains("hello()"));
    }

    #[test]
    fn test_folded_hunk_placeholder() {
        use crate::app::FOLD_THRESHOLD;
        let files = vec![ChangedFile { path: PathBuf::from("src/main.rs"), status: FileStatus::Modified, old_path: None }];
        let mut app = App::new(files.clone(), "main".to_string(), "HEAD".to_string());
        app.focused_panel = crate::app::Panel::DiffView;

        let mut rich_lines = vec![make_rich_line(DiffLine {
            old_lineno: None, new_lineno: Some(1),
            kind: LineKind::Added, content: "x".to_string(),
        })];
        for i in 0..FOLD_THRESHOLD {
            rich_lines.push(make_rich_line(DiffLine {
                old_lineno: Some(i as u32 + 1), new_lineno: Some(i as u32 + 2),
                kind: LineKind::Context, content: format!("c{}", i),
            }));
        }
        rich_lines.push(make_rich_line(DiffLine {
            old_lineno: Some(10), new_lineno: None,
            kind: LineKind::Removed, content: "y".to_string(),
        }));

        app.current_rich_diff = Some(RichDiffFile {
            file: files[0].clone(),
            hunks: vec![RichHunk {
                header: "@@ -1,10 +1,10 @@".to_string(), old_start: 1, new_start: 1,
                lines: rich_lines,
            }],
        });

        let c = text_str(&build_diff_text(&app, 1000));
        assert!(c.contains("lines of context") && !c.contains("c0"));
    }

    // ── Line-select selection highlight ───────────────────────────────────────

    #[test]
    fn test_line_select_selected_line_uses_sel_bg() {
        use crate::app::Mode;
        use super::SEL_BG;
        let mut app = app_with_diff(1);
        app.focused_panel = crate::app::Panel::DiffView;
        app.mode = Mode::LineSelect { hunk_idx: 0, anchor_line: 0, active_line: 0 };
        let text = build_diff_text(&app, 1000);
        // The selected line (index 0) should have SEL_BG on at least one span.
        let has_sel_bg = text.lines.iter().skip(1).take(1).any(|l| {
            l.spans.iter().any(|s| s.style.bg == Some(SEL_BG))
        });
        assert!(has_sel_bg, "selected line should render with SEL_BG");
    }

    #[test]
    fn test_line_select_non_selected_line_no_sel_bg() {
        use crate::app::Mode;
        use super::SEL_BG;
        let mut app = app_with_diff(1);
        app.focused_panel = crate::app::Panel::DiffView;
        app.mode = Mode::LineSelect { hunk_idx: 0, anchor_line: 0, active_line: 0 };
        let text = build_diff_text(&app, 1000);
        // Line index 2 (third diff line) is Context; should NOT have SEL_BG
        let has_sel_bg = text.lines.iter().skip(3).take(1).any(|l| {
            l.spans.iter().any(|s| s.style.bg == Some(SEL_BG))
        });
        assert!(!has_sel_bg, "non-selected line should not render with SEL_BG");
    }

    // ── Line-level note marker placement ─────────────────────────────────────

    #[test]
    fn test_line_level_note_marker_appears_inline() {
        use crate::app::Mode;
        let mut app = app_with_diff(1);
        app.focused_panel = crate::app::Panel::DiffView;
        // Create a line-level note on hunk 0, line 0 (Added, new_lineno=Some(1))
        app.mode = Mode::LineSelect { hunk_idx: 0, anchor_line: 0, active_line: 0 };
        app.start_comment_for_selection();
        if let Mode::Comment { ref mut input, .. } = app.mode { *input = "inline note".to_string(); }
        app.submit_comment();
        let content = text_str(&build_diff_text(&app, 1000));
        assert!(content.contains("◎"), "note marker should appear");
        assert!(content.contains("inline note"), "note text should appear");
    }

    #[test]
    fn test_whole_hunk_note_marker_at_footer() {
        let mut app = app_with_diff(1);
        app.focused_panel = crate::app::Panel::DiffView;
        app.mode = Mode::Comment { hunk_idx: 0, input: "whole hunk".to_string(), cursor: 0, original: None, line_range: None };
        app.submit_comment();
        let content = text_str(&build_diff_text(&app, 1000));
        assert!(content.contains("whole hunk"), "whole-hunk note should appear");
        assert!(content.contains("◎"), "note marker should appear");
    }

    #[test]
    fn test_line_level_note_on_one_hunk_not_shown_for_other() {
        use crate::app::Mode;
        let mut app = app_with_diff(2);
        app.focused_panel = crate::app::Panel::DiffView;
        // Create line-level note on hunk 0
        app.mode = Mode::LineSelect { hunk_idx: 0, anchor_line: 0, active_line: 0 };
        app.start_comment_for_selection();
        if let Mode::Comment { ref mut input, .. } = app.mode { *input = "only on hunk0".to_string(); }
        app.submit_comment();
        let content = text_str(&build_diff_text(&app, 1000));
        // The note should appear exactly once
        assert_eq!(content.matches("only on hunk0").count(), 1);
    }
}
