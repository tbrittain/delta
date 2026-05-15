use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
};

use crate::app::{App, Panel, FOLD_THRESHOLD};
use crate::diff::LineKind;
use crate::highlight::HighlightedSpan;
use super::{ACCENT, MUTED, NOTE_FG};

/// Expand tabs to 4 spaces and strip carriage returns so that control
/// characters in source-code content do not corrupt terminal layout.
/// Tabs are terminal-interpreted (jump to tab stops), not fixed-width;
/// \r moves the cursor to column 0 — both produce garbled rendering.
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

pub(super) fn push_diff_line(
    dl: &crate::diff::DiffLine,
    highlights: Option<&[HighlightedSpan]>,
    out: &mut Vec<Line<'static>>,
) {
    let (prefix, bg) = match dl.kind {
        LineKind::Added   => ("+", Some(Color::Rgb(0, 60, 0))),
        LineKind::Removed => ("-", Some(Color::Rgb(70, 0, 0))),
        LineKind::Context => (" ", None),
    };
    let lineno = match dl.kind { LineKind::Removed => dl.old_lineno, _ => dl.new_lineno };
    let lineno_str = match lineno { Some(n) => format!("{:>4}", n), None => "    ".to_string() };
    let gutter_style = match bg {
        Some(b) => Style::default().fg(Color::DarkGray).bg(b),
        None    => Style::default().fg(Color::DarkGray),
    };
    let mut spans = vec![
        Span::styled(lineno_str, gutter_style),
        Span::styled(" ",        gutter_style),
        Span::styled(prefix,     gutter_style),
    ];
    match highlights {
        Some(hl) if !hl.is_empty() => {
            for token in hl {
                let style = match bg {
                    Some(b) => Style::default().fg(token.fg).bg(b),
                    None    => Style::default().fg(token.fg),
                };
                spans.push(Span::styled(render_safe(&token.content), style));
            }
        }
        _ => {
            let fallback_fg = match dl.kind {
                LineKind::Added   => Color::Green,
                LineKind::Removed => Color::Red,
                LineKind::Context => Color::Gray,
            };
            let style = match bg {
                Some(b) => Style::default().fg(fallback_fg).bg(b),
                None    => Style::default().fg(fallback_fg),
            };
            spans.push(Span::styled(render_safe(&dl.content), style));
        }
    }
    out.push(Line::from(spans));
}

pub(super) fn push_diff_lines_folded(
    diff_lines: &[crate::diff::DiffLine],
    line_highlights: Option<&[Vec<HighlightedSpan>]>,
    out: &mut Vec<Line<'static>>,
) {
    let fold_style = Style::default().fg(Color::DarkGray);
    let mut ctx_start = 0;
    let mut i = 0;
    while i <= diff_lines.len() {
        let is_context = i < diff_lines.len() && diff_lines[i].kind == LineKind::Context;
        if !is_context {
            let ctx_count = i - ctx_start;
            if ctx_count >= FOLD_THRESHOLD {
                out.push(Line::from(Span::styled(
                    format!("  ·· {} lines of context ··", ctx_count), fold_style)));
            } else {
                for j in ctx_start..i {
                    let hl = line_highlights.and_then(|h| h.get(j)).map(|v| v.as_slice());
                    push_diff_line(&diff_lines[j], hl, out);
                }
            }
            if i < diff_lines.len() {
                let hl = line_highlights.and_then(|h| h.get(i)).map(|v| v.as_slice());
                push_diff_line(&diff_lines[i], hl, out);
            }
            ctx_start = i + 1;
        }
        i += 1;
    }
}

pub(crate) fn build_diff_text(app: &App, note_max_chars: usize) -> Text<'static> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let Some(ref diff) = app.current_diff else {
        lines.push(Line::from(Span::styled("Loading…", Style::default().fg(Color::DarkGray))));
        return Text::from(lines);
    };
    if diff.hunks.is_empty() {
        lines.push(Line::from(Span::styled("No diff content.", Style::default().fg(Color::DarkGray))));
        return Text::from(lines);
    }
    for (hunk_idx, hunk) in diff.hunks.iter().enumerate() {
        let is_selected = hunk_idx == app.selected_hunk && app.focused_panel == Panel::DiffView;
        let header_style = if is_selected {
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
        } else { Style::default().fg(MUTED) };
        let marker_style = Style::default().fg(ACCENT).add_modifier(Modifier::BOLD);
        if is_selected {
            lines.push(Line::from(vec![Span::styled("▶ ", marker_style), Span::styled(hunk.header.clone(), header_style)]));
        } else {
            lines.push(Line::from(vec![Span::raw("  "), Span::styled(hunk.header.clone(), header_style)]));
        }
        let hunk_hl = app.current_highlights.as_ref().and_then(|h| h.get(hunk_idx));
        if app.expanded_hunks.contains(&hunk_idx) {
            for (line_idx, diff_line) in hunk.lines.iter().enumerate() {
                let hl = hunk_hl.and_then(|h| h.get(line_idx)).map(|v| v.as_slice());
                push_diff_line(diff_line, hl, &mut lines);
            }
        } else {
            push_diff_lines_folded(&hunk.lines, hunk_hl.map(|h| h.as_slice()), &mut lines);
        }
        for note in &app.notes {
            if note.file == diff.file.path && note.hunk_header == hunk.header {
                let note_style = Style::default().fg(NOTE_FG).add_modifier(Modifier::ITALIC);
                for (i, line_text) in note.note.lines().enumerate() {
                    let prefix = if i == 0 { "  ◎ " } else { "    " };
                    let display = if note_max_chars > 0 && line_text.chars().count() > note_max_chars {
                        format!("{}…", line_text.chars().take(note_max_chars.saturating_sub(1)).collect::<String>())
                    } else { line_text.to_string() };
                    lines.push(Line::from(Span::styled(format!("{}{}", prefix, display), note_style)));
                }
            }
        }
        lines.push(Line::raw(""));
    }
    Text::from(lines)
}

#[cfg(test)]
mod tests {
    use super::build_diff_text;
    use crate::app::{App, Mode};
    use crate::diff::{ChangedFile, DiffFile, DiffLine, FileStatus, Hunk, LineKind};
    use std::path::PathBuf;

    fn make_app(hunk_count: usize) -> App {
        let files = vec![ChangedFile { path: PathBuf::from("src/main.rs"), status: FileStatus::Modified }];
        let mut app = App::new(files.clone(), "main".to_string(), "HEAD".to_string());
        app.focused_panel = crate::app::Panel::DiffView;
        app.current_diff = Some(DiffFile {
            file: files[0].clone(),
            hunks: (0..hunk_count).map(|i| Hunk {
                header: format!("@@ -{},3 +{},4 @@", i * 10 + 1, i * 10 + 1),
                old_start: (i * 10 + 1) as u32, new_start: (i * 10 + 1) as u32,
                lines: vec![
                    DiffLine { old_lineno: None,    new_lineno: Some(1), kind: LineKind::Added,   content: "new line".to_string() },
                    DiffLine { old_lineno: Some(1), new_lineno: None,    kind: LineKind::Removed, content: "old line".to_string() },
                ],
            }).collect(),
        });
        app
    }

    fn text_str(text: &ratatui::text::Text<'static>) -> String {
        text.lines.iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect::<String>())
            .collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn test_selected_hunk_has_marker() {
        assert!(text_str(&build_diff_text(&make_app(2), 1000)).contains("▶ "));
    }

    #[test]
    fn test_non_selected_hunk_indent() {
        let mut app = make_app(2); app.selected_hunk = 1;
        let text = build_diff_text(&app, 1000);
        let first = text.lines.iter().find(|l| l.spans.iter().any(|s| s.content.contains("@@"))).unwrap();
        assert_eq!(first.spans[0].content.as_ref(), "  ");
    }

    #[test]
    fn test_loading_when_no_diff() {
        let files = vec![ChangedFile { path: PathBuf::from("src/main.rs"), status: FileStatus::Modified }];
        let app = App::new(files, "main".to_string(), "HEAD".to_string());
        assert!(text_str(&build_diff_text(&app, 1000)).contains("Loading"));
    }

    #[test]
    fn test_comment_not_in_diff_text() {
        let mut app = make_app(1);
        app.mode = Mode::Comment { hunk_idx: 0, input: "secret note".to_string(), cursor: 0, original: None };
        assert!(!text_str(&build_diff_text(&app, 1000)).contains("secret note"));
    }

    #[test]
    fn test_submitted_note_shown_inline() {
        let mut app = make_app(1);
        app.mode = Mode::Comment { hunk_idx: 0, input: "my note".to_string(), cursor: 0, original: None };
        app.submit_comment();
        assert!(text_str(&build_diff_text(&app, 1000)).contains("my note"));
    }

    #[test]
    fn test_inline_note_truncated() {
        let mut app = make_app(1);
        app.mode = Mode::Comment { hunk_idx: 0, input: "a".repeat(60), cursor: 0, original: None };
        app.submit_comment();
        let c = text_str(&build_diff_text(&app, 20));
        assert!(c.contains("…") && !c.contains(&"a".repeat(21)));
    }

    #[test]
    fn test_tab_chars_expanded_in_fallback_rendering() {
        // Tab chars in dl.content must become spaces — literal \t causes terminal
        // tab-stop jumps that produce garbled layout on real terminals.
        let mut app = make_app(1);
        if let Some(ref mut diff) = app.current_diff {
            diff.hunks[0].lines[0].content = "\t\tsome\tcontent".to_string();
        }
        let text = build_diff_text(&app, 1000);
        let full: String = text.lines.iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
            .collect();
        assert!(!full.contains('\t'), "tab chars must not reach the terminal");
        assert!(full.contains("        some    content"), "tabs should expand to 4 spaces each");
    }

    #[test]
    fn test_tab_chars_expanded_in_highlighted_rendering() {
        use crate::highlight::HighlightedSpan;
        use ratatui::style::Color;
        let mut app = make_app(1);
        app.current_highlights = Some(vec![vec![vec![
            HighlightedSpan { content: "\t\tsome\tcontent".to_string(), fg: Color::White },
        ]]]);
        let text = build_diff_text(&app, 1000);
        let full: String = text.lines.iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
            .collect();
        assert!(!full.contains('\t'), "tab chars in highlights must not reach the terminal");
    }

    #[test]
    fn test_carriage_returns_stripped() {
        let mut app = make_app(1);
        if let Some(ref mut diff) = app.current_diff {
            diff.hunks[0].lines[0].content = "content\r".to_string();
        }
        let text = build_diff_text(&app, 1000);
        let full: String = text.lines.iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
            .collect();
        assert!(!full.contains('\r'), "carriage returns must be stripped before terminal output");
    }

    #[test]
    fn test_folded_hunk_placeholder() {
        use crate::app::FOLD_THRESHOLD;
        let files = vec![ChangedFile { path: PathBuf::from("src/main.rs"), status: FileStatus::Modified }];
        let mut app = App::new(files.clone(), "main".to_string(), "HEAD".to_string());
        app.focused_panel = crate::app::Panel::DiffView;
        let mut diff_lines = vec![DiffLine { old_lineno: None, new_lineno: Some(1), kind: LineKind::Added, content: "x".to_string() }];
        for i in 0..FOLD_THRESHOLD {
            diff_lines.push(DiffLine { old_lineno: Some(i as u32 + 1), new_lineno: Some(i as u32 + 2), kind: LineKind::Context, content: format!("c{}", i) });
        }
        diff_lines.push(DiffLine { old_lineno: Some(10), new_lineno: None, kind: LineKind::Removed, content: "y".to_string() });
        app.current_diff = Some(DiffFile {
            file: files[0].clone(),
            hunks: vec![Hunk { header: "@@ -1,10 +1,10 @@".to_string(), old_start: 1, new_start: 1, lines: diff_lines }],
        });
        let c = text_str(&build_diff_text(&app, 1000));
        assert!(c.contains("lines of context") && !c.contains("c0"));
    }
}
