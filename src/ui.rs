use std::io;

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame, Terminal,
};

use crate::app::{App, FeedbackNote, Mode, Panel, FOLD_THRESHOLD};
use crate::diff::{ChangedFile, FileStatus, LineKind};
use crate::git::GitBackend;

pub fn run<G: GitBackend>(
    files: Vec<ChangedFile>,
    from: &str,
    to: &str,
    git: &G,
) -> Result<Vec<FeedbackNote>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(files, from.to_string(), to.to_string());
    load_current_file(&mut app, git);

    let result = run_event_loop(&mut terminal, &mut app, git);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result?;
    Ok(app.notes)
}

fn load_current_file<G: GitBackend>(app: &mut App, git: &G) {
    if app.files.is_empty() {
        return;
    }
    let path = app.files[app.selected_file].path.to_string_lossy().to_string();
    let file = app.files[app.selected_file].clone();
    app.current_diff = git
        .file_diff(&app.from, &app.to, &path)
        .ok()
        .map(|raw| crate::diff::parse_diff(&raw, file));
}

fn run_event_loop<G: GitBackend>(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    git: &G,
) -> Result<()> {
    loop {
        terminal.draw(|f| render(f, app))?;

        let Event::Key(key) = event::read()? else {
            continue;
        };

        if key.kind != KeyEventKind::Press {
            continue;
        }

        match app.mode.clone() {
            Mode::Normal => {
                match key.code {
                    KeyCode::Char('q') => break,
                    KeyCode::Tab => {
                        app.focused_panel = match app.focused_panel {
                            Panel::FileList => Panel::DiffView,
                            Panel::DiffView => {
                                if !app.notes.is_empty() { Panel::NotesView } else { Panel::FileList }
                            }
                            Panel::NotesView => Panel::FileList,
                        };
                    }
                    KeyCode::Up => match app.focused_panel {
                        Panel::FileList => app.file_list_up(),
                        Panel::DiffView => app.diff_scroll_up(),
                        Panel::NotesView => app.notes_up(),
                    },
                    KeyCode::Down => match app.focused_panel {
                        Panel::FileList => app.file_list_down(),
                        Panel::DiffView => {
                            let viewport = terminal
                                .size()
                                .map(|r| r.height.saturating_sub(3) as usize)
                                .unwrap_or(20);
                            app.diff_scroll_down(viewport);
                        }
                        Panel::NotesView => app.notes_down(),
                    },
                    KeyCode::Enter => {
                        if app.focused_panel == Panel::FileList {
                            load_current_file(app, git);
                            app.focused_panel = Panel::DiffView;
                        } else if app.focused_panel == Panel::NotesView {
                            jump_to_note(app, git);
                        }
                    }
                    KeyCode::Char('[') => {
                        if app.focused_panel == Panel::DiffView {
                            app.prev_hunk();
                        }
                    }
                    KeyCode::Char(']') => {
                        if app.focused_panel == Panel::DiffView {
                            app.next_hunk();
                        }
                    }
                    KeyCode::Char('c') => {
                        if app.focused_panel == Panel::DiffView {
                            app.start_comment();
                        }
                    }
                    KeyCode::Char(' ') => match app.focused_panel {
                        Panel::DiffView => app.toggle_hunk_fold(),
                        Panel::NotesView => app.toggle_note_expand(),
                        _ => {}
                    },
                    KeyCode::Char('e') => match app.focused_panel {
                        Panel::DiffView => app.edit_note_for_current_hunk(),
                        Panel::NotesView => {
                            jump_to_note(app, git);
                            app.edit_note_for_current_hunk();
                        }
                        _ => {}
                    },
                    KeyCode::Char('d') => match app.focused_panel {
                        Panel::DiffView => app.delete_note_for_current_hunk(),
                        Panel::NotesView => {
                            app.delete_selected_note();
                            if app.notes.is_empty() {
                                app.focused_panel = Panel::DiffView;
                            }
                        }
                        _ => {}
                    },
                    _ => {}
                }

                // Auto-load diff when navigating the file list
                if app.focused_panel == Panel::FileList && app.current_diff.is_none() {
                    load_current_file(app, git);
                }
            }

            Mode::Comment { mut input, hunk_idx, mut cursor } => match key.code {
                // Ctrl+D submits — Ctrl+Enter is indistinguishable from Enter
                // in most terminal emulators so we use Ctrl+D ("done") instead.
                KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    app.submit_comment();
                }
                KeyCode::Enter => {
                    input.insert(cursor, '\n');
                    cursor += 1;
                    app.mode = Mode::Comment { hunk_idx, input, cursor };
                }
                KeyCode::Left => {
                    cursor = cursor_prev(&input, cursor);
                    app.mode = Mode::Comment { hunk_idx, input, cursor };
                }
                KeyCode::Right => {
                    cursor = cursor_next(&input, cursor);
                    app.mode = Mode::Comment { hunk_idx, input, cursor };
                }
                KeyCode::Esc => app.cancel_comment(),
                KeyCode::Backspace => {
                    if cursor > 0 {
                        let prev = cursor_prev(&input, cursor);
                        input.drain(prev..cursor);
                        cursor = prev;
                    }
                    app.mode = Mode::Comment { hunk_idx, input, cursor };
                }
                KeyCode::Char(c) => {
                    input.insert(cursor, c);
                    cursor += c.len_utf8();
                    app.mode = Mode::Comment { hunk_idx, input, cursor };
                }
                _ => {}
            },
        }
    }

    Ok(())
}

// ── Rendering ────────────────────────────────────────────────────────────────

fn render(frame: &mut Frame, app: &App) {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(frame.area());

    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(32), Constraint::Min(0)])
        .split(vertical[0]);

    render_file_list(frame, app, horizontal[0]);
    match app.focused_panel {
        Panel::NotesView => render_notes_panel(frame, app, horizontal[1]),
        _ => render_diff_view(frame, app, horizontal[1]),
    }
    render_status_bar(frame, app, vertical[1]);
}

fn render_file_list(frame: &mut Frame, app: &App, area: Rect) {
    let focused = app.focused_panel == Panel::FileList;
    let border_style = if focused {
        Style::default().fg(Color::Blue)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let items: Vec<ListItem> = app
        .files
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let has_notes = app.notes.iter().any(|n| n.file == f.path);
            let note_marker = if has_notes { " ●" } else { "" };

            let status_color = match f.status {
                FileStatus::Added => Color::Green,
                FileStatus::Modified => Color::Yellow,
                FileStatus::Deleted => Color::Red,
                FileStatus::Renamed => Color::Cyan,
            };
            let base_style = if i == app.selected_file {
                Style::default().bg(Color::Blue).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            let line = Line::from(vec![
                Span::styled(
                    format!("[{}]", f.status.indicator()),
                    base_style.fg(status_color),
                ),
                Span::styled(
                    format!(" {}{}", f.path.display(), note_marker),
                    base_style,
                ),
            ]);
            ListItem::new(line)
        })
        .collect();

    let title = format!(" Files ({}) ", app.files.len());
    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(title),
    );

    let mut state = ListState::default();
    state.select(Some(app.selected_file));
    frame.render_stateful_widget(list, area, &mut state);
}

fn render_diff_view(frame: &mut Frame, app: &App, area: Rect) {
    let focused = app.focused_panel == Panel::DiffView;
    let border_style = if focused {
        Style::default().fg(Color::Blue)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let title = {
        let file_name = app
            .files
            .get(app.selected_file)
            .map(|f| f.path.display().to_string())
            .unwrap_or_else(|| "Diff".to_string());
        match &app.current_diff {
            Some(diff) if !diff.hunks.is_empty() => {
                format!(" {} — {}/{} ", file_name, app.selected_hunk + 1, diff.hunks.len())
            }
            _ => format!(" {} ", file_name),
        }
    };

    let text = build_diff_text(app);

    let para = Paragraph::new(text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border_style)
                .title(title),
        )
        .scroll((app.diff_scroll as u16, 0))
        .wrap(Wrap { trim: false });

    frame.render_widget(para, area);
}

/// Move cursor one character to the left (byte-boundary safe).
fn cursor_prev(s: &str, cursor: usize) -> usize {
    if cursor == 0 { return 0; }
    let mut pos = cursor - 1;
    while pos > 0 && !s.is_char_boundary(pos) { pos -= 1; }
    pos
}

/// Move cursor one character to the right (byte-boundary safe).
fn cursor_next(s: &str, cursor: usize) -> usize {
    if cursor >= s.len() { return s.len(); }
    let mut pos = cursor + 1;
    while pos < s.len() && !s.is_char_boundary(pos) { pos += 1; }
    pos
}

fn push_diff_line(dl: &crate::diff::DiffLine, out: &mut Vec<Line<'static>>) {
    let (prefix, style) = match dl.kind {
        LineKind::Added => ("+", Style::default().fg(Color::Green)),
        LineKind::Removed => ("-", Style::default().fg(Color::Red)),
        LineKind::Context => (" ", Style::default().fg(Color::Gray)),
    };
    let lineno = match dl.kind {
        LineKind::Removed => dl.old_lineno,
        _ => dl.new_lineno,
    };
    let lineno_str = match lineno {
        Some(n) => format!("{:>4}", n),
        None => "    ".to_string(),
    };
    out.push(Line::from(vec![
        Span::styled(lineno_str, Style::default().fg(Color::DarkGray)),
        Span::styled(" ", Style::default().fg(Color::DarkGray)),
        Span::styled(prefix, style),
        Span::styled(dl.content.clone(), style),
    ]));
}

fn push_diff_lines_folded(diff_lines: &[crate::diff::DiffLine], out: &mut Vec<Line<'static>>) {
    let fold_style = Style::default().fg(Color::DarkGray);
    let mut ctx_start = 0;
    let mut i = 0;

    while i <= diff_lines.len() {
        let is_context = i < diff_lines.len() && diff_lines[i].kind == LineKind::Context;

        if !is_context {
            let ctx_count = i - ctx_start;
            if ctx_count >= FOLD_THRESHOLD {
                out.push(Line::from(Span::styled(
                    format!("  ·· {} lines of context ··", ctx_count),
                    fold_style,
                )));
            } else {
                for j in ctx_start..i {
                    push_diff_line(&diff_lines[j], out);
                }
            }
            if i < diff_lines.len() {
                push_diff_line(&diff_lines[i], out);
            }
            ctx_start = i + 1;
        }

        i += 1;
    }
}

pub(crate) fn build_diff_text(app: &App) -> Text<'static> {
    let mut lines: Vec<Line<'static>> = Vec::new();

    let Some(ref diff) = app.current_diff else {
        lines.push(Line::from(Span::styled(
            "Loading…",
            Style::default().fg(Color::DarkGray),
        )));
        return Text::from(lines);
    };

    if diff.hunks.is_empty() {
        lines.push(Line::from(Span::styled(
            "No diff content.",
            Style::default().fg(Color::DarkGray),
        )));
        return Text::from(lines);
    }

    for (hunk_idx, hunk) in diff.hunks.iter().enumerate() {
        let is_selected =
            hunk_idx == app.selected_hunk && app.focused_panel == Panel::DiffView;

        let header_style = if is_selected {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Cyan)
        };
        let marker_style = Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD);
        if is_selected {
            lines.push(Line::from(vec![
                Span::styled("▶ ", marker_style),
                Span::styled(hunk.header.clone(), header_style),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(hunk.header.clone(), header_style),
            ]));
        }

        if app.expanded_hunks.contains(&hunk_idx) {
            for diff_line in &hunk.lines {
                push_diff_line(diff_line, &mut lines);
            }
        } else {
            push_diff_lines_folded(&hunk.lines, &mut lines);
        }

        for note in &app.notes {
            if note.file == diff.file.path && note.hunk_header == hunk.header {
                let note_style = Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::ITALIC);
                for (i, line_text) in note.note.lines().enumerate() {
                    let prefix = if i == 0 { "  ◎ " } else { "    " };
                    lines.push(Line::from(Span::styled(
                        format!("{}{}", prefix, line_text),
                        note_style,
                    )));
                }
            }
        }

        if let Mode::Comment {
            hunk_idx: cidx,
            ref input,
            cursor,
        } = app.mode
        {
            if cidx == hunk_idx {
                let input_lines: Vec<&str> = input.split('\n').collect();
                let pre_cursor = &input[..cursor];
                let cursor_line_idx = pre_cursor.matches('\n').count();
                let cursor_col = pre_cursor.split('\n').last().map(str::len).unwrap_or(0);

                for (i, line_text) in input_lines.iter().enumerate() {
                    let mut spans: Vec<Span<'static>> = if i == 0 {
                        vec![Span::styled("  ▶ ", Style::default().fg(Color::Yellow))]
                    } else {
                        vec![Span::raw("    ")]
                    };

                    if i == cursor_line_idx {
                        let before = line_text[..cursor_col].to_string();
                        let after = line_text[cursor_col..].to_string();
                        spans.push(Span::raw(before));
                        spans.push(Span::styled("█", Style::default().fg(Color::Yellow)));
                        spans.push(Span::raw(after));
                    } else {
                        spans.push(Span::raw(line_text.to_string()));
                    }

                    lines.push(Line::from(spans));
                }
            }
        }

        lines.push(Line::raw(""));
    }

    Text::from(lines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::{ChangedFile, DiffFile, DiffLine, FileStatus, Hunk, LineKind};
    use std::path::PathBuf;

    fn make_app_with_hunks(hunk_count: usize) -> App {
        let files = vec![ChangedFile {
            path: PathBuf::from("src/main.rs"),
            status: FileStatus::Modified,
        }];
        let mut app = App::new(files.clone(), "main".to_string(), "HEAD".to_string());
        app.focused_panel = Panel::DiffView;
        app.current_diff = Some(DiffFile {
            file: files[0].clone(),
            hunks: (0..hunk_count)
                .map(|i| Hunk {
                    header: format!("@@ -{},3 +{},4 @@", i * 10 + 1, i * 10 + 1),
                    old_start: (i * 10 + 1) as u32,
                    new_start: (i * 10 + 1) as u32,
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
                    ],
                })
                .collect(),
        });
        app
    }

    fn text_to_string(text: &Text<'static>) -> String {
        text.lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn test_selected_hunk_has_marker() {
        let app = make_app_with_hunks(2);
        let text = build_diff_text(&app);
        let content = text_to_string(&text);
        assert!(content.contains("▶ "), "selected hunk should have ▶ marker");
    }

    #[test]
    fn test_non_selected_hunk_has_no_marker() {
        let mut app = make_app_with_hunks(2);
        app.selected_hunk = 0;
        let text = build_diff_text(&app);
        let content = text_to_string(&text);
        // Only one ▶ should appear (for hunk 0), not two
        assert_eq!(content.matches("▶").count(), 1);
    }

    #[test]
    fn test_selecting_second_hunk_moves_marker() {
        let mut app = make_app_with_hunks(2);
        app.selected_hunk = 1;
        let text = build_diff_text(&app);
        let content = text_to_string(&text);
        assert_eq!(content.matches("▶").count(), 1);
        // The text immediately after "▶ " should be the second hunk's header
        let marker_pos = content.find("▶").unwrap();
        let after_marker = &content[marker_pos + "▶ ".len()..];
        assert!(
            after_marker.starts_with("@@ -11,"),
            "▶ marker should immediately precede the second hunk header, got: {:?}",
            &after_marker[..after_marker.len().min(20)]
        );
    }

    #[test]
    fn test_non_selected_headers_have_indent() {
        let mut app = make_app_with_hunks(2);
        app.selected_hunk = 1; // select second hunk
        let text = build_diff_text(&app);
        // First line of diff content should be the non-selected hunk — starts with "  " not "▶"
        let first_diff_line = text
            .lines
            .iter()
            .find(|l| l.spans.iter().any(|s| s.content.contains("@@")))
            .unwrap();
        let first_span = &first_diff_line.spans[0];
        assert_eq!(first_span.content.as_ref(), "  ");
    }

    #[test]
    fn test_no_diff_shows_loading() {
        let files = vec![ChangedFile {
            path: PathBuf::from("src/main.rs"),
            status: FileStatus::Modified,
        }];
        let app = App::new(files, "main".to_string(), "HEAD".to_string());
        let text = build_diff_text(&app);
        let content = text_to_string(&text);
        assert!(content.contains("Loading"));
    }

    // ── Multi-line comment rendering ──────────────────────────────────────────

    #[test]
    fn test_multiline_comment_renders_multiple_lines() {
        let mut app = make_app_with_hunks(1);
        app.mode = Mode::Comment {
            hunk_idx: 0,
            input: "line one\nline two\nline three".to_string(),
            cursor: 0,
        };
        let text = build_diff_text(&app);
        let content = text_to_string(&text);
        assert!(content.contains("line one"));
        assert!(content.contains("line two"));
        assert!(content.contains("line three"));
    }

    #[test]
    fn test_multiline_comment_cursor_on_last_line_only() {
        let mut app = make_app_with_hunks(1);
        let input = "line one\nline two".to_string();
        let cursor = input.len(); // cursor at end — after "line two"
        app.mode = Mode::Comment { hunk_idx: 0, input, cursor };
        let text = build_diff_text(&app);
        // Cursor █ should appear exactly once, on the last visual line
        let content = text_to_string(&text);
        assert_eq!(content.matches("█").count(), 1);
        let cursor_pos = content.find("█").unwrap();
        let line_two_pos = content.find("line two").unwrap();
        assert!(cursor_pos > line_two_pos, "cursor should be after 'line two'");
    }

    #[test]
    fn test_multiline_comment_first_line_has_marker() {
        let mut app = make_app_with_hunks(1);
        app.mode = Mode::Comment {
            hunk_idx: 0,
            input: "first\nsecond".to_string(),
            cursor: 0,
        };
        let text = build_diff_text(&app);
        let content = text_to_string(&text);
        // ▶ marker should precede the first line of the comment
        let marker_pos = content.find("▶").unwrap();
        let first_pos = content.find("first").unwrap();
        assert!(marker_pos < first_pos);
    }

    // ── Context folding rendering ─────────────────────────────────────────────

    fn make_app_with_long_context_hunk() -> App {
        use crate::diff::{DiffFile, DiffLine, FileStatus, Hunk, LineKind};
        let files = vec![ChangedFile {
            path: PathBuf::from("src/main.rs"),
            status: FileStatus::Modified,
        }];
        let mut app = App::new(files.clone(), "main".to_string(), "HEAD".to_string());
        app.focused_panel = Panel::DiffView;
        // One hunk: 1 added + FOLD_THRESHOLD context lines + 1 removed
        let mut lines = vec![DiffLine {
            old_lineno: None, new_lineno: Some(1),
            kind: LineKind::Added, content: "added".to_string(),
        }];
        for i in 0..crate::app::FOLD_THRESHOLD {
            lines.push(DiffLine {
                old_lineno: Some(i as u32 + 1), new_lineno: Some(i as u32 + 2),
                kind: LineKind::Context, content: format!("ctx {}", i),
            });
        }
        lines.push(DiffLine {
            old_lineno: Some(10), new_lineno: None,
            kind: LineKind::Removed, content: "removed".to_string(),
        });
        app.current_diff = Some(DiffFile {
            file: files[0].clone(),
            hunks: vec![Hunk {
                header: "@@ -1,10 +1,10 @@".to_string(),
                old_start: 1, new_start: 1, lines,
            }],
        });
        app
    }

    #[test]
    fn test_folded_hunk_shows_placeholder() {
        let app = make_app_with_long_context_hunk();
        let text = build_diff_text(&app);
        let content = text_to_string(&text);
        assert!(content.contains("·· "), "folded context should show placeholder");
        assert!(content.contains("lines of context"));
    }

    #[test]
    fn test_folded_hunk_hides_individual_context_lines() {
        let app = make_app_with_long_context_hunk();
        let text = build_diff_text(&app);
        let content = text_to_string(&text);
        // Individual context line content should not appear when folded
        assert!(!content.contains("ctx 0"), "individual context lines should be hidden when folded");
    }

    #[test]
    fn test_expanded_hunk_shows_context_lines() {
        let mut app = make_app_with_long_context_hunk();
        app.expanded_hunks.insert(0);
        let text = build_diff_text(&app);
        let content = text_to_string(&text);
        assert!(content.contains("ctx 0"), "expanded hunk should show all context lines");
        assert!(!content.contains("lines of context"), "expanded hunk should not show placeholder");
    }

    #[test]
    fn test_folded_hunk_still_shows_added_and_removed() {
        let app = make_app_with_long_context_hunk();
        let text = build_diff_text(&app);
        let content = text_to_string(&text);
        assert!(content.contains("added"));
        assert!(content.contains("removed"));
    }
}

fn jump_to_note<G: GitBackend>(app: &mut App, git: &G) {
    let Some(file_idx) = app.selected_note_file_idx() else { return };
    let target_header = app.notes[app.selected_note].hunk_header.clone();
    app.select_file(file_idx);
    load_current_file(app, git);
    if let Some(hunk_idx) = app
        .current_diff
        .as_ref()
        .and_then(|d| d.hunks.iter().position(|h| h.header == target_header))
    {
        app.selected_hunk = hunk_idx;
        app.scroll_to_selected_hunk();
    }
    app.focused_panel = Panel::DiffView;
}

fn render_notes_panel(frame: &mut Frame, app: &App, area: Rect) {
    const TRUNCATE_LEN: usize = 55;

    let title = format!(" Notes ({}) ", app.notes.len());
    let mut lines: Vec<Line<'static>> = Vec::new();

    if app.notes.is_empty() {
        lines.push(Line::from(Span::styled(
            "No notes yet.",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        for (i, note) in app.notes.iter().enumerate() {
            let is_selected = i == app.selected_note;
            let is_expanded = app.expanded_notes.contains(&i);

            let header_style = if is_selected {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Cyan)
            };
            let marker = if is_selected { "▶ " } else { "  " };
            lines.push(Line::from(Span::styled(
                format!("{}{} · {}", marker, note.file.display(), note.hunk_header),
                header_style,
            )));

            let note_style = Style::default().fg(Color::White);
            if is_expanded {
                for line_text in note.note.lines() {
                    lines.push(Line::from(vec![
                        Span::raw("    "),
                        Span::styled(line_text.to_string(), note_style),
                    ]));
                }
            } else {
                let first_line = note.note.lines().next().unwrap_or("");
                let truncated = if first_line.len() > TRUNCATE_LEN {
                    format!("{}…", &first_line[..TRUNCATE_LEN])
                } else {
                    first_line.to_string()
                };
                lines.push(Line::from(vec![
                    Span::raw("    "),
                    Span::styled(truncated, note_style),
                ]));
            }

            lines.push(Line::raw(""));
        }
    }

    let para = Paragraph::new(Text::from(lines)).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Blue))
            .title(title),
    );
    frame.render_widget(para, area);
}

fn render_status_bar(frame: &mut Frame, app: &App, area: Rect) {
    let text = match app.mode {
        Mode::Comment { .. } => " Ctrl+D: submit  Enter: newline  Esc: cancel".to_string(),
        Mode::Normal => match app.focused_panel {
            Panel::FileList => {
                " Tab: diff view  ↑↓: navigate  Enter: open  q: quit".to_string()
            }
            Panel::NotesView => {
                " Tab: back  ↑↓: navigate  Enter: jump  Space: expand  e: edit  d: delete  q: quit".to_string()
            }
            Panel::DiffView => {
                let note_count = app.notes.len();
                let notes_str = if note_count == 1 {
                    "  ●1 note".to_string()
                } else if note_count > 1 {
                    format!("  ●{} notes", note_count)
                } else {
                    String::new()
                };
                let note_actions = if app.current_hunk_has_note() {
                    "  e: edit  d: delete"
                } else {
                    "  c: comment"
                };
                let fold_hint = if app.selected_hunk_is_foldable() {
                    if app.expanded_hunks.contains(&app.selected_hunk) {
                        "  Space: fold"
                    } else {
                        "  Space: expand"
                    }
                } else {
                    ""
                };
                format!(
                    " Tab: file list  ↑↓: scroll  []: hunk{}{}  q: quit{}",
                    note_actions,
                    fold_hint,
                    notes_str,
                )
            }
        },
    };

    let style = Style::default().bg(Color::Blue).fg(Color::White);
    let bar = Paragraph::new(text).style(style);
    frame.render_widget(bar, area);
}
