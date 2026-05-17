mod cursor;
mod diff_render;
mod popup;
mod render;
pub(crate) mod split_render;

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
    Terminal,
};

use crate::app::{App, FeedbackNote, Mode, Panel, delete_selection, selected_range};
use crate::diff::ChangedFile;
use crate::git::{GitBackend};

use cursor::{
    cursor_up_visual, cursor_down_visual,
    cursor_prev, cursor_next, cursor_home, cursor_end,
    cursor_word_left, cursor_word_right,
};
use popup::{comment_popup_content_width};

// ── Palette ───────────────────────────────────────────────────────────────────
// Constants live here because cursor.rs, diff_render.rs, and popup.rs import
// them via `super::ACCENT` etc.
pub(crate) const ACCENT: Color = Color::Cyan;
pub(crate) const MUTED:  Color = Color::Rgb(100, 110, 130);
pub(crate) const NOTE_FG:Color = Color::Rgb(100, 150, 210);
pub(crate) const SEL_BG: Color = Color::Rgb(60, 80, 140);

use ratatui::style::Color;

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
    app.select_first_tree_file();
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
    app.current_rich_diff = result.ok().map(|raw| {
        let diff = crate::diff::parse_diff(&raw, file);
        app.highlighter.enrich(&diff)
    });
    log::debug!("[ui] load_current_file: current_rich_diff={}", match &app.current_rich_diff {
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
        terminal.draw(|f| render::render(f, app))?;
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
                            let notes_h: u16 = if app.notes.is_empty() { 0 } else { 10 };
                            let vp = terminal.size().map(|r| r.height.saturating_sub(3 + notes_h) as usize).unwrap_or(20);
                            app.diff_scroll_down(vp);
                        }
                        Panel::NotesView => { app.notes_down(); app.scroll_notes_to_selected(8); }
                    },
                    KeyCode::Left if app.focused_panel == Panel::FileList => {
                        app.file_list_scroll_left();
                    }
                    KeyCode::Right if app.focused_panel == Panel::FileList => {
                        app.file_list_scroll_right();
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
                    KeyCode::Char('[') if app.focused_panel == Panel::DiffView => {
                        if let Some(prev) = app.at_first_hunk_boundary().then(|| app.prev_file_in_tree()).flatten() {
                            app.select_file(prev);
                            app.sync_tree_cursor_to_file();
                            load_current_file(app, git);
                            if let Some(ref diff) = app.current_rich_diff
                                && !diff.hunks.is_empty()
                            {
                                app.selected_hunk = diff.hunks.len() - 1;
                                app.scroll_to_selected_hunk();
                            }
                        } else {
                            app.prev_hunk();
                        }
                    }
                    KeyCode::Char(']') if app.focused_panel == Panel::DiffView => {
                        if let Some(next) = app.at_last_hunk_boundary().then(|| app.next_file_in_tree()).flatten() {
                            app.select_file(next);
                            app.sync_tree_cursor_to_file();
                            load_current_file(app, git);
                            // selected_hunk is already 0 from load_current_file
                        } else {
                            app.next_hunk();
                        }
                    }
                    KeyCode::Char('w') if app.focused_panel == Panel::DiffView => {
                        app.cycle_whitespace_mode();
                        load_current_file(app, git);
                    }
                    KeyCode::Char('s') if app.focused_panel == Panel::DiffView => {
                        app.toggle_view_mode();
                    }
                    KeyCode::Char('v') if app.focused_panel == Panel::DiffView => { app.enter_line_select(); }
                    KeyCode::Char('c') if app.focused_panel == Panel::DiffView => { app.start_comment(); }
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
                if app.focused_panel == Panel::FileList && app.current_rich_diff.is_none() {
                    load_current_file(app, git);
                }
            }

            Mode::LineSelect { .. } => {
                match key.code {
                    KeyCode::Esc        => { app.mode = crate::app::Mode::Normal; }
                    KeyCode::Up         => app.line_select_up(),
                    KeyCode::Down       => app.line_select_down(),
                    KeyCode::Char('c')  => app.start_comment_for_selection(),
                    KeyCode::Char('d')  => { app.delete_note_for_selection(); app.mode = crate::app::Mode::Normal; }
                    _ => {}
                }
            }

            Mode::Comment { mut input, hunk_idx, mut cursor, original, line_range } => {
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
                        app.mode = Mode::Comment { hunk_idx, input, cursor, original, line_range: line_range.clone() }; true
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
                        app.mode = Mode::Comment { hunk_idx, input, cursor, original, line_range: line_range.clone() }; true
                    }
                    KeyCode::Char('v') if ctrl => {
                        if let Some((ni, nc)) = delete_selection(&input, cursor, app.comment_anchor) {
                            input = ni; cursor = nc; app.comment_anchor = None;
                        }
                        if let Some(text) = clipboard_get() {
                            for c in text.chars() { input.insert(cursor, c); cursor += c.len_utf8(); }
                        }
                        app.mode = Mode::Comment { hunk_idx, input, cursor, original, line_range: line_range.clone() }; true
                    }
                    KeyCode::Enter => {
                        if let Some((ni, nc)) = delete_selection(&input, cursor, app.comment_anchor) {
                            input = ni; cursor = nc; app.comment_anchor = None;
                        }
                        input.insert(cursor, '\n'); cursor += 1;
                        app.mode = Mode::Comment { hunk_idx, input, cursor, original, line_range: line_range.clone() }; true
                    }

                    KeyCode::Up   if shift => { extend!(); cursor = cursor_up_visual(&input, cursor, cw);   app.mode = Mode::Comment { hunk_idx, input, cursor, original, line_range: line_range.clone() }; true }
                    KeyCode::Up            => { clear_sel!(); cursor = cursor_up_visual(&input, cursor, cw);   app.mode = Mode::Comment { hunk_idx, input, cursor, original, line_range: line_range.clone() }; true }
                    KeyCode::Down if shift => { extend!(); cursor = cursor_down_visual(&input, cursor, cw); app.mode = Mode::Comment { hunk_idx, input, cursor, original, line_range: line_range.clone() }; true }
                    KeyCode::Down          => { clear_sel!(); cursor = cursor_down_visual(&input, cursor, cw); app.mode = Mode::Comment { hunk_idx, input, cursor, original, line_range: line_range.clone() }; true }
                    KeyCode::Home if shift => { extend!(); cursor = cursor_home(&input, cursor); app.mode = Mode::Comment { hunk_idx, input, cursor, original, line_range: line_range.clone() }; true }
                    KeyCode::Home          => { clear_sel!(); cursor = cursor_home(&input, cursor); app.mode = Mode::Comment { hunk_idx, input, cursor, original, line_range: line_range.clone() }; true }
                    KeyCode::End  if shift => { extend!(); cursor = cursor_end(&input, cursor);  app.mode = Mode::Comment { hunk_idx, input, cursor, original, line_range: line_range.clone() }; true }
                    KeyCode::End           => { clear_sel!(); cursor = cursor_end(&input, cursor);  app.mode = Mode::Comment { hunk_idx, input, cursor, original, line_range: line_range.clone() }; true }

                    KeyCode::Left if ctrl && shift => { extend!(); cursor = cursor_word_left(&input, cursor);  app.mode = Mode::Comment { hunk_idx, input, cursor, original, line_range: line_range.clone() }; true }
                    KeyCode::Left if ctrl           => { clear_sel!(); cursor = cursor_word_left(&input, cursor);  app.mode = Mode::Comment { hunk_idx, input, cursor, original, line_range: line_range.clone() }; true }
                    KeyCode::Left if shift          => { extend!(); cursor = cursor_prev(&input, cursor);         app.mode = Mode::Comment { hunk_idx, input, cursor, original, line_range: line_range.clone() }; true }
                    KeyCode::Left                   => { clear_sel!(); cursor = cursor_prev(&input, cursor);         app.mode = Mode::Comment { hunk_idx, input, cursor, original, line_range: line_range.clone() }; true }

                    KeyCode::Right if ctrl && shift => { extend!(); cursor = cursor_word_right(&input, cursor); app.mode = Mode::Comment { hunk_idx, input, cursor, original, line_range: line_range.clone() }; true }
                    KeyCode::Right if ctrl           => { clear_sel!(); cursor = cursor_word_right(&input, cursor); app.mode = Mode::Comment { hunk_idx, input, cursor, original, line_range: line_range.clone() }; true }
                    KeyCode::Right if shift          => { extend!(); cursor = cursor_next(&input, cursor);          app.mode = Mode::Comment { hunk_idx, input, cursor, original, line_range: line_range.clone() }; true }
                    KeyCode::Right                   => { clear_sel!(); cursor = cursor_next(&input, cursor);          app.mode = Mode::Comment { hunk_idx, input, cursor, original, line_range: line_range.clone() }; true }

                    KeyCode::Backspace => {
                        if let Some((ni, nc)) = delete_selection(&input, cursor, app.comment_anchor) {
                            input = ni; cursor = nc; app.comment_anchor = None;
                        } else if cursor > 0 {
                            let prev = cursor_prev(&input, cursor);
                            input.drain(prev..cursor); cursor = prev;
                        }
                        app.mode = Mode::Comment { hunk_idx, input, cursor, original, line_range: line_range.clone() }; true
                    }
                    KeyCode::Delete => {
                        if let Some((ni, nc)) = delete_selection(&input, cursor, app.comment_anchor) {
                            input = ni; cursor = nc; app.comment_anchor = None;
                        } else if cursor < input.len() {
                            let next = cursor_next(&input, cursor);
                            input.drain(cursor..next);
                        }
                        app.mode = Mode::Comment { hunk_idx, input, cursor, original, line_range: line_range.clone() }; true
                    }
                    KeyCode::Char(c) if !ctrl => {
                        if let Some((ni, nc)) = delete_selection(&input, cursor, app.comment_anchor) {
                            input = ni; cursor = nc; app.comment_anchor = None;
                        }
                        input.insert(cursor, c); cursor += c.len_utf8();
                        app.mode = Mode::Comment { hunk_idx, input, cursor, original, line_range: line_range.clone() }; true
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

fn jump_to_note<G: GitBackend>(app: &mut App, git: &G) {
    let Some(file_idx) = app.selected_note_file_idx() else { return };
    let target_header = app.notes[app.selected_note].hunk_header.clone();
    app.expand_parents_of(file_idx);
    app.select_file(file_idx);
    app.sync_tree_cursor_to_file();
    load_current_file(app, git);
    if let Some(hunk_idx) = app.current_rich_diff.as_ref()
        .and_then(|d| d.hunks.iter().position(|h| h.header == target_header))
    {
        app.selected_hunk = hunk_idx;
        app.scroll_to_selected_hunk();
    }
    app.focused_panel = Panel::DiffView;
}
