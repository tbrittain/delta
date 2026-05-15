use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
};

use crate::app::{App, Panel, FOLD_THRESHOLD};
use crate::diff::LineKind;
use crate::segment::{RichHunk, RichLine};
use super::{ACCENT, MUTED, NOTE_FG};

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
/// The gutter (line number + sigil) always uses the line-level background.
/// Content segments carry their own fg and bg from the enrichment pipeline.
pub(super) fn push_diff_line(rl: &RichLine, out: &mut Vec<Line<'static>>) {
    let (lineno_str, prefix, gutter_bg) = gutter(rl);
    let gutter_style = match gutter_bg {
        Some(b) => Style::default().fg(Color::DarkGray).bg(b),
        None    => Style::default().fg(Color::DarkGray),
    };

    let mut spans = vec![
        Span::styled(lineno_str, gutter_style),
        Span::styled(" ",        gutter_style),
        Span::styled(prefix,     gutter_style),
    ];

    for seg in &rl.segments {
        let style = match seg.bg {
            Some(b) => Style::default().fg(seg.fg).bg(b),
            None    => Style::default().fg(seg.fg),
        };
        spans.push(Span::styled(render_safe(&seg.content), style));
    }

    out.push(Line::from(spans));
}

/// Push the lines of a hunk, folding long context runs into a placeholder.
pub(super) fn push_diff_lines_folded(lines: &[RichLine], out: &mut Vec<Line<'static>>) {
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
                for rl in lines.iter().take(i).skip(ctx_start) {
                    push_diff_line(rl, out);
                }
            }
            if i < lines.len() {
                push_diff_line(&lines[i], out);
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

    if app.expanded_hunks.contains(&hunk_idx) {
        for rl in &hunk.lines {
            push_diff_line(rl, out);
        }
    } else {
        push_diff_lines_folded(&hunk.lines, out);
    }

    for note in &app.notes {
        if note.file == file_path && note.hunk_header == hunk.header {
            let note_style = Style::default().fg(NOTE_FG).add_modifier(Modifier::ITALIC);
            for (i, line_text) in note.note.lines().enumerate() {
                let prefix = if i == 0 { "  ◎ " } else { "    " };
                let display = if note_max_chars > 0 && line_text.chars().count() > note_max_chars {
                    format!("{}…", line_text.chars().take(note_max_chars.saturating_sub(1)).collect::<String>())
                } else {
                    line_text.to_string()
                };
                out.push(Line::from(Span::styled(format!("{}{}", prefix, display), note_style)));
            }
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
        let files = vec![ChangedFile { path: PathBuf::from("src/main.rs"), status: FileStatus::Modified }];
        let app = App::new(files, "main".to_string(), "HEAD".to_string());
        assert!(text_str(&build_diff_text(&app, 1000)).contains("Loading"));
    }

    #[test]
    fn test_comment_not_in_diff_text() {
        let mut app = app_with_diff(1);
        app.mode = Mode::Comment { hunk_idx: 0, input: "secret note".to_string(), cursor: 0, original: None };
        assert!(!text_str(&build_diff_text(&app, 1000)).contains("secret note"));
    }

    #[test]
    fn test_submitted_note_shown_inline() {
        let mut app = app_with_diff(1);
        app.mode = Mode::Comment { hunk_idx: 0, input: "my note".to_string(), cursor: 0, original: None };
        app.submit_comment();
        assert!(text_str(&build_diff_text(&app, 1000)).contains("my note"));
    }

    #[test]
    fn test_inline_note_truncated() {
        let mut app = app_with_diff(1);
        app.mode = Mode::Comment { hunk_idx: 0, input: "a".repeat(60), cursor: 0, original: None };
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
        let files = vec![ChangedFile { path: PathBuf::from("src/main.rs"), status: FileStatus::Modified }];
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
        let files = vec![ChangedFile { path: PathBuf::from("src/main.rs"), status: FileStatus::Modified }];
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
}
