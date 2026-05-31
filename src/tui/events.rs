use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers, MouseEventKind};

use super::app::{App, Screen};

/// Poll for an event and update the app state.
/// Returns true if the app should continue running.
pub fn handle_event(app: &mut App) -> std::io::Result<bool> {
    if !event::poll(std::time::Duration::from_millis(50))? {
        return Ok(true);
    }

    match event::read()? {
        Event::Key(key) if key.kind == KeyEventKind::Press => {
            if handle_key(app, key.code, key.modifiers) {
                return Ok(true);
            }
        }
        Event::Mouse(mouse) => {
            handle_mouse(app, mouse.kind, mouse.column, mouse.row);
        }
        Event::Resize(_, _) => {}
        _ => {}
    }

    Ok(!app.should_quit)
}

/// Handle a key press. Returns true to continue, false to quit.
fn handle_key(app: &mut App, code: KeyCode, modifiers: KeyModifiers) -> bool {
    // Global quit.
    if code == KeyCode::Char('c') && modifiers.contains(KeyModifiers::CONTROL) {
        app.should_quit = true;
        return false;
    }

    // Tab / BackTab always navigate between screens.
    match code {
        KeyCode::Tab => { app.next_screen(); return true; }
        KeyCode::BackTab => { app.prev_screen(); return true; }
        _ => {}
    }

    // Digits 1-5 navigate unless the task input is focused (so users can type numbers).
    let consumed = if app.screen == Screen::Task && app.task_input_focused {
        false
    } else {
        match code {
            KeyCode::Char('1') => { app.screen = Screen::Dashboard; true }
            KeyCode::Char('2') => { app.screen = Screen::Task; true }
            KeyCode::Char('3') => { app.screen = Screen::Files; true }
            KeyCode::Char('4') => { app.screen = Screen::Logs; true }
            KeyCode::Char('5') => { app.screen = Screen::Graph; true }
            _ => false,
        }
    };
    if consumed {
        return true;
    }

    // Screen-specific input.
    match app.screen {
        Screen::Task => handle_task_keys(app, code, modifiers),
        Screen::Logs => handle_log_keys(app, code),
        Screen::Files => handle_file_keys(app, code),
        _ => {}
    }

    true
}

fn handle_task_keys(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
    if app.running {
        return; // Block input while running.
    }

    let ctrl = modifiers.contains(KeyModifiers::CONTROL);

    match code {
        KeyCode::Char(c) if !ctrl => {
            let pos = app.task_cursor.min(app.task_input.len());
            app.task_input.insert(pos, c);
            app.task_cursor = (pos + c.len_utf8()).min(app.task_input.len());
            app.task_input_focused = true;
        }
        KeyCode::Backspace if ctrl => {
            // Delete word before cursor.
            let pos = app.task_cursor;
            let prev = prev_word_boundary(&app.task_input, pos);
            app.task_input.replace_range(prev..pos, "");
            app.task_cursor = prev;
            app.task_input_focused = true;
        }
        KeyCode::Backspace => {
            if app.task_cursor > 0 {
                let prev = prev_char_boundary(&app.task_input, app.task_cursor);
                app.task_input.replace_range(prev..app.task_cursor, "");
                app.task_cursor = prev;
            }
            app.task_input_focused = true;
        }
        KeyCode::Delete if ctrl => {
            // Delete word after cursor.
            let pos = app.task_cursor;
            let next = next_word_boundary(&app.task_input, pos);
            app.task_input.replace_range(pos..next, "");
            app.task_input_focused = true;
        }
        KeyCode::Delete => {
            let next = next_char_boundary(&app.task_input, app.task_cursor);
            app.task_input.replace_range(app.task_cursor..next, "");
            app.task_input_focused = true;
        }
        KeyCode::Left if ctrl => {
            app.task_cursor = prev_word_boundary(&app.task_input, app.task_cursor);
            app.task_input_focused = true;
        }
        KeyCode::Left => {
            app.task_cursor = prev_char_boundary(&app.task_input, app.task_cursor);
            app.task_input_focused = true;
        }
        KeyCode::Right if ctrl => {
            app.task_cursor = next_word_boundary(&app.task_input, app.task_cursor);
            app.task_input_focused = true;
        }
        KeyCode::Right => {
            app.task_cursor = next_char_boundary(&app.task_input, app.task_cursor);
            app.task_input_focused = true;
        }
        KeyCode::Home => {
            app.task_cursor = 0;
            app.task_input_focused = true;
        }
        KeyCode::End => {
            app.task_cursor = app.task_input.len();
            app.task_input_focused = true;
        }
        KeyCode::Enter if ctrl => {
            app.execute_task();
            app.task_input.clear();
            app.task_cursor = 0;
        }
        KeyCode::Enter => {
            app.execute_task();
            app.task_input.clear();
            app.task_cursor = 0;
        }
        KeyCode::Esc => {
            app.task_input_focused = false;
        }
        _ => {}
    }
}

/// Find the start of the previous word (or start of string).
/// Find the previous char boundary (byte index) strictly before `cursor`.
fn prev_char_boundary(s: &str, cursor: usize) -> usize {
    if cursor == 0 {
        return 0;
    }
    let mut idx = cursor.saturating_sub(1);
    while idx > 0 && !s.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

/// Find the next char boundary (byte index) at or after `cursor`.
fn next_char_boundary(s: &str, cursor: usize) -> usize {
    let len = s.len();
    if cursor >= len {
        return len;
    }
    let mut idx = cursor;
    while idx < len && !s.is_char_boundary(idx) {
        idx += 1;
    }
    idx
}

fn prev_word_boundary(s: &str, cursor: usize) -> usize {
    if cursor == 0 {
        return 0;
    }
    let byte_pos = cursor.min(s.len());
    let mut chars = s[..byte_pos].char_indices().rev().peekable();
    // Skip whitespace immediately before cursor.
    while let Some((_, c)) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
        } else {
            break;
        }
    }
    // Skip the word characters.
    let mut boundary = byte_pos;
    for (idx, c) in chars {
        if c.is_whitespace() {
            boundary = idx + c.len_utf8();
            break;
        }
        boundary = idx;
    }
    boundary
}

/// Find the end of the next word (or end of string).
fn next_word_boundary(s: &str, cursor: usize) -> usize {
    let byte_pos = cursor.min(s.len());
    let mut chars = s[byte_pos..].char_indices().peekable();
    // Skip whitespace immediately after cursor.
    while let Some((_, c)) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
        } else {
            break;
        }
    }
    // Skip the word characters.
    let mut boundary = s.len();
    for (idx, c) in chars {
        if c.is_whitespace() {
            boundary = byte_pos + idx;
            break;
        }
        boundary = byte_pos + idx + c.len_utf8();
    }
    boundary
}

fn handle_log_keys(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::Up | KeyCode::Char('k') => {
            app.log_scroll = app.log_scroll.saturating_sub(1);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            app.log_scroll = (app.log_scroll + 1).min(app.logs.len().saturating_sub(1));
        }
        KeyCode::PageUp => {
            app.log_scroll = app.log_scroll.saturating_sub(10);
        }
        KeyCode::PageDown => {
            app.log_scroll = (app.log_scroll + 10).min(app.logs.len().saturating_sub(1));
        }
        KeyCode::Home => app.log_scroll = 0,
        KeyCode::End => app.log_scroll = app.logs.len().saturating_sub(1),
        _ => {}
    }
}

fn handle_file_keys(app: &mut App, code: KeyCode) {
    let mut files: Vec<String> = app.orchestrator.file_contents.keys().cloned().collect();
    if files.is_empty() {
        return;
    }
    files.sort();

    // Ensure file_scroll is in bounds before indexing.
    app.file_scroll = app.file_scroll.min(files.len().saturating_sub(1));

    match code {
        KeyCode::Up | KeyCode::Char('k') => {
            app.file_scroll = app.file_scroll.saturating_sub(1);
            app.selected_file = Some(files[app.file_scroll].clone());
        }
        KeyCode::Down | KeyCode::Char('j') => {
            app.file_scroll = (app.file_scroll + 1).min(files.len().saturating_sub(1));
            app.selected_file = Some(files[app.file_scroll].clone());
        }
        _ => {}
    }
}

// ── Mouse handling ──

fn contains(rect: ratatui::layout::Rect, col: u16, row: u16) -> bool {
    rect.x <= col && col < rect.x + rect.width && rect.y <= row && row < rect.y + rect.height
}

fn handle_mouse(app: &mut App, kind: MouseEventKind, col: u16, row: u16) {
    match kind {
        MouseEventKind::Down(_) => {
            let in_sidebar = app.sidebar_rect.is_some_and(|r| contains(r, col, row));
            let in_file_tree = app.file_tree_rect.is_some_and(|r| contains(r, col, row));
            let in_task_input = app.task_input_rect.is_some_and(|r| contains(r, col, row));

            if in_sidebar {
                let rect = app.sidebar_rect.unwrap();
                let list_y_start = rect.y + 4; // title (3) + gap (1)
                let idx = (row as usize).saturating_sub(list_y_start as usize);
                let screens = Screen::all();
                if idx < screens.len() {
                    app.screen = screens[idx];
                }
            } else if app.screen == Screen::Files && in_file_tree {
                let rect = app.file_tree_rect.unwrap();
                let idx = (row as usize).saturating_sub(rect.y as usize);
                let mut files: Vec<String> = app.orchestrator.file_contents.keys().cloned().collect();
                files.sort();
                if idx < files.len() {
                    app.file_scroll = idx;
                    app.selected_file = Some(files[idx].clone());
                }
            } else if app.screen == Screen::Task && in_task_input {
                let rect = app.task_input_rect.unwrap();
                app.task_input_focused = true;
                let click_x = col.saturating_sub(rect.x + 1) as usize;
                app.task_cursor = visual_offset_to_byte_index(&app.task_input, click_x);
            }
        }
        MouseEventKind::ScrollUp => {
            if app.screen == Screen::Logs {
                app.log_scroll = app.log_scroll.saturating_sub(3);
            } else if app.screen == Screen::Files {
                let mut files: Vec<String> = app.orchestrator.file_contents.keys().cloned().collect();
                files.sort();
                if app.file_scroll > 0 {
                    app.file_scroll -= 1;
                    app.selected_file = Some(files[app.file_scroll].clone());
                }
            }
        }
        MouseEventKind::ScrollDown => {
            if app.screen == Screen::Logs {
                app.log_scroll = (app.log_scroll + 3).min(app.logs.len().saturating_sub(1));
            } else if app.screen == Screen::Files {
                let mut files: Vec<String> = app.orchestrator.file_contents.keys().cloned().collect();
                files.sort();
                if app.file_scroll + 1 < files.len() {
                    app.file_scroll += 1;
                    app.selected_file = Some(files[app.file_scroll].clone());
                }
            }
        }
        _ => {}
    }
}

/// Approximate conversion from visual column to byte index in a string.
fn visual_offset_to_byte_index(s: &str, target_col: usize) -> usize {
    let mut col = 0usize;
    for (idx, c) in s.char_indices() {
        if col >= target_col {
            return idx;
        }
        col += unicode_width::UnicodeWidthChar::width(c).unwrap_or(1);
    }
    s.len()
}
