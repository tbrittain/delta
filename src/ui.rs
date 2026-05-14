use std::io;

use anyhow::Result;
use crossterm::{
    cursor::SetCursorStyle,
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
    Frame, Terminal,
};

use crate::app::{App, FeedbackNote, Mode, Panel, FOLD_THRESHOLD, delete_selection, selected_range};
use crate::diff::{ChangedFile, FileStatus, LineKind};
use crate::git::GitBackend;
use crate::highlight::HighlightedSpan;

// ── Palette ───────────────────────────────────────────────────────────────────
const ACCENT: Color = Color::Cyan;
const MUTED: Color = Color::Rgb(100, 110, 130);
const NOTE_FG: Color = Color::Rgb(100, 150, 210);
const SEL_BG: Color = Color::Rgb(60, 80, 140);

// ── Clipboard I/O (boundary layer — not unit tested) ─────────────────────────

fn clipboard_get() -> Option<String> {
    arboard::Clipboard::new().ok()?.get_text().ok()
}

fn clipboard_set(text: String) {
    if let Ok(mut cb) = arboard::Clipboard::new() {
        let _ = cb.set_text(text);
    }
}

pub fn run<G: GitBackend>(
    files: Vec<ChangedFile>,
    from: &str,
    to: &str,
    git: &G,
) -> Result<Vec<FeedbackNote>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    // Use a blinking bar (I-beam) cursor — it overlays text without displacing it.
    execute!(stdout, SetCursorStyle::BlinkingBar)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(files, from.to_string(), to.to_string());
    load_current_file(&mut app, git);

    let result = run_event_loop(&mut terminal, &mut app, git);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), SetCursorStyle::DefaultUserShape, LeaveAlternateScreen)?;
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
    log::debug!("[ui] load_current_file: path={:?}", path);
    let result = git.file_diff(&app.from, &app.to, &path);
    log::debug!(
        "[ui] load_current_file: file_diff result={}",
        match &result { Ok(s) => format!("Ok({} bytes)", s.len()), Err(e) => format!("Err({e})") }
    );
    app.current_diff = result.ok().map(|raw| crate::diff::parse_diff(&raw, file));
    app.current_highlights = app.current_diff.as_ref().map(|d| app.highlighter.highlight_diff(d));
    log::debug!(
        "[ui] load_current_file: current_diff={}",
        match &app.current_diff {
            Some(d) => format!("Some({} hunks)", d.hunks.len()),
            None => "None".into(),
        }
    );
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
                    KeyCode::BackTab => {
                        app.focused_panel = match app.focused_panel {
                            Panel::FileList => {
                                if !app.notes.is_empty() { Panel::NotesView } else { Panel::DiffView }
                            }
                            Panel::DiffView => Panel::FileList,
                            Panel::NotesView => Panel::DiffView,
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
                        if app.focused_panel == Panel::DiffView { app.prev_hunk(); }
                    }
                    KeyCode::Char(']') => {
                        if app.focused_panel == Panel::DiffView { app.next_hunk(); }
                    }
                    KeyCode::Char('c') => {
                        if app.focused_panel == Panel::DiffView { app.start_comment(); }
                    }
                    KeyCode::Char(' ') => match app.focused_panel {
                        Panel::DiffView   => app.toggle_hunk_fold(),
                        Panel::NotesView  => app.toggle_note_expand(),
                        _ => {}
                    },
                    KeyCode::Char('e') => match app.focused_panel {
                        Panel::DiffView => { app.edit_note_for_current_hunk(); }
                        Panel::NotesView => { jump_to_note(app, git); app.edit_note_for_current_hunk(); }
                        _ => {}
                    },
                    KeyCode::Char('d') => match app.focused_panel {
                        Panel::DiffView  => app.delete_note_for_current_hunk(),
                        Panel::NotesView => {
                            app.delete_selected_note();
                            if app.notes.is_empty() { app.focused_panel = Panel::DiffView; }
                        }
                        _ => {}
                    },
                    _ => {}
                }

                if app.focused_panel == Panel::FileList && app.current_diff.is_none() {
                    load_current_file(app, git);
                }
            }

            Mode::Comment { mut input, hunk_idx, mut cursor, original } => {
                let shift = key.modifiers.contains(KeyModifiers::SHIFT);
                let ctrl  = key.modifiers.contains(KeyModifiers::CONTROL);

                macro_rules! extend { () => {
                    if app.comment_anchor.is_none() { app.comment_anchor = Some(cursor); }
                }}
                macro_rules! clear_sel { () => { app.comment_anchor = None; }}

                let consumed = match key.code {
                    KeyCode::Char('s') if ctrl => { app.submit_comment(); true }
                    KeyCode::Esc => { app.cancel_comment(); true }

                    KeyCode::Char('a') if ctrl => {
                        app.comment_anchor = Some(0);
                        cursor = input.len();
                        app.mode = Mode::Comment { hunk_idx, input, cursor, original };
                        true
                    }
                    KeyCode::Char('c') if ctrl => {
                        if let Some((s, e)) = selected_range(cursor, app.comment_anchor) {
                            clipboard_set(input[s..e].to_string());
                        }
                        true
                    }
                    KeyCode::Char('x') if ctrl => {
                        if let Some((new_input, new_cursor)) =
                            delete_selection(&input, cursor, app.comment_anchor)
                        {
                            if let Some((s, _)) = selected_range(cursor, app.comment_anchor) {
                                let end = s + cursor.max(app.comment_anchor.unwrap_or(cursor))
                                    - cursor.min(app.comment_anchor.unwrap_or(cursor));
                                clipboard_set(input[s..end].to_string());
                            }
                            input = new_input; cursor = new_cursor; app.comment_anchor = None;
                        }
                        app.mode = Mode::Comment { hunk_idx, input, cursor, original };
                        true
                    }
                    KeyCode::Char('v') if ctrl => {
                        if let Some((ni, nc)) = delete_selection(&input, cursor, app.comment_anchor) {
                            input = ni; cursor = nc; app.comment_anchor = None;
                        }
                        if let Some(text) = clipboard_get() {
                            for c in text.chars() { input.insert(cursor, c); cursor += c.len_utf8(); }
                        }
                        app.mode = Mode::Comment { hunk_idx, input, cursor, original };
                        true
                    }

                    KeyCode::Enter => {
                        if let Some((ni, nc)) = delete_selection(&input, cursor, app.comment_anchor) {
                            input = ni; cursor = nc; app.comment_anchor = None;
                        }
                        input.insert(cursor, '\n'); cursor += 1;
                        app.mode = Mode::Comment { hunk_idx, input, cursor, original };
                        true
                    }

                    KeyCode::Up    if shift => { extend!(); cursor = cursor_up(&input, cursor);   app.mode = Mode::Comment { hunk_idx, input, cursor, original }; true }
                    KeyCode::Up              => { clear_sel!(); cursor = cursor_up(&input, cursor);   app.mode = Mode::Comment { hunk_idx, input, cursor, original }; true }
                    KeyCode::Down  if shift => { extend!(); cursor = cursor_down(&input, cursor); app.mode = Mode::Comment { hunk_idx, input, cursor, original }; true }
                    KeyCode::Down            => { clear_sel!(); cursor = cursor_down(&input, cursor); app.mode = Mode::Comment { hunk_idx, input, cursor, original }; true }
                    KeyCode::Home  if shift => { extend!(); cursor = cursor_home(&input, cursor); app.mode = Mode::Comment { hunk_idx, input, cursor, original }; true }
                    KeyCode::Home            => { clear_sel!(); cursor = cursor_home(&input, cursor); app.mode = Mode::Comment { hunk_idx, input, cursor, original }; true }
                    KeyCode::End   if shift => { extend!(); cursor = cursor_end(&input, cursor);  app.mode = Mode::Comment { hunk_idx, input, cursor, original }; true }
                    KeyCode::End             => { clear_sel!(); cursor = cursor_end(&input, cursor);  app.mode = Mode::Comment { hunk_idx, input, cursor, original }; true }

                    KeyCode::Left if ctrl && shift => { extend!(); cursor = cursor_word_left(&input, cursor);  app.mode = Mode::Comment { hunk_idx, input, cursor, original }; true }
                    KeyCode::Left if ctrl           => { clear_sel!(); cursor = cursor_word_left(&input, cursor);  app.mode = Mode::Comment { hunk_idx, input, cursor, original }; true }
                    KeyCode::Left if shift          => { extend!(); cursor = cursor_prev(&input, cursor);         app.mode = Mode::Comment { hunk_idx, input, cursor, original }; true }
                    KeyCode::Left                   => { clear_sel!(); cursor = cursor_prev(&input, cursor);         app.mode = Mode::Comment { hunk_idx, input, cursor, original }; true }

                    KeyCode::Right if ctrl && shift => { extend!(); cursor = cursor_word_right(&input, cursor); app.mode = Mode::Comment { hunk_idx, input, cursor, original }; true }
                    KeyCode::Right if ctrl           => { clear_sel!(); cursor = cursor_word_right(&input, cursor); app.mode = Mode::Comment { hunk_idx, input, cursor, original }; true }
                    KeyCode::Right if shift          => { extend!(); cursor = cursor_next(&input, cursor);          app.mode = Mode::Comment { hunk_idx, input, cursor, original }; true }
                    KeyCode::Right                   => { clear_sel!(); cursor = cursor_next(&input, cursor);          app.mode = Mode::Comment { hunk_idx, input, cursor, original }; true }

                    KeyCode::Backspace => {
                        if let Some((ni, nc)) = delete_selection(&input, cursor, app.comment_anchor) {
                            input = ni; cursor = nc; app.comment_anchor = None;
                        } else if cursor > 0 {
                            let prev = cursor_prev(&input, cursor);
                            input.drain(prev..cursor);
                            cursor = prev;
                        }
                        app.mode = Mode::Comment { hunk_idx, input, cursor, original };
                        true
                    }
                    KeyCode::Delete => {
                        if let Some((ni, nc)) = delete_selection(&input, cursor, app.comment_anchor) {
                            input = ni; cursor = nc; app.comment_anchor = None;
                        } else if cursor < input.len() {
                            let next = cursor_next(&input, cursor);
                            input.drain(cursor..next);
                        }
                        app.mode = Mode::Comment { hunk_idx, input, cursor, original };
                        true
                    }
                    KeyCode::Char(c) if !ctrl => {
                        if let Some((ni, nc)) = delete_selection(&input, cursor, app.comment_anchor) {
                            input = ni; cursor = nc; app.comment_anchor = None;
                        }
                        input.insert(cursor, c); cursor += c.len_utf8();
                        app.mode = Mode::Comment { hunk_idx, input, cursor, original };
                        true
                    }
                    _ => false,
                };

                if consumed && matches!(app.mode, Mode::Comment { .. }) {
                    let popup_content_height = terminal.size()
                        .map(|s| {
                            let popup = comment_popup_area(s.width, s.height.saturating_sub(1));
                            popup.height.saturating_sub(3) as usize
                        })
                        .unwrap_or(5);
                    app.scroll_comment_to_cursor(popup_content_height);
                }
            }
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

    let [file_area, right_area] = {
        let areas = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(32), Constraint::Min(0)])
            .split(vertical[0]);
        [areas[0], areas[1]]
    };

    render_file_list(frame, app, file_area);

    if !app.notes.is_empty() {
        let areas = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(10)])
            .split(right_area);
        render_diff_view(frame, app, areas[0]);
        render_notes_panel(frame, app, areas[1]);
    } else {
        render_diff_view(frame, app, right_area);
    }

    if matches!(app.mode, Mode::Comment { .. }) {
        render_comment_popup(frame, app, vertical[0]);
    }

    render_status_bar(frame, app, vertical[1]);
}

fn render_file_list(frame: &mut Frame, app: &App, area: Rect) {
    let focused = app.focused_panel == Panel::FileList;
    let (border_style, border_type) = if focused {
        (Style::default().fg(ACCENT), BorderType::Double)
    } else {
        (Style::default().fg(Color::DarkGray), BorderType::Plain)
    };

    let items: Vec<ListItem> = app.files.iter().enumerate().map(|(i, f)| {
        let has_notes = app.notes.iter().any(|n| n.file == f.path);
        let note_marker = if has_notes { " ●" } else { "" };
        let status_color = match f.status {
            FileStatus::Added    => Color::Green,
            FileStatus::Modified => Color::Yellow,
            FileStatus::Deleted  => Color::Red,
            FileStatus::Renamed  => Color::Cyan,
        };
        let base_style = if i == app.selected_file {
            Style::default().add_modifier(Modifier::REVERSED).add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        ListItem::new(Line::from(vec![
            Span::styled(format!("[{}]", f.status.indicator()), base_style.fg(status_color)),
            Span::styled(format!(" {}{}", f.path.display(), note_marker), base_style),
        ]))
    }).collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .border_type(border_type)
            .style(Style::default().bg(app.highlighter.panel_bg))
            .title(format!(" Files ({}) ", app.files.len())),
    );
    let mut state = ListState::default();
    state.select(Some(app.selected_file));
    frame.render_stateful_widget(list, area, &mut state);
}

fn render_diff_view(frame: &mut Frame, app: &App, area: Rect) {
    let focused = app.focused_panel == Panel::DiffView;
    let (border_style, border_type) = if focused {
        (Style::default().fg(ACCENT), BorderType::Double)
    } else {
        (Style::default().fg(Color::DarkGray), BorderType::Plain)
    };

    let title = {
        let file_name = app.files.get(app.selected_file)
            .map(|f| f.path.display().to_string())
            .unwrap_or_else(|| "Diff".to_string());
        match &app.current_diff {
            Some(diff) if !diff.hunks.is_empty() =>
                format!(" {} — {}/{} ", file_name, app.selected_hunk + 1, diff.hunks.len()),
            _ => format!(" {} ", file_name),
        }
    };

    let note_max_chars = area.width.saturating_sub(6) as usize;
    let text = build_diff_text(app, note_max_chars);

    let para = Paragraph::new(text)
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .border_type(border_type)
            .style(Style::default().bg(app.highlighter.panel_bg))
            .title(title))
        .scroll((app.diff_scroll as u16, 0));

    frame.render_widget(para, area);
}

// ── Comment popup ─────────────────────────────────────────────────────────────

fn comment_popup_area(total_width: u16, total_height: u16) -> Rect {
    let width  = (total_width  * 70 / 100).max(40).min(total_width.saturating_sub(4));
    let height = (total_height * 40 / 100).max(8).min(total_height.saturating_sub(4));
    Rect {
        x: (total_width.saturating_sub(width)) / 2,
        y: (total_height.saturating_sub(height)) / 2,
        width,
        height,
    }
}

/// Build styled spans for one line of comment input.
///
/// Only handles selection highlighting — the cursor is rendered by the
/// terminal itself via `frame.set_cursor_position` and blinks as a bar.
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

fn render_comment_popup(frame: &mut Frame, app: &App, area: Rect) {
    let Mode::Comment { ref input, cursor, hunk_idx, .. } = app.mode else { return };

    let rel = comment_popup_area(area.width, area.height);
    let popup = Rect { x: area.x + rel.x, y: area.y + rel.y, width: rel.width, height: rel.height };

    frame.render_widget(Clear, popup);

    let hunk_header = app.current_diff.as_ref()
        .and_then(|d| d.hunks.get(hunk_idx))
        .map(|h| h.header.clone())
        .unwrap_or_default();
    let max_title = popup.width.saturating_sub(4) as usize;
    let title_hunk: String = if hunk_header.chars().count() > max_title {
        format!("{}…", hunk_header.chars().take(max_title.saturating_sub(1)).collect::<String>())
    } else {
        hunk_header
    };
    let title = if title_hunk.is_empty() { " Comment ".to_string() } else { format!(" {} ", title_hunk) };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(ACCENT))
        .style(Style::default().bg(app.highlighter.panel_bg))
        .title(title);

    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    if inner.height < 2 { return; }

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(inner);
    let content_area = layout[0];
    let help_area    = layout[1];

    // Render text with selection highlighting (no embedded cursor character).
    let selection = selected_range(cursor, app.comment_anchor);
    let input_lines: Vec<&str> = input.split('\n').collect();
    let mut line_start_byte = 0;
    let mut lines: Vec<Line<'static>> = Vec::new();
    for line_text in &input_lines {
        lines.push(Line::from(line_spans(line_text, line_start_byte, selection)));
        line_start_byte += line_text.len() + 1; // +1 for '\n'
    }

    let content_para = Paragraph::new(Text::from(lines))
        .scroll((app.comment_scroll as u16, 0))
        .wrap(Wrap { trim: false });
    frame.render_widget(content_para, content_area);

    // Position the blinking bar cursor. Uses logical lines/cols — accurate when
    // lines are shorter than the content width (typical for prose comments).
    let cursor_line = input[..cursor].matches('\n').count();
    let cursor_col  = input[..cursor].split('\n').last().map(|s| s.chars().count()).unwrap_or(0);
    if cursor_line >= app.comment_scroll {
        let visible_row = (cursor_line - app.comment_scroll) as u16;
        if visible_row < content_area.height {
            let x = (content_area.x + cursor_col as u16).min(content_area.x + content_area.width.saturating_sub(1));
            let y = content_area.y + visible_row;
            frame.set_cursor_position((x, y));
        }
    }

    let help = Paragraph::new(" Ctrl+S: submit   Esc: cancel")
        .style(Style::default().add_modifier(Modifier::REVERSED));
    frame.render_widget(help, help_area);
}

// ── Cursor movement ───────────────────────────────────────────────────────────

fn cursor_prev(s: &str, cursor: usize) -> usize {
    if cursor == 0 { return 0; }
    let mut pos = cursor - 1;
    while pos > 0 && !s.is_char_boundary(pos) { pos -= 1; }
    pos
}

fn cursor_next(s: &str, cursor: usize) -> usize {
    if cursor >= s.len() { return s.len(); }
    let mut pos = cursor + 1;
    while pos < s.len() && !s.is_char_boundary(pos) { pos += 1; }
    pos
}

fn cursor_up(input: &str, cursor: usize) -> usize {
    let before = &input[..cursor];
    let Some(prev_nl) = before.rfind('\n') else { return cursor };
    let current_line_start = prev_nl + 1;
    let char_col = input[current_line_start..cursor].chars().count();
    let prev_line_start = input[..prev_nl].rfind('\n').map(|p| p + 1).unwrap_or(0);
    let prev_line = &input[prev_line_start..prev_nl];
    let target_byte = prev_line.char_indices().nth(char_col).map(|(i, _)| i).unwrap_or(prev_line.len());
    prev_line_start + target_byte
}

fn cursor_down(input: &str, cursor: usize) -> usize {
    let current_line_start = input[..cursor].rfind('\n').map(|p| p + 1).unwrap_or(0);
    let char_col = input[current_line_start..cursor].chars().count();
    let rest = &input[cursor..];
    let Some(nl_offset) = rest.find('\n') else { return cursor };
    let next_line_start = cursor + nl_offset + 1;
    let next_line_end = input[next_line_start..].find('\n')
        .map(|p| next_line_start + p)
        .unwrap_or(input.len());
    let next_line = &input[next_line_start..next_line_end];
    let target_byte = next_line.char_indices().nth(char_col).map(|(i, _)| i).unwrap_or(next_line.len());
    next_line_start + target_byte
}

fn cursor_home(input: &str, cursor: usize) -> usize {
    input[..cursor].rfind('\n').map(|p| p + 1).unwrap_or(0)
}

fn cursor_end(input: &str, cursor: usize) -> usize {
    input[cursor..].find('\n').map(|p| cursor + p).unwrap_or(input.len())
}

fn cursor_word_left(input: &str, cursor: usize) -> usize {
    if cursor == 0 { return 0; }
    let chars: Vec<(usize, char)> = input[..cursor].char_indices().collect();
    let n = chars.len();
    let mut i = n;
    while i > 0 && !is_word_char(chars[i - 1].1) { i -= 1; }
    while i > 0 && is_word_char(chars[i - 1].1) { i -= 1; }
    if i == 0 { 0 } else { chars[i].0 }
}

fn cursor_word_right(input: &str, cursor: usize) -> usize {
    if cursor >= input.len() { return input.len(); }
    let chars: Vec<(usize, char)> = input[cursor..].char_indices().collect();
    let n = chars.len();
    let mut i = 0;
    while i < n && is_word_char(chars[i].1) { i += 1; }
    while i < n && !is_word_char(chars[i].1) { i += 1; }
    cursor + if i < n { chars[i].0 } else { input[cursor..].len() }
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

// ── Diff rendering ────────────────────────────────────────────────────────────

fn push_diff_line(
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
                spans.push(Span::styled(token.content.clone(), style));
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
            spans.push(Span::styled(dl.content.clone(), style));
        }
    }
    out.push(Line::from(spans));
}

fn push_diff_lines_folded(
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
        } else {
            Style::default().fg(MUTED)
        };
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
                    } else {
                        line_text.to_string()
                    };
                    lines.push(Line::from(Span::styled(format!("{}{}", prefix, display), note_style)));
                }
            }
        }
        lines.push(Line::raw(""));
    }

    Text::from(lines)
}

// ── Notes panel ───────────────────────────────────────────────────────────────

fn render_notes_panel(frame: &mut Frame, app: &App, area: Rect) {
    let focused = app.focused_panel == Panel::NotesView;
    let (border_style, border_type) = if focused {
        (Style::default().fg(ACCENT), BorderType::Double)
    } else {
        (Style::default().fg(Color::DarkGray), BorderType::Plain)
    };

    let content_width = area.width.saturating_sub(2) as usize;
    let max_header = content_width.saturating_sub(2);
    let max_text   = content_width.saturating_sub(4);

    let mut lines: Vec<Line<'static>> = Vec::new();
    if app.notes.is_empty() {
        lines.push(Line::from(Span::styled("No notes yet.", Style::default().fg(Color::DarkGray))));
    } else {
        for (i, note) in app.notes.iter().enumerate() {
            let is_selected = i == app.selected_note;
            let is_expanded = app.expanded_notes.contains(&i);
            let header_style = if is_selected {
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(MUTED)
            };
            let marker = if is_selected { "▶ " } else { "  " };
            let full_header = format!("{} · {}", note.file.display(), note.hunk_header);
            let header_text = if full_header.chars().count() > max_header {
                format!("{}…", full_header.chars().take(max_header.saturating_sub(1)).collect::<String>())
            } else {
                full_header
            };
            lines.push(Line::from(Span::styled(format!("{}{}", marker, header_text), header_style)));

            let note_style = Style::default().fg(Color::White);
            if is_expanded {
                for line_text in note.note.lines() {
                    lines.push(Line::from(vec![Span::raw("    "), Span::styled(line_text.to_string(), note_style)]));
                }
            } else {
                let first_line = note.note.lines().next().unwrap_or("");
                let truncated = if first_line.chars().count() > max_text {
                    format!("{}…", first_line.chars().take(max_text.saturating_sub(1)).collect::<String>())
                } else {
                    first_line.to_string()
                };
                lines.push(Line::from(vec![Span::raw("    "), Span::styled(truncated, note_style)]));
            }
            lines.push(Line::raw(""));
        }
    }

    let para = Paragraph::new(Text::from(lines))
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .border_type(border_type)
            .style(Style::default().bg(app.highlighter.panel_bg))
            .title(format!(" Notes ({}) ", app.notes.len())))
        .wrap(Wrap { trim: false });
    frame.render_widget(para, area);
}

fn jump_to_note<G: GitBackend>(app: &mut App, git: &G) {
    let Some(file_idx) = app.selected_note_file_idx() else { return };
    let target_header = app.notes[app.selected_note].hunk_header.clone();
    app.select_file(file_idx);
    load_current_file(app, git);
    if let Some(hunk_idx) = app.current_diff.as_ref()
        .and_then(|d| d.hunks.iter().position(|h| h.header == target_header))
    {
        app.selected_hunk = hunk_idx;
        app.scroll_to_selected_hunk();
    }
    app.focused_panel = Panel::DiffView;
}

fn status_bar_text(app: &App) -> String {
    match app.mode {
        Mode::Comment { .. } => " Ctrl+S: submit   Ctrl+C/V/X: copy/paste/cut   Shift+arrows: select   Esc: cancel".to_string(),
        Mode::Normal => match app.focused_panel {
            Panel::FileList  => " Tab/Shift+Tab: navigate  ↑↓: files  Enter: open  q: quit".to_string(),
            Panel::NotesView => " Tab/Shift+Tab: navigate  ↑↓: notes  Enter: jump  Space: expand  e: edit  d: delete  q: quit".to_string(),
            Panel::DiffView  => {
                let note_count = app.notes.len();
                let notes_str = if note_count == 1 { "  ●1 note".to_string() }
                    else if note_count > 1 { format!("  ●{} notes", note_count) }
                    else { String::new() };
                let note_actions = if app.current_hunk_has_note() { "  e: edit  d: delete" } else { "  c: comment" };
                let fold_hint = if app.selected_hunk_is_foldable() {
                    if app.expanded_hunks.contains(&app.selected_hunk) { "  Space: fold" } else { "  Space: expand" }
                } else { "" };
                format!(" Tab/Shift+Tab: navigate  ↑↓: scroll  []: hunk{}{}  q: quit{}", note_actions, fold_hint, notes_str)
            }
        },
    }
}

fn render_status_bar(frame: &mut Frame, app: &App, area: Rect) {
    frame.render_widget(
        Paragraph::new(status_bar_text(app)).style(Style::default().add_modifier(Modifier::REVERSED)),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::{ChangedFile, DiffFile, DiffLine, FileStatus, Hunk, LineKind};
    use std::path::PathBuf;

    fn make_app_with_hunks(hunk_count: usize) -> App {
        let files = vec![ChangedFile { path: PathBuf::from("src/main.rs"), status: FileStatus::Modified }];
        let mut app = App::new(files.clone(), "main".to_string(), "HEAD".to_string());
        app.focused_panel = Panel::DiffView;
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

    fn text_to_string(text: &Text<'static>) -> String {
        text.lines.iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect::<String>())
            .collect::<Vec<_>>().join("\n")
    }

    fn spans_text(spans: &[Span<'static>]) -> String {
        spans.iter().map(|s| s.content.as_ref()).collect()
    }

    fn has_sel_bg(span: &Span<'static>) -> bool {
        span.style.bg == Some(SEL_BG)
    }

    // ── line_spans ────────────────────────────────────────────────────────────

    #[test]
    fn test_line_spans_no_selection() {
        let spans = line_spans("hello", 0, None);
        assert_eq!(spans_text(&spans), "hello");
        assert!(spans.iter().all(|s| !has_sel_bg(s)));
    }

    #[test]
    fn test_line_spans_no_cursor_block_character() {
        // Cursor is rendered by the terminal, not embedded in spans.
        let spans = line_spans("hello", 0, None);
        assert!(!spans_text(&spans).contains("█"));
    }

    #[test]
    fn test_line_spans_selection_highlights_middle() {
        // selection [1..4] = "ell"
        let spans = line_spans("hello", 0, Some((1, 4)));
        assert_eq!(spans_text(&spans), "hello");
        let sel = spans.iter().find(|s| s.content.as_ref() == "ell").unwrap();
        assert!(has_sel_bg(sel));
        let h = spans.iter().find(|s| s.content.as_ref() == "h").unwrap();
        assert!(!has_sel_bg(h));
    }

    #[test]
    fn test_line_spans_selection_from_start() {
        let spans = line_spans("hello", 0, Some((0, 3)));
        assert_eq!(spans_text(&spans), "hello");
        let sel = spans.iter().find(|s| s.content.as_ref() == "hel").unwrap();
        assert!(has_sel_bg(sel));
    }

    #[test]
    fn test_line_spans_selection_to_end() {
        let spans = line_spans("hello", 0, Some((3, 5)));
        assert_eq!(spans_text(&spans), "hello");
        let sel = spans.iter().find(|s| s.content.as_ref() == "lo").unwrap();
        assert!(has_sel_bg(sel));
    }

    #[test]
    fn test_line_spans_multiline_sel_on_first_line() {
        // "hello\nworld", selection (3..9): on "hello" (start=0) → sel [3..5]="lo"
        let spans = line_spans("hello", 0, Some((3, 9)));
        assert_eq!(spans_text(&spans), "hello");
        let lo = spans.iter().find(|s| s.content.as_ref() == "lo").unwrap();
        assert!(has_sel_bg(lo));
        let hel = spans.iter().find(|s| s.content.as_ref() == "hel").unwrap();
        assert!(!has_sel_bg(hel));
    }

    #[test]
    fn test_line_spans_multiline_sel_on_second_line() {
        // On "world" (line_start=6): sel covers bytes 6..9 → local [0..3]="wor"
        let spans = line_spans("world", 6, Some((3, 9)));
        assert_eq!(spans_text(&spans), "world");
        let wor = spans.iter().find(|s| s.content.as_ref() == "wor").unwrap();
        assert!(has_sel_bg(wor));
        let ld = spans.iter().find(|s| s.content.as_ref() == "ld").unwrap();
        assert!(!has_sel_bg(ld));
    }

    #[test]
    fn test_line_spans_fully_selected() {
        let spans = line_spans("world", 6, Some((3, 20)));
        assert_eq!(spans_text(&spans), "world");
        assert!(spans.iter().all(has_sel_bg));
    }

    #[test]
    fn test_line_spans_empty_line() {
        let spans = line_spans("", 0, None);
        assert_eq!(spans_text(&spans), "");
    }

    #[test]
    fn test_line_spans_selection_not_on_this_line() {
        let spans = line_spans("hello", 0, Some((10, 20)));
        assert_eq!(spans_text(&spans), "hello");
        assert!(spans.iter().all(|s| !has_sel_bg(s)));
    }

    // ── Diff text ─────────────────────────────────────────────────────────────

    #[test]
    fn test_selected_hunk_has_marker() {
        assert!(text_to_string(&build_diff_text(&make_app_with_hunks(2), 1000)).contains("▶ "));
    }

    #[test]
    fn test_non_selected_hunk_has_no_marker() {
        let mut app = make_app_with_hunks(2);
        app.selected_hunk = 0;
        assert_eq!(text_to_string(&build_diff_text(&app, 1000)).matches("▶").count(), 1);
    }

    #[test]
    fn test_selecting_second_hunk_moves_marker() {
        let mut app = make_app_with_hunks(2);
        app.selected_hunk = 1;
        let content = text_to_string(&build_diff_text(&app, 1000));
        assert_eq!(content.matches("▶").count(), 1);
        let pos = content.find("▶").unwrap();
        assert!(content[pos + "▶ ".len()..].starts_with("@@ -11,"));
    }

    #[test]
    fn test_non_selected_headers_have_indent() {
        let mut app = make_app_with_hunks(2);
        app.selected_hunk = 1;
        let text = build_diff_text(&app, 1000);
        let first = text.lines.iter().find(|l| l.spans.iter().any(|s| s.content.contains("@@"))).unwrap();
        assert_eq!(first.spans[0].content.as_ref(), "  ");
    }

    #[test]
    fn test_no_diff_shows_loading() {
        let files = vec![ChangedFile { path: PathBuf::from("src/main.rs"), status: FileStatus::Modified }];
        let app = App::new(files, "main".to_string(), "HEAD".to_string());
        assert!(text_to_string(&build_diff_text(&app, 1000)).contains("Loading"));
    }

    #[test]
    fn test_comment_input_not_in_diff_text() {
        let mut app = make_app_with_hunks(1);
        app.mode = Mode::Comment { hunk_idx: 0, input: "my important comment".to_string(), cursor: 0, original: None };
        assert!(!text_to_string(&build_diff_text(&app, 1000)).contains("my important comment"));
    }

    #[test]
    fn test_submitted_note_still_shown_inline() {
        let mut app = make_app_with_hunks(1);
        app.mode = Mode::Comment { hunk_idx: 0, input: "submitted note".to_string(), cursor: 0, original: None };
        app.submit_comment();
        assert!(text_to_string(&build_diff_text(&app, 1000)).contains("submitted note"));
    }

    #[test]
    fn test_inline_note_truncated_when_exceeds_max() {
        let mut app = make_app_with_hunks(1);
        app.mode = Mode::Comment { hunk_idx: 0, input: "a".repeat(60), cursor: 0, original: None };
        app.submit_comment();
        let content = text_to_string(&build_diff_text(&app, 20));
        assert!(content.contains("…") && !content.contains(&"a".repeat(21)));
    }

    #[test]
    fn test_inline_note_not_truncated_when_fits() {
        let mut app = make_app_with_hunks(1);
        app.mode = Mode::Comment { hunk_idx: 0, input: "short".to_string(), cursor: 0, original: None };
        app.submit_comment();
        let content = text_to_string(&build_diff_text(&app, 1000));
        assert!(content.contains("short") && !content.contains("…"));
    }

    // ── Context folding ───────────────────────────────────────────────────────

    fn make_long_context_app() -> App {
        let files = vec![ChangedFile { path: PathBuf::from("src/main.rs"), status: FileStatus::Modified }];
        let mut app = App::new(files.clone(), "main".to_string(), "HEAD".to_string());
        app.focused_panel = Panel::DiffView;
        let mut lines = vec![DiffLine { old_lineno: None, new_lineno: Some(1), kind: LineKind::Added, content: "added".to_string() }];
        for i in 0..crate::app::FOLD_THRESHOLD {
            lines.push(DiffLine { old_lineno: Some(i as u32 + 1), new_lineno: Some(i as u32 + 2), kind: LineKind::Context, content: format!("ctx {}", i) });
        }
        lines.push(DiffLine { old_lineno: Some(10), new_lineno: None, kind: LineKind::Removed, content: "removed".to_string() });
        app.current_diff = Some(DiffFile {
            file: files[0].clone(),
            hunks: vec![Hunk { header: "@@ -1,10 +1,10 @@".to_string(), old_start: 1, new_start: 1, lines }],
        });
        app
    }

    #[test]
    fn test_folded_hunk_shows_placeholder() {
        let c = text_to_string(&build_diff_text(&make_long_context_app(), 1000));
        assert!(c.contains("·· ") && c.contains("lines of context"));
    }

    #[test]
    fn test_folded_hunk_hides_individual_context_lines() {
        assert!(!text_to_string(&build_diff_text(&make_long_context_app(), 1000)).contains("ctx 0"));
    }

    #[test]
    fn test_expanded_hunk_shows_context_lines() {
        let mut app = make_long_context_app();
        app.expanded_hunks.insert(0);
        let c = text_to_string(&build_diff_text(&app, 1000));
        assert!(c.contains("ctx 0") && !c.contains("lines of context"));
    }

    #[test]
    fn test_folded_hunk_still_shows_changed_lines() {
        let c = text_to_string(&build_diff_text(&make_long_context_app(), 1000));
        assert!(c.contains("added") && c.contains("removed"));
    }

    // ── Comment popup ─────────────────────────────────────────────────────────

    fn popup_rendered(app: &App) -> String {
        let backend = ratatui::backend::TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render_comment_popup(f, app, f.area())).unwrap();
        terminal.backend().buffer().content().iter().map(|c| c.symbol()).collect()
    }

    #[test]
    fn test_popup_renders_input_text() {
        let mut app = make_app_with_hunks(1);
        app.mode = Mode::Comment { hunk_idx: 0, input: "review comment text".to_string(), cursor: 0, original: None };
        assert!(popup_rendered(&app).contains("review comment text"));
    }

    #[test]
    fn test_popup_has_no_block_cursor_character() {
        // The █ character must not appear — cursor is rendered by the terminal.
        let mut app = make_app_with_hunks(1);
        app.mode = Mode::Comment { hunk_idx: 0, input: "hello world".to_string(), cursor: 5, original: None };
        assert!(!popup_rendered(&app).contains("█"));
    }

    #[test]
    fn test_popup_not_rendered_in_normal_mode() {
        assert!(!popup_rendered(&make_app_with_hunks(1)).contains("Comment"));
    }

    #[test]
    fn test_popup_renders_help_line() {
        let mut app = make_app_with_hunks(1);
        app.mode = Mode::Comment { hunk_idx: 0, input: String::new(), cursor: 0, original: None };
        let rendered = popup_rendered(&app);
        assert!(rendered.contains("Ctrl+S") && rendered.contains("Esc"));
    }

    #[test]
    fn test_popup_renders_multiline_input() {
        let mut app = make_app_with_hunks(1);
        app.mode = Mode::Comment { hunk_idx: 0, input: "line one\nline two\nline three".to_string(), cursor: 0, original: None };
        let rendered = popup_rendered(&app);
        assert!(rendered.contains("line one") && rendered.contains("line two") && rendered.contains("line three"));
    }

    #[test]
    fn test_popup_title_shows_hunk_header() {
        let mut app = make_app_with_hunks(1);
        app.mode = Mode::Comment { hunk_idx: 0, input: String::new(), cursor: 0, original: None };
        assert!(popup_rendered(&app).contains("@@"));
    }

    // ── status_bar_text ───────────────────────────────────────────────────────

    fn app_diff_view() -> App { let mut a = make_app_with_hunks(1); a.focused_panel = Panel::DiffView; a }

    #[test]
    fn test_status_bar_comment_mode() {
        let mut app = app_diff_view();
        app.mode = Mode::Comment { hunk_idx: 0, input: String::new(), cursor: 0, original: None };
        let text = status_bar_text(&app);
        assert!(text.contains("Ctrl+S: submit") && text.contains("Esc: cancel"));
    }

    #[test]
    fn test_status_bar_comment_mode_mentions_clipboard() {
        let mut app = app_diff_view();
        app.mode = Mode::Comment { hunk_idx: 0, input: String::new(), cursor: 0, original: None };
        assert!(status_bar_text(&app).contains("Ctrl+C/V/X"));
    }

    #[test]
    fn test_status_bar_comment_mode_mentions_selection() {
        let mut app = app_diff_view();
        app.mode = Mode::Comment { hunk_idx: 0, input: String::new(), cursor: 0, original: None };
        assert!(status_bar_text(&app).contains("Shift+arrows"));
    }

    #[test]
    fn test_status_bar_file_list_panel() {
        let mut app = app_diff_view(); app.focused_panel = Panel::FileList;
        let text = status_bar_text(&app);
        assert!(text.contains("Tab") && text.contains("Enter: open"));
    }

    #[test]
    fn test_status_bar_notes_view_panel() {
        let mut app = app_diff_view(); app.focused_panel = Panel::NotesView;
        let text = status_bar_text(&app);
        assert!(text.contains("Enter: jump") && text.contains("e: edit") && text.contains("d: delete"));
    }

    #[test]
    fn test_status_bar_diff_no_notes_shows_comment_action() {
        assert!(status_bar_text(&app_diff_view()).contains("c: comment"));
    }

    #[test]
    fn test_status_bar_diff_one_note() {
        let mut app = app_diff_view();
        app.mode = Mode::Comment { hunk_idx: 0, input: "a note".to_string(), cursor: 0, original: None };
        app.submit_comment();
        assert!(status_bar_text(&app).contains("●1 note"));
    }

    #[test]
    fn test_status_bar_diff_multiple_notes() {
        let mut app = make_app_with_hunks(2); app.focused_panel = Panel::DiffView;
        for hunk_idx in [0, 1] {
            app.mode = Mode::Comment { hunk_idx, input: "note".to_string(), cursor: 0, original: None };
            app.submit_comment(); app.selected_hunk = hunk_idx;
        }
        app.selected_hunk = 0;
        assert!(status_bar_text(&app).contains("●2 notes"));
    }

    #[test]
    fn test_status_bar_diff_hunk_with_note_shows_edit_delete() {
        let mut app = app_diff_view();
        app.mode = Mode::Comment { hunk_idx: 0, input: "existing".to_string(), cursor: 0, original: None };
        app.submit_comment();
        let text = status_bar_text(&app);
        assert!(text.contains("e: edit") && text.contains("d: delete") && !text.contains("c: comment"));
    }

    #[test]
    fn test_status_bar_foldable_hunk_shows_expand() {
        assert!(status_bar_text(&make_long_context_app()).contains("Space: expand"));
    }

    #[test]
    fn test_status_bar_expanded_hunk_shows_fold() {
        let mut app = make_long_context_app(); app.expanded_hunks.insert(0);
        assert!(status_bar_text(&app).contains("Space: fold"));
    }

    // ── Notes panel ───────────────────────────────────────────────────────────

    use ratatui::{Terminal, backend::TestBackend};

    fn notes_panel_rendered(app: &App) -> String {
        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render_notes_panel(f, app, f.area())).unwrap();
        terminal.backend().buffer().content().iter().map(|c| c.symbol()).collect()
    }

    fn app_with_note(note_text: &str) -> App {
        let mut app = make_app_with_hunks(1); app.focused_panel = Panel::DiffView;
        app.mode = Mode::Comment { hunk_idx: 0, input: note_text.to_string(), cursor: 0, original: None };
        app.submit_comment(); app.focused_panel = Panel::NotesView; app
    }

    #[test]
    fn test_notes_panel_empty_shows_placeholder() {
        let mut app = make_app_with_hunks(1); app.focused_panel = Panel::NotesView;
        assert!(notes_panel_rendered(&app).contains("No notes yet."));
    }

    #[test]
    fn test_notes_panel_short_note_shown_in_full() {
        assert!(notes_panel_rendered(&app_with_note("short note")).contains("short note"));
    }

    #[test]
    fn test_notes_panel_long_note_truncated() {
        let long = "a".repeat(80);
        let rendered = notes_panel_rendered(&app_with_note(&long));
        assert!(rendered.contains("…") && !rendered.contains(&"a".repeat(75)));
    }

    #[test]
    fn test_notes_panel_expanded_note_shows_full_text() {
        let long = "a".repeat(80);
        let mut app = app_with_note(&long); app.expanded_notes.insert(0);
        let rendered = notes_panel_rendered(&app);
        assert!(rendered.contains(&"a".repeat(74)) && !rendered.contains("…"));
    }

    #[test]
    fn test_notes_panel_selected_note_has_marker() {
        assert!(notes_panel_rendered(&app_with_note("my note")).contains("▶"));
    }

    #[test]
    fn test_notes_panel_shows_hunk_header_in_item() {
        assert!(notes_panel_rendered(&app_with_note("some note")).contains("@@"));
    }

    // ── Cursor movement ───────────────────────────────────────────────────────

    #[test] fn test_cursor_up_previous_line_start()   { assert_eq!(cursor_up("hello\nworld", 6), 0); }
    #[test] fn test_cursor_up_preserves_column()      { assert_eq!(cursor_up("hello\nworld", 8), 2); }
    #[test] fn test_cursor_up_clamps_short_line()     { assert_eq!(cursor_up("hi\nworld", 7), 2); }
    #[test] fn test_cursor_up_first_line_no_op()      { assert_eq!(cursor_up("hello\nworld", 3), 3); }

    #[test] fn test_cursor_down_next_line_same_col()  { assert_eq!(cursor_down("hello\nworld", 0), 6); }
    #[test] fn test_cursor_down_preserves_column()    { assert_eq!(cursor_down("hello\nworld", 3), 9); }
    #[test] fn test_cursor_down_clamps_column()       { assert_eq!(cursor_down("hello\nhi", 4), 8); }
    #[test] fn test_cursor_down_last_line_no_op()     { assert_eq!(cursor_down("hello\nworld", 8), 8); }

    #[test] fn test_cursor_home_moves_to_start()      { assert_eq!(cursor_home("hello\nworld", 9), 6); }
    #[test] fn test_cursor_home_first_line()          { assert_eq!(cursor_home("hello", 3), 0); }
    #[test] fn test_cursor_home_already_at_start()    { assert_eq!(cursor_home("hello\nworld", 6), 6); }

    #[test] fn test_cursor_end_moves_to_end()         { assert_eq!(cursor_end("hello\nworld", 0), 5); }
    #[test] fn test_cursor_end_last_line()            { assert_eq!(cursor_end("hello\nworld", 8), 11); }
    #[test] fn test_cursor_end_already_at_end()       { assert_eq!(cursor_end("hello\nworld", 5), 5); }

    #[test] fn test_word_left_to_word_start()         { assert_eq!(cursor_word_left("foo bar baz", 11), 8); }
    #[test] fn test_word_left_skips_whitespace()      { assert_eq!(cursor_word_left("foo bar", 4), 0); }
    #[test] fn test_word_left_at_start_no_op()        { assert_eq!(cursor_word_left("foo bar", 0), 0); }
    #[test] fn test_word_right_past_word_and_space()  { assert_eq!(cursor_word_right("foo bar baz", 0), 4); }
    #[test] fn test_word_right_from_middle()          { assert_eq!(cursor_word_right("foo bar", 1), 4); }
    #[test] fn test_word_right_at_end_no_op()         { assert_eq!(cursor_word_right("foo bar", 7), 7); }
}
