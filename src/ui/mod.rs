mod cursor;
mod diff_render;
mod popup;

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
    widgets::{Block, BorderType, Borders, List, ListItem, ListState, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap},
    Frame, Terminal,
};

use crate::app::{App, FeedbackNote, Mode, Panel, delete_selection, selected_range};
use crate::diff::{ChangedFile, FileStatus};
use crate::filetree::TreeItem;
use crate::git::{GitBackend, WhitespaceMode};

use cursor::{
    cursor_up_visual, cursor_down_visual,
    cursor_prev, cursor_next, cursor_home, cursor_end,
    cursor_word_left, cursor_word_right,
};
use diff_render::build_diff_text;
use popup::{comment_popup_content_width, render_comment_popup};

// ── Palette ───────────────────────────────────────────────────────────────────
pub(crate) const ACCENT: Color = Color::Cyan;
pub(crate) const MUTED:  Color = Color::Rgb(100, 110, 130);
pub(crate) const NOTE_FG:Color = Color::Rgb(100, 150, 210);
pub(crate) const SEL_BG: Color = Color::Rgb(60, 80, 140);

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
    if app.files.is_empty() { return; }
    app.diff_scroll = 0;
    app.selected_hunk = 0;
    let path = app.files[app.selected_file].path.to_string_lossy().to_string();
    let file = app.files[app.selected_file].clone();
    log::debug!("[ui] load_current_file: path={:?}", path);
    let result = git.file_diff(&app.from, &app.to, &path, app.whitespace_mode);
    log::debug!("[ui] load_current_file: file_diff result={}", match &result {
        Ok(s) => format!("Ok({} bytes)", s.len()), Err(e) => format!("Err({e})")
    });
    app.current_diff = result.ok().map(|raw| crate::diff::parse_diff(&raw, file));
    app.current_highlights = app.current_diff.as_ref().map(|d| app.highlighter.highlight_diff(d));
    log::debug!("[ui] load_current_file: current_diff={}", match &app.current_diff {
        Some(d) => format!("Some({} hunks)", d.hunks.len()), None => "None".into()
    });
}

fn run_event_loop<G: GitBackend>(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    git: &G,
) -> Result<()> {
    loop {
        // Keep diff_view_content_width in sync so scroll accounting matches the
        // wrapped rendering: files panel (32) + diff borders (2) + gutter (6) = 40.
        if let Ok(s) = terminal.size() {
            app.diff_view_content_width = (s.width as usize).saturating_sub(40);
        }
        terminal.draw(|f| render(f, app))?;
        let Event::Key(key) = event::read()? else { continue; };
        if key.kind != KeyEventKind::Press { continue; }

        match app.mode.clone() {
            Mode::Normal => {
                match key.code {
                    KeyCode::Char('q') => break,
                    KeyCode::Tab => {
                        app.focused_panel = match app.focused_panel {
                            Panel::FileList  => Panel::DiffView,
                            Panel::DiffView  => if !app.notes.is_empty() { Panel::NotesView } else { Panel::FileList },
                            Panel::NotesView => Panel::FileList,
                        };
                    }
                    KeyCode::BackTab => {
                        app.focused_panel = match app.focused_panel {
                            Panel::FileList  => if !app.notes.is_empty() { Panel::NotesView } else { Panel::DiffView },
                            Panel::DiffView  => Panel::FileList,
                            Panel::NotesView => Panel::DiffView,
                        };
                    }
                    KeyCode::Up => match app.focused_panel {
                        Panel::FileList  => app.file_list_up(),
                        Panel::DiffView  => app.diff_scroll_up(),
                        Panel::NotesView => { app.notes_up(); app.scroll_notes_to_selected(8); }
                    },
                    KeyCode::Down => match app.focused_panel {
                        Panel::FileList  => app.file_list_down(),
                        Panel::DiffView  => {
                            let vp = terminal.size().map(|r| r.height.saturating_sub(3) as usize).unwrap_or(20);
                            app.diff_scroll_down(vp);
                        }
                        Panel::NotesView => { app.notes_down(); app.scroll_notes_to_selected(8); }
                    },
                    KeyCode::Left => {
                        if app.focused_panel == Panel::FileList {
                            app.file_list_scroll_left();
                        }
                    }
                    KeyCode::Right => {
                        if app.focused_panel == Panel::FileList {
                            app.file_list_scroll_right();
                        }
                    }
                    KeyCode::Enter => {
                        if app.focused_panel == Panel::FileList {
                            let is_dir = app.tree_items().get(app.file_tree_cursor)
                                .map(|i| i.is_dir()).unwrap_or(false);
                            if is_dir {
                                app.toggle_dir_at_cursor();
                            } else {
                                load_current_file(app, git);
                            }
                        } else if app.focused_panel == Panel::NotesView {
                            jump_to_note(app, git);
                        }
                    }
                    KeyCode::Char('[') => { if app.focused_panel == Panel::DiffView { app.prev_hunk(); } }
                    KeyCode::Char(']') => { if app.focused_panel == Panel::DiffView { app.next_hunk(); } }
                    KeyCode::Char('w') => {
                        if app.focused_panel == Panel::DiffView {
                            app.cycle_whitespace_mode();
                            load_current_file(app, git);
                        }
                    }
                    KeyCode::Char('c') => { if app.focused_panel == Panel::DiffView { app.start_comment(); } }
                    KeyCode::Char(' ') => match app.focused_panel {
                        Panel::FileList  => app.toggle_dir_at_cursor(),
                        Panel::DiffView  => app.toggle_hunk_fold(),
                        Panel::NotesView => { app.toggle_note_expand(); app.scroll_notes_to_selected(8); }
                    },
                    KeyCode::Char('e') => match app.focused_panel {
                        Panel::DiffView  => { app.edit_note_for_current_hunk(); }
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
                let (tw, th) = terminal.size().map(|s| (s.width, s.height)).unwrap_or((80, 24));
                let cw = comment_popup_content_width(tw, th);

                macro_rules! extend { () => {
                    if app.comment_anchor.is_none() { app.comment_anchor = Some(cursor); }
                }}
                macro_rules! clear_sel { () => { app.comment_anchor = None; }}

                let consumed = match key.code {
                    KeyCode::Char('s') if ctrl => { app.submit_comment(); true }
                    KeyCode::Esc => { app.cancel_comment(); true }

                    KeyCode::Char('a') if ctrl => {
                        app.comment_anchor = Some(0); cursor = input.len();
                        app.mode = Mode::Comment { hunk_idx, input, cursor, original }; true
                    }
                    KeyCode::Char('c') if ctrl => {
                        if let Some((s, e)) = selected_range(cursor, app.comment_anchor) {
                            clipboard_set(input[s..e].to_string());
                        }
                        true
                    }
                    KeyCode::Char('x') if ctrl => {
                        if let Some((new_input, nc)) = delete_selection(&input, cursor, app.comment_anchor) {
                            if let Some((s, _)) = selected_range(cursor, app.comment_anchor) {
                                let end = s + cursor.max(app.comment_anchor.unwrap_or(cursor))
                                    - cursor.min(app.comment_anchor.unwrap_or(cursor));
                                clipboard_set(input[s..end].to_string());
                            }
                            input = new_input; cursor = nc; app.comment_anchor = None;
                        }
                        app.mode = Mode::Comment { hunk_idx, input, cursor, original }; true
                    }
                    KeyCode::Char('v') if ctrl => {
                        if let Some((ni, nc)) = delete_selection(&input, cursor, app.comment_anchor) {
                            input = ni; cursor = nc; app.comment_anchor = None;
                        }
                        if let Some(text) = clipboard_get() {
                            for c in text.chars() { input.insert(cursor, c); cursor += c.len_utf8(); }
                        }
                        app.mode = Mode::Comment { hunk_idx, input, cursor, original }; true
                    }
                    KeyCode::Enter => {
                        if let Some((ni, nc)) = delete_selection(&input, cursor, app.comment_anchor) {
                            input = ni; cursor = nc; app.comment_anchor = None;
                        }
                        input.insert(cursor, '\n'); cursor += 1;
                        app.mode = Mode::Comment { hunk_idx, input, cursor, original }; true
                    }

                    KeyCode::Up   if shift => { extend!(); cursor = cursor_up_visual(&input, cursor, cw);   app.mode = Mode::Comment { hunk_idx, input, cursor, original }; true }
                    KeyCode::Up            => { clear_sel!(); cursor = cursor_up_visual(&input, cursor, cw);   app.mode = Mode::Comment { hunk_idx, input, cursor, original }; true }
                    KeyCode::Down if shift => { extend!(); cursor = cursor_down_visual(&input, cursor, cw); app.mode = Mode::Comment { hunk_idx, input, cursor, original }; true }
                    KeyCode::Down          => { clear_sel!(); cursor = cursor_down_visual(&input, cursor, cw); app.mode = Mode::Comment { hunk_idx, input, cursor, original }; true }
                    KeyCode::Home if shift => { extend!(); cursor = cursor_home(&input, cursor); app.mode = Mode::Comment { hunk_idx, input, cursor, original }; true }
                    KeyCode::Home          => { clear_sel!(); cursor = cursor_home(&input, cursor); app.mode = Mode::Comment { hunk_idx, input, cursor, original }; true }
                    KeyCode::End  if shift => { extend!(); cursor = cursor_end(&input, cursor);  app.mode = Mode::Comment { hunk_idx, input, cursor, original }; true }
                    KeyCode::End           => { clear_sel!(); cursor = cursor_end(&input, cursor);  app.mode = Mode::Comment { hunk_idx, input, cursor, original }; true }

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
                            input.drain(prev..cursor); cursor = prev;
                        }
                        app.mode = Mode::Comment { hunk_idx, input, cursor, original }; true
                    }
                    KeyCode::Delete => {
                        if let Some((ni, nc)) = delete_selection(&input, cursor, app.comment_anchor) {
                            input = ni; cursor = nc; app.comment_anchor = None;
                        } else if cursor < input.len() {
                            let next = cursor_next(&input, cursor);
                            input.drain(cursor..next);
                        }
                        app.mode = Mode::Comment { hunk_idx, input, cursor, original }; true
                    }
                    KeyCode::Char(c) if !ctrl => {
                        if let Some((ni, nc)) = delete_selection(&input, cursor, app.comment_anchor) {
                            input = ni; cursor = nc; app.comment_anchor = None;
                        }
                        input.insert(cursor, c); cursor += c.len_utf8();
                        app.mode = Mode::Comment { hunk_idx, input, cursor, original }; true
                    }
                    _ => false,
                };

                if consumed && matches!(app.mode, Mode::Comment { .. }) {
                    let (tw2, th2) = terminal.size().map(|s| (s.width, s.height)).unwrap_or((80, 24));
                    let cw2 = comment_popup_content_width(tw2, th2);
                    let popup_h = popup::comment_popup_area(tw2, th2.saturating_sub(1)).height.saturating_sub(3) as usize;
                    app.scroll_comment_to_cursor(popup_h, cw2);
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
    let h_scroll = app.file_list_h_scroll;
    let inner_width = area.width.saturating_sub(2) as usize;
    let tree = app.tree_items();
    let items: Vec<ListItem> = tree.iter().map(|item| {
        let depth = item.depth();
        let indent = "  ".repeat(depth);
        match item {
            TreeItem::Dir { display_name, file_count, collapsed, has_notes, .. } => {
                let arrow = if *collapsed { "▸" } else { "▾" };
                let note_marker = if *has_notes { " ●" } else { "" };
                let raw_spans: Vec<(String, Style)> = vec![
                    (indent, Style::default()),
                    (format!("{} ", arrow), Style::default().fg(ACCENT)),
                    (format!("{} ({}){}", display_name, file_count, note_marker), Style::default()),
                ];
                ListItem::new(Line::from(viewport_hscroll(raw_spans, h_scroll, inner_width)))
            }
            TreeItem::File { file_idx, display_name, has_notes, .. } => {
                let f = &app.files[*file_idx];
                let note_marker = if *has_notes { " ●" } else { "" };
                let status_color = match f.status {
                    FileStatus::Added    => Color::Green,
                    FileStatus::Modified => Color::Yellow,
                    FileStatus::Deleted  => Color::Red,
                    FileStatus::Renamed  => Color::Cyan,
                };
                let raw_spans: Vec<(String, Style)> = vec![
                    (indent, Style::default()),
                    (format!("[{}]", f.status.indicator()), Style::default().fg(status_color)),
                    (format!(" {}{}", display_name, note_marker), Style::default()),
                ];
                ListItem::new(Line::from(viewport_hscroll(raw_spans, h_scroll, inner_width)))
            }
        }
    }).collect();
    let list = List::new(items)
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .border_type(border_type)
            .style(Style::default().bg(app.highlighter.panel_bg))
            .title(format!(" Files ({}) ", app.files.len())))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED).add_modifier(Modifier::BOLD));
    let mut state = ListState::default();
    state.select(Some(app.file_tree_cursor));
    frame.render_stateful_widget(list, area, &mut state);
}

fn render_diff_view(frame: &mut Frame, app: &App, area: Rect) {
    let focused = app.focused_panel == Panel::DiffView;
    let (border_style, border_type) = if focused {
        (Style::default().fg(ACCENT), BorderType::Double)
    } else {
        (Style::default().fg(Color::DarkGray), BorderType::Plain)
    };
    // Use the loaded file's path so the title stays in sync with the diff
    // content. Falling back to selected_file only when nothing is loaded yet.
    let file_name = app.current_diff
        .as_ref()
        .map(|d| d.file.path.display().to_string())
        .or_else(|| app.files.get(app.selected_file).map(|f| f.path.display().to_string()))
        .unwrap_or_else(|| "Diff".to_string());
    let ws_label = app.whitespace_mode.label();
    let title = match &app.current_diff {
        Some(diff) if !diff.hunks.is_empty() =>
            format!(" {} — {}/{}{} ", file_name, app.selected_hunk + 1, diff.hunks.len(), ws_label),
        _ => format!(" {}{} ", file_name, ws_label),
    };
    let note_max_chars = area.width.saturating_sub(6) as usize;
    let text = build_diff_text(app, note_max_chars);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .border_type(border_type)
        .style(Style::default().bg(app.highlighter.panel_bg))
        .title(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(
        Paragraph::new(text)
            .scroll((app.diff_scroll as u16, 0))
            .wrap(Wrap { trim: false }),
        inner,
    );
    // Scroll-position indicator: only shown when the diff is taller than the viewport.
    let total = app.diff_content_lines();
    let viewport = inner.height as usize;
    if total > viewport {
        let mut scrollbar_state = ScrollbarState::new(total)
            .position(app.diff_scroll)
            .viewport_content_length(viewport);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None),
            inner,
            &mut scrollbar_state,
        );
    }
}

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
            } else { Style::default().fg(MUTED) };
            let marker = if is_selected { "▶ " } else { "  " };
            let full_header = format!("{} · {}", note.file.display(), note.hunk_header);
            let header_text = if full_header.chars().count() > max_header {
                format!("{}…", full_header.chars().take(max_header.saturating_sub(1)).collect::<String>())
            } else { full_header };
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
                } else { first_line.to_string() };
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
        .scroll((app.notes_scroll as u16, 0))
        .wrap(Wrap { trim: false });
    frame.render_widget(para, area);
}

fn jump_to_note<G: GitBackend>(app: &mut App, git: &G) {
    let Some(file_idx) = app.selected_note_file_idx() else { return };
    let target_header = app.notes[app.selected_note].hunk_header.clone();
    app.expand_parents_of(file_idx);
    app.select_file(file_idx);
    app.sync_tree_cursor_to_file();
    load_current_file(app, git);
    if let Some(hunk_idx) = app.current_diff.as_ref()
        .and_then(|d| d.hunks.iter().position(|h| h.header == target_header))
    {
        app.selected_hunk = hunk_idx;
        app.scroll_to_selected_hunk();
    }
    app.focused_panel = Panel::DiffView;
}

/// Apply a viewport horizontal scroll to a sequence of coloured spans.
/// Characters before `skip` are dropped (spread evenly across spans in order);
/// at most `max_width` characters are returned. Colours are preserved per span.
fn viewport_hscroll(spans: Vec<(String, Style)>, skip: usize, max_width: usize) -> Vec<Span<'static>> {
    let mut result = Vec::new();
    let mut remaining_skip = skip;
    let mut remaining_width = max_width;
    for (text, style) in spans {
        if remaining_width == 0 { break; }
        let chars: Vec<char> = text.chars().collect();
        let skip_here = remaining_skip.min(chars.len());
        remaining_skip -= skip_here;
        let visible = &chars[skip_here..];
        if visible.is_empty() { continue; }
        let take = remaining_width.min(visible.len());
        let s: String = visible[..take].iter().collect();
        remaining_width -= take;
        result.push(Span::styled(s, style));
    }
    result
}

fn status_bar_text(app: &App) -> String {
    match app.mode {
        Mode::Comment { .. } => " Ctrl+S: submit   Ctrl+C/V/X: copy/paste/cut   Shift+arrows: select   Esc: cancel".to_string(),
        Mode::Normal => match app.focused_panel {
            Panel::FileList  => " Tab/Shift+Tab: navigate  ↑↓: items  ←/→: scroll names  Enter/Space: open/toggle  q: quit".to_string(),
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
                let ws_hint = match app.whitespace_mode {
                    WhitespaceMode::None          => "  w: whitespace".to_string(),
                    WhitespaceMode::IgnoreChanges => "  w: whitespace(-b)".to_string(),
                    WhitespaceMode::IgnoreAll     => "  w: whitespace(-w)".to_string(),
                };
                format!(" Tab/Shift+Tab: navigate  ↑↓: scroll  []: hunk{}{}{}  q: quit{}", note_actions, fold_hint, ws_hint, notes_str)
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
    use super::cursor::line_spans;
    use super::SEL_BG;
    use super::diff_render::build_diff_text;
    use super::popup::render_comment_popup;
    use crate::app::{App, Mode, Panel};
    use crate::diff::{ChangedFile, DiffFile, DiffLine, FileStatus, Hunk, LineKind};
    use ratatui::{Terminal, backend::TestBackend};
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

    fn text_to_string(text: &ratatui::text::Text<'static>) -> String {
        text.lines.iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect::<String>())
            .collect::<Vec<_>>().join("\n")
    }

    fn spans_text(spans: &[ratatui::text::Span<'static>]) -> String {
        spans.iter().map(|s| s.content.as_ref()).collect()
    }

    fn has_sel_bg(span: &ratatui::text::Span<'static>) -> bool {
        span.style.bg == Some(SEL_BG)
    }

    fn diff_view_rendered(app: &App, width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render_diff_view(f, app, f.area())).unwrap();
        terminal.backend().buffer().content().iter().map(|c| c.symbol()).collect()
    }

    /// Build an App whose diff contains a single added line with `content`.
    fn app_with_single_line(content: &str) -> App {
        let files = vec![ChangedFile { path: PathBuf::from("src/main.rs"), status: FileStatus::Modified }];
        let mut app = App::new(files.clone(), "main".to_string(), "HEAD".to_string());
        app.focused_panel = Panel::DiffView;
        app.current_diff = Some(DiffFile {
            file: files[0].clone(),
            hunks: vec![Hunk {
                header: "@@ -1,1 +1,1 @@".to_string(),
                old_start: 1, new_start: 1,
                lines: vec![DiffLine {
                    old_lineno: None, new_lineno: Some(1),
                    kind: LineKind::Added, content: content.to_string(),
                }],
            }],
        });
        app
    }

    // ── Diff view wrap regression guard ──────────────────────────────────────
    //
    // These tests render the diff view in a narrow terminal and count how many
    // characters of the line content are visible.  With Wrap enabled every
    // character must appear; without Wrap only the first (panel_width - 6)
    // characters can fit before the line is clipped — the assertion fails.

    #[test]
    fn test_diff_view_wraps_long_lines_full_content_visible() {
        // Use a rare Unicode character so counts aren't polluted by decorations.
        let content = "Q".repeat(60);
        // Panel: 40 cols wide, borders=2, gutter=6 → content area = 32 cols.
        // Line total = 6 + 60 = 66 cols → wraps to 3 rows.
        // Row 1: gutter + Q*32  (38 total, padded to 40)
        // Row 2: Q*28            (padded to 40)
        // All 60 Q chars must appear somewhere in the buffer.
        let rendered = diff_view_rendered(&app_with_single_line(&content), 40, 10);
        let q_count = rendered.chars().filter(|&c| c == 'Q').count();
        assert_eq!(q_count, 60,
            "diff view wrap: all 60 Q chars should be visible; got {} \
             (likely caused by Wrap being disabled — only the first ~32 fit per row)", q_count);
    }

    #[test]
    fn test_diff_view_short_line_fits_in_one_row() {
        // A short line should still render correctly (no spurious wrapping).
        let content = "Q".repeat(20); // 6 + 20 = 26 < 40 panel width — no wrap needed
        let rendered = diff_view_rendered(&app_with_single_line(&content), 40, 10);
        let q_count = rendered.chars().filter(|&c| c == 'Q').count();
        assert_eq!(q_count, 20, "short diff line should render all 20 chars");
    }

    fn popup_rendered(app: &App) -> String {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render_comment_popup(f, app, f.area())).unwrap();
        terminal.backend().buffer().content().iter().map(|c| c.symbol()).collect()
    }

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

    fn app_diff_view() -> App { let mut a = make_app_with_hunks(1); a.focused_panel = Panel::DiffView; a }

    // ── line_spans (imported from cursor) ─────────────────────────────────────

    #[test]
    fn test_line_spans_no_selection() {
        let spans = line_spans("hello", 0, None);
        assert_eq!(spans_text(&spans), "hello");
        assert!(spans.iter().all(|s| !has_sel_bg(s)));
    }

    #[test]
    fn test_line_spans_selection_middle() {
        let spans = line_spans("hello", 0, Some((1, 4)));
        assert!(has_sel_bg(spans.iter().find(|s| s.content.as_ref() == "ell").unwrap()));
    }

    // ── build_diff_text ───────────────────────────────────────────────────────

    #[test]
    fn test_selected_hunk_has_marker() {
        assert!(text_to_string(&build_diff_text(&make_app_with_hunks(2), 1000)).contains("▶ "));
    }

    #[test]
    fn test_selecting_second_hunk_moves_marker() {
        let mut app = make_app_with_hunks(2); app.selected_hunk = 1;
        let content = text_to_string(&build_diff_text(&app, 1000));
        assert_eq!(content.matches("▶").count(), 1);
        let pos = content.find("▶").unwrap();
        assert!(content[pos + "▶ ".len()..].starts_with("@@ -11,"));
    }

    #[test]
    fn test_no_diff_shows_loading() {
        let files = vec![ChangedFile { path: PathBuf::from("src/main.rs"), status: FileStatus::Modified }];
        let app = App::new(files, "main".to_string(), "HEAD".to_string());
        assert!(text_to_string(&build_diff_text(&app, 1000)).contains("Loading"));
    }

    #[test]
    fn test_submitted_note_shown_inline() {
        let mut app = make_app_with_hunks(1);
        app.mode = Mode::Comment { hunk_idx: 0, input: "my note".to_string(), cursor: 0, original: None };
        app.submit_comment();
        assert!(text_to_string(&build_diff_text(&app, 1000)).contains("my note"));
    }

    #[test]
    fn test_inline_note_truncated() {
        let mut app = make_app_with_hunks(1);
        app.mode = Mode::Comment { hunk_idx: 0, input: "a".repeat(60), cursor: 0, original: None };
        app.submit_comment();
        let c = text_to_string(&build_diff_text(&app, 20));
        assert!(c.contains("…") && !c.contains(&"a".repeat(21)));
    }

    // ── Comment popup ─────────────────────────────────────────────────────────

    #[test]
    fn test_popup_renders_input() {
        let mut app = make_app_with_hunks(1);
        app.mode = Mode::Comment { hunk_idx: 0, input: "review text".to_string(), cursor: 0, original: None };
        assert!(popup_rendered(&app).contains("review text"));
    }

    #[test]
    fn test_popup_no_block_cursor() {
        let mut app = make_app_with_hunks(1);
        app.mode = Mode::Comment { hunk_idx: 0, input: "hello".to_string(), cursor: 3, original: None };
        assert!(!popup_rendered(&app).contains("█"));
    }

    #[test]
    fn test_popup_not_in_normal_mode() {
        assert!(!popup_rendered(&make_app_with_hunks(1)).contains("Comment"));
    }

    #[test]
    fn test_popup_help_line() {
        let mut app = make_app_with_hunks(1);
        app.mode = Mode::Comment { hunk_idx: 0, input: String::new(), cursor: 0, original: None };
        let s = popup_rendered(&app);
        assert!(s.contains("Ctrl+S") && s.contains("Esc"));
    }

    #[test]
    fn test_popup_title_has_hunk_header() {
        let mut app = make_app_with_hunks(1);
        app.mode = Mode::Comment { hunk_idx: 0, input: String::new(), cursor: 0, original: None };
        assert!(popup_rendered(&app).contains("@@"));
    }

    // ── Whitespace mode in diff title ─────────────────────────────────────────

    fn diff_title(app: &App, width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render_diff_view(f, app, f.area())).unwrap();
        // Title appears in the top border row (row 0). Collect all chars from that row.
        terminal.backend().buffer().content()[..(width as usize)]
            .iter().map(|c| c.symbol()).collect()
    }

    #[test]
    fn test_diff_title_no_ws_mode() {
        let app = make_app_with_hunks(1);
        let title = diff_title(&app, 80, 10);
        assert!(!title.contains("(-b)") && !title.contains("(-w)"),
            "no whitespace mode should show no label");
    }

    #[test]
    fn test_diff_title_shows_ws_mode_label() {
        let mut app = make_app_with_hunks(1);
        app.whitespace_mode = crate::git::WhitespaceMode::IgnoreAll;
        let title = diff_title(&app, 80, 10);
        assert!(title.contains("(-w)"), "IgnoreAll should show (-w) in title; got: {}", title);
    }

    #[test]
    fn test_diff_title_shows_ignore_changes_label() {
        let mut app = make_app_with_hunks(1);
        app.whitespace_mode = crate::git::WhitespaceMode::IgnoreChanges;
        let title = diff_title(&app, 80, 10);
        assert!(title.contains("(-b)"), "IgnoreChanges should show (-b) in title; got: {}", title);
    }

    // ── Scroll-position indicator ─────────────────────────────────────────────

    #[test]
    fn test_scroll_indicator_visible_when_content_exceeds_viewport() {
        // Build an app with many hunks so content overflows a small panel.
        let app = make_app_with_hunks(20);
        let backend = TestBackend::new(80, 12);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render_diff_view(f, &app, f.area())).unwrap();
        let buf = terminal.backend().buffer();
        // Inner right column is col 78 (border at 79). Check rows 1..11 for any scrollbar char.
        let scrollbar_chars = ["│", "█", "▐", "▌"];
        let found = (1u16..11).any(|row| {
            scrollbar_chars.contains(&buf[(78u16, row)].symbol())
        });
        assert!(found, "scrollbar should appear in the right inner column when content is taller than viewport");
    }

    #[test]
    fn test_scroll_indicator_absent_when_content_fits() {
        // Single hunk (5 lines) in a tall panel (20 rows) — no scroll needed, no indicator.
        let app = make_app_with_hunks(1);
        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render_diff_view(f, &app, f.area())).unwrap();
        let buf = terminal.backend().buffer();
        // No scrollbar in the inner right column.
        let scrollbar_chars = ["│", "█", "▐", "▌"];
        let found = (1u16..19).any(|row| {
            scrollbar_chars.contains(&buf[(78u16, row)].symbol())
        });
        assert!(!found, "scrollbar should not appear when all content fits in the viewport");
    }

    // ── Status bar ────────────────────────────────────────────────────────────

    #[test]
    fn test_status_bar_comment_mode() {
        let mut app = app_diff_view();
        app.mode = Mode::Comment { hunk_idx: 0, input: String::new(), cursor: 0, original: None };
        let text = status_bar_text(&app);
        assert!(text.contains("Ctrl+S: submit") && text.contains("Esc: cancel"));
    }

    #[test]
    fn test_status_bar_comment_mentions_clipboard() {
        let mut app = app_diff_view();
        app.mode = Mode::Comment { hunk_idx: 0, input: String::new(), cursor: 0, original: None };
        assert!(status_bar_text(&app).contains("Ctrl+C/V/X"));
    }

    #[test]
    fn test_status_bar_diff_no_notes() {
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
    fn test_status_bar_diff_shows_whitespace_hint() {
        let app = app_diff_view();
        let text = status_bar_text(&app);
        assert!(text.contains("w: whitespace"), "status bar should mention whitespace key");
    }

    #[test]
    fn test_status_bar_diff_shows_active_ws_mode() {
        let mut app = app_diff_view();
        app.whitespace_mode = WhitespaceMode::IgnoreAll;
        let text = status_bar_text(&app);
        assert!(text.contains("(-w)"), "active whitespace mode should appear in status bar");
    }

    #[test]
    fn test_status_bar_note_shows_edit_delete() {
        let mut app = app_diff_view();
        app.mode = Mode::Comment { hunk_idx: 0, input: "existing".to_string(), cursor: 0, original: None };
        app.submit_comment();
        let text = status_bar_text(&app);
        assert!(text.contains("e: edit") && text.contains("d: delete") && !text.contains("c: comment"));
    }

    // ── Notes panel ───────────────────────────────────────────────────────────

    #[test]
    fn test_notes_panel_empty() {
        let mut app = make_app_with_hunks(1); app.focused_panel = Panel::NotesView;
        assert!(notes_panel_rendered(&app).contains("No notes yet."));
    }

    #[test]
    fn test_notes_panel_short_note() {
        assert!(notes_panel_rendered(&app_with_note("short note")).contains("short note"));
    }

    #[test]
    fn test_notes_panel_long_note_truncated() {
        let long = "a".repeat(80);
        let r = notes_panel_rendered(&app_with_note(&long));
        assert!(r.contains("…") && !r.contains(&"a".repeat(75)));
    }

    #[test]
    fn test_notes_panel_expanded() {
        let long = "a".repeat(80);
        let mut app = app_with_note(&long); app.expanded_notes.insert(0);
        let r = notes_panel_rendered(&app);
        assert!(r.contains(&"a".repeat(74)) && !r.contains("…"));
    }

    #[test]
    fn test_notes_panel_selected_marker() {
        assert!(notes_panel_rendered(&app_with_note("my note")).contains("▶"));
    }

    #[test]
    fn test_notes_panel_hunk_header() {
        assert!(notes_panel_rendered(&app_with_note("note")).contains("@@"));
    }

    // ── File list / tree rendering ────────────────────────────────────────────

    fn file_list_rendered(app: &App) -> String {
        let backend = TestBackend::new(32, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render_file_list(f, app, f.area())).unwrap();
        terminal.backend().buffer().content().iter().map(|c| c.symbol()).collect()
    }

    fn app_with_tree_files() -> App {
        let files = vec![
            ChangedFile { path: PathBuf::from("src/app.rs"),  status: FileStatus::Modified },
            ChangedFile { path: PathBuf::from("src/main.rs"), status: FileStatus::Added },
        ];
        App::new(files, "main".to_string(), "HEAD".to_string())
    }

    #[test]
    fn test_file_list_status_indicator_colour_does_not_bleed_into_space() {
        // Regression: the space separator after [M] must not carry the status colour.
        // A flat (depth-0) file produces: │[M] filename...
        // Col 0 = border, 1-3 = [M], 4 = space, 5+ = filename.
        let files = vec![ChangedFile {
            path: PathBuf::from("main.rs"),
            status: FileStatus::Modified,
        }];
        let app = App::new(files, "main".to_string(), "HEAD".to_string());
        let backend = TestBackend::new(32, 5);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render_file_list(f, &app, f.area())).unwrap();
        let buf = terminal.backend().buffer();
        // Row 1 is the first list item (row 0 is the top border).
        let indicator_fg = buf[(1, 1)].fg; // '[' of [M]
        let space_fg     = buf[(4, 1)].fg; // separator space after [M]
        assert_eq!(indicator_fg, Color::Yellow, "[M] bracket must be coloured yellow for Modified");
        assert_ne!(space_fg, Color::Yellow, "separator space after [M] must not be coloured");
    }

    #[test]
    fn test_file_list_hscroll_is_full_viewport() {
        // Viewport scroll: when h_scroll > 0 the entire row content shifts left,
        // so the status indicator eventually scrolls off the left edge.
        // A depth-0 file renders as "[M] main.rs" inside the panel.
        // With h_scroll=4 the first 4 chars are gone: "[M] " disappears,
        // so col 1 (first content cell) should be 'm' not '['.
        let files = vec![ChangedFile {
            path: PathBuf::from("main.rs"),
            status: FileStatus::Modified,
        }];
        let mut app = App::new(files, "main".to_string(), "HEAD".to_string());
        app.file_list_h_scroll = 4; // skip "[M] "

        let backend = TestBackend::new(32, 5);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render_file_list(f, &app, f.area())).unwrap();
        let buf = terminal.backend().buffer();
        // Col 0 is the panel border; col 1 is the first content column.
        assert_eq!(buf[(1, 1)].symbol(), "m", "after scrolling past '[M] ', first char should be 'm' from 'main.rs'");
    }

    #[test]
    fn test_file_list_renders_dir_arrow() {
        let app = app_with_tree_files();
        let s = file_list_rendered(&app);
        assert!(s.contains("▾"), "expanded dir should show ▾ arrow");
    }

    #[test]
    fn test_file_list_renders_dir_name() {
        let app = app_with_tree_files();
        assert!(file_list_rendered(&app).contains("src/"));
    }

    #[test]
    fn test_file_list_renders_file_with_status() {
        let app = app_with_tree_files();
        let s = file_list_rendered(&app);
        assert!(s.contains("app.rs") && s.contains("main.rs"));
    }

    #[test]
    fn test_file_list_collapsed_dir_hides_files() {
        let mut app = app_with_tree_files();
        // Collapse src/ by toggling cursor at 0
        app.file_tree_cursor = 0;
        app.toggle_dir_at_cursor();
        let s = file_list_rendered(&app);
        assert!(s.contains("▸"), "collapsed dir should show ▸ arrow");
        assert!(!s.contains("app.rs"), "files should be hidden when dir is collapsed");
    }

    #[test]
    fn test_file_list_note_marker_on_file() {
        let mut app = app_with_tree_files();
        app.notes.push(FeedbackNote {
            file: PathBuf::from("src/app.rs"),
            hunk_header: "@@".to_string(),
            hunk_content: String::new(),
            note: "check this".to_string(),
        });
        assert!(file_list_rendered(&app).contains("●"));
    }

    #[test]
    fn test_status_bar_file_list_mentions_toggle() {
        let mut app = app_with_tree_files();
        app.focused_panel = Panel::FileList;
        assert!(status_bar_text(&app).contains("toggle"));
    }

    #[test]
    fn test_status_bar_file_list_mentions_scroll() {
        let mut app = app_with_tree_files();
        app.focused_panel = Panel::FileList;
        assert!(status_bar_text(&app).contains("scroll"));
    }

    #[test]
    fn test_viewport_hscroll_skips_and_caps() {
        // Single span, basic skip and cap behaviour.
        let plain = Style::default();
        let result = viewport_hscroll(vec![("hello world".into(), plain)], 6, 5);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].content, "world");

        // Skip past end of first span into second.
        let yellow = Style::default().fg(Color::Yellow);
        let spans = vec![("[M]".into(), yellow), (" main.rs".into(), plain)];
        let result = viewport_hscroll(spans, 4, 20);
        assert_eq!(result.len(), 1, "first span fully skipped");
        assert_eq!(result[0].content, "main.rs");
        assert_eq!(result[0].style, plain, "remaining content has plain style");

        // skip == 0: all content returned up to max_width.
        let result = viewport_hscroll(vec![("hello".into(), plain)], 0, 3);
        assert_eq!(result[0].content, "hel");
    }
}
