use std::io;

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame, Terminal,
};

use crate::app::{App, FeedbackNote, Mode, Panel};
use crate::diff::{ChangedFile, FileStatus, LineKind};
use crate::git::GitBackend;

pub fn run<G: GitBackend>(
    files: Vec<ChangedFile>,
    base: &str,
    git: &G,
) -> Result<Vec<FeedbackNote>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(files, base.to_string());
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
        .file_diff(&app.base, &path)
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
                            Panel::DiffView => Panel::FileList,
                        };
                    }
                    KeyCode::Up => match app.focused_panel {
                        Panel::FileList => app.file_list_up(),
                        Panel::DiffView => app.diff_scroll_up(),
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
                    },
                    KeyCode::Enter => {
                        if app.focused_panel == Panel::FileList {
                            load_current_file(app, git);
                            app.focused_panel = Panel::DiffView;
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
                    _ => {}
                }

                // Auto-load diff when navigating the file list
                if app.focused_panel == Panel::FileList && app.current_diff.is_none() {
                    load_current_file(app, git);
                }
            }

            Mode::Comment { mut input, hunk_idx } => match key.code {
                KeyCode::Enter => app.submit_comment(),
                KeyCode::Esc => app.cancel_comment(),
                KeyCode::Backspace => {
                    input.pop();
                    app.mode = Mode::Comment { hunk_idx, input };
                }
                KeyCode::Char(c) => {
                    input.push(c);
                    app.mode = Mode::Comment { hunk_idx, input };
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
    render_diff_view(frame, app, horizontal[1]);
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
        .scroll((app.diff_scroll as u16, 0));

    frame.render_widget(para, area);
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

        for diff_line in &hunk.lines {
            let (prefix, style) = match diff_line.kind {
                LineKind::Added => ("+", Style::default().fg(Color::Green)),
                LineKind::Removed => ("-", Style::default().fg(Color::Red)),
                LineKind::Context => (" ", Style::default().fg(Color::Gray)),
            };
            // Show the line number most relevant for this line kind:
            // added/context → new file line number; removed → old file line number.
            let lineno = match diff_line.kind {
                LineKind::Removed => diff_line.old_lineno,
                _ => diff_line.new_lineno,
            };
            let lineno_str = match lineno {
                Some(n) => format!("{:>4}", n),
                None => "    ".to_string(),
            };
            lines.push(Line::from(vec![
                Span::styled(lineno_str, Style::default().fg(Color::DarkGray)),
                Span::styled(" ", Style::default().fg(Color::DarkGray)),
                Span::styled(prefix, style),
                Span::styled(diff_line.content.clone(), style),
            ]));
        }

        for note in &app.notes {
            if note.file == diff.file.path && note.hunk_header == hunk.header {
                lines.push(Line::from(Span::styled(
                    format!("  ◎ {}", note.note),
                    Style::default()
                        .fg(Color::Magenta)
                        .add_modifier(Modifier::ITALIC),
                )));
            }
        }

        if let Mode::Comment {
            hunk_idx: cidx,
            ref input,
        } = app.mode
        {
            if cidx == hunk_idx {
                lines.push(Line::from(vec![
                    Span::styled("  ▶ ", Style::default().fg(Color::Yellow)),
                    Span::raw(input.clone()),
                    Span::styled("█", Style::default().fg(Color::Yellow)),
                ]));
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
        let mut app = App::new(files.clone(), "main".to_string());
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
        let app = App::new(files, "main".to_string());
        let text = build_diff_text(&app);
        let content = text_to_string(&text);
        assert!(content.contains("Loading"));
    }
}

fn render_status_bar(frame: &mut Frame, app: &App, area: Rect) {
    let text = match app.mode {
        Mode::Comment { .. } => " Enter: submit  Esc: cancel".to_string(),
        Mode::Normal => match app.focused_panel {
            Panel::FileList => {
                " Tab: diff view  ↑↓: navigate  Enter: open  q: quit".to_string()
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
                format!(
                    " Tab: file list  ↑↓: scroll  []: hunk  c: comment  q: quit{}",
                    notes_str
                )
            }
        },
    };

    let style = Style::default().bg(Color::Blue).fg(Color::White);
    let bar = Paragraph::new(text).style(style);
    frame.render_widget(bar, area);
}
