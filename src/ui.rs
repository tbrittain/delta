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
use crate::diff::{ChangedFile, LineKind};
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
                        Panel::DiffView => app.diff_scroll_down(),
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
            let label = format!(
                "[{}] {}{}",
                f.status.indicator(),
                f.path.display(),
                note_marker
            );
            let style = if i == app.selected_file {
                Style::default().bg(Color::Blue).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(label).style(style)
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

    let title = app
        .files
        .get(app.selected_file)
        .map(|f| format!(" {} ", f.path.display()))
        .unwrap_or_else(|| " Diff ".to_string());

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
        lines.push(Line::from(Span::styled(
            hunk.header.clone(),
            header_style,
        )));

        for diff_line in &hunk.lines {
            let (prefix, style) = match diff_line.kind {
                LineKind::Added => ("+", Style::default().fg(Color::Green)),
                LineKind::Removed => ("-", Style::default().fg(Color::Red)),
                LineKind::Context => (" ", Style::default().fg(Color::Gray)),
            };
            lines.push(Line::from(vec![
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
