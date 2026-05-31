use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers, MouseEventKind};

use super::app::{App, Screen};

// ── Text wrapping & cursor utilities ──

/// Sum of Unicode display widths for a string slice.
pub fn visual_width(s: &str) -> usize {
    s.chars()
        .map(|c| unicode_width::UnicodeWidthChar::width(c).unwrap_or(1))
        .sum()
}

/// Wrap `text` into visual lines of at most `width` display columns.
/// Returns a vector of `(start_byte, end_byte)` for each line.
/// Hard `\n` characters start a new line and are NOT included in ranges.
pub fn wrap_text(text: &str, width: u16) -> Vec<(usize, usize)> {
    if width == 0 {
        return if text.is_empty() { vec![(0, 0)] } else { vec![(0, text.len())] };
    }
    let w = width as usize;
    let mut lines: Vec<(usize, usize)> = Vec::new();
    let mut start = 0usize;
    let mut col = 0usize;

    for (idx, c) in text.char_indices() {
        let cw = unicode_width::UnicodeWidthChar::width(c).unwrap_or(1);
        if c == '\n' {
            lines.push((start, idx)); // newline itself is not part of the line
            start = idx + 1;
            col = 0;
        } else if col + cw > w && col > 0 {
            lines.push((start, idx)); // wrap before this char
            start = idx;
            col = cw;
        } else {
            col += cw;
        }
    }
    lines.push((start, text.len()));
    lines
}

/// Map a byte-index cursor into a visual (row, col) position.
pub fn byte_index_to_visual_pos(text: &str, cursor: usize, width: u16) -> (usize, usize) {
    let lines = wrap_text(text, width);
    for (row, (s, e)) in lines.iter().enumerate() {
        if cursor >= *s && cursor < *e {
            let col = visual_width(&text[*s..cursor]);
            return (row, col);
        }
    }
    // Cursor is at or past the end of the last line.
    let last_row = lines.len().saturating_sub(1);
    let last = lines.get(last_row).copied().unwrap_or((0, 0));
    let col = visual_width(&text[last.0..cursor.min(text.len())]);
    (last_row, col)
}

/// Convert a visual (row, col) click into a byte index inside `text`.
pub fn visual_pos_to_byte_index(text: &str, row: usize, col: usize, width: u16) -> usize {
    let lines = wrap_text(text, width);
    let (start, end) = lines.get(row).copied().unwrap_or((text.len(), text.len()));
    let mut current_col = 0usize;
    for (idx, c) in text[start..end].char_indices() {
        let cw = unicode_width::UnicodeWidthChar::width(c).unwrap_or(1);
        if current_col + cw > col {
            return start + idx;
        }
        current_col += cw;
    }
    end
}

/// Ensure the cursor is visible by adjusting `task_scroll`.
fn auto_scroll(app: &mut App, width: u16) {
    let (row, _) = byte_index_to_visual_pos(&app.task_input, app.task_cursor, width);
    let visible_height = app
        .task_input_rect
        .map(|r| r.height as usize)
        .unwrap_or(1);
    if row < app.task_scroll {
        app.task_scroll = row;
    } else if row >= app.task_scroll + visible_height {
        app.task_scroll = row.saturating_sub(visible_height - 1);
    }
}

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
    let width = app.task_input_rect.map(|r| r.width).unwrap_or(40);

    match code {
        KeyCode::Char(c) if !ctrl => {
            let pos = app.task_cursor.min(app.task_input.len());
            app.task_input.insert(pos, c);
            app.task_cursor = (pos + c.len_utf8()).min(app.task_input.len());
            app.task_input_focused = true;
            auto_scroll(app, width);
        }
        KeyCode::Backspace if ctrl => {
            let pos = app.task_cursor;
            let prev = prev_word_boundary(&app.task_input, pos);
            app.task_input.replace_range(prev..pos, "");
            app.task_cursor = prev;
            app.task_input_focused = true;
            auto_scroll(app, width);
        }
        KeyCode::Backspace => {
            if app.task_cursor > 0 {
                let prev = prev_char_boundary(&app.task_input, app.task_cursor);
                app.task_input.replace_range(prev..app.task_cursor, "");
                app.task_cursor = prev;
            }
            app.task_input_focused = true;
            auto_scroll(app, width);
        }
        KeyCode::Delete if ctrl => {
            let pos = app.task_cursor;
            let next = next_word_boundary(&app.task_input, pos);
            app.task_input.replace_range(pos..next, "");
            app.task_input_focused = true;
            auto_scroll(app, width);
        }
        KeyCode::Delete => {
            let next = next_char_boundary(&app.task_input, app.task_cursor);
            app.task_input.replace_range(app.task_cursor..next, "");
            app.task_input_focused = true;
            auto_scroll(app, width);
        }
        KeyCode::Left if ctrl => {
            app.task_cursor = prev_word_boundary(&app.task_input, app.task_cursor);
            app.task_input_focused = true;
            auto_scroll(app, width);
        }
        KeyCode::Left => {
            app.task_cursor = prev_char_boundary(&app.task_input, app.task_cursor);
            app.task_input_focused = true;
            auto_scroll(app, width);
        }
        KeyCode::Right if ctrl => {
            app.task_cursor = next_word_boundary(&app.task_input, app.task_cursor);
            app.task_input_focused = true;
            auto_scroll(app, width);
        }
        KeyCode::Right => {
            app.task_cursor = next_char_boundary(&app.task_input, app.task_cursor);
            app.task_input_focused = true;
            auto_scroll(app, width);
        }
        KeyCode::Up => {
            let (row, col) = byte_index_to_visual_pos(&app.task_input, app.task_cursor, width);
            if row > 0 {
                app.task_cursor = visual_pos_to_byte_index(&app.task_input, row - 1, col, width);
            } else {
                app.task_cursor = 0;
            }
            app.task_input_focused = true;
            auto_scroll(app, width);
        }
        KeyCode::Down => {
            let lines = wrap_text(&app.task_input, width);
            let (row, col) = byte_index_to_visual_pos(&app.task_input, app.task_cursor, width);
            if row + 1 < lines.len() {
                app.task_cursor = visual_pos_to_byte_index(&app.task_input, row + 1, col, width);
            } else {
                app.task_cursor = app.task_input.len();
            }
            app.task_input_focused = true;
            auto_scroll(app, width);
        }
        KeyCode::Home => {
            let (row, _) = byte_index_to_visual_pos(&app.task_input, app.task_cursor, width);
            let lines = wrap_text(&app.task_input, width);
            if let Some((s, _)) = lines.get(row) {
                app.task_cursor = *s;
            }
            app.task_input_focused = true;
            auto_scroll(app, width);
        }
        KeyCode::End => {
            let (row, _) = byte_index_to_visual_pos(&app.task_input, app.task_cursor, width);
            let lines = wrap_text(&app.task_input, width);
            if let Some((_, e)) = lines.get(row) {
                app.task_cursor = *e;
            }
            app.task_input_focused = true;
            auto_scroll(app, width);
        }
        KeyCode::Enter if ctrl => {
            app.execute_task();
            app.task_input.clear();
            app.task_cursor = 0;
            app.task_scroll = 0;
        }
        KeyCode::Enter => {
            let pos = app.task_cursor.min(app.task_input.len());
            app.task_input.insert(pos, '\n');
            app.task_cursor = pos + 1;
            app.task_input_focused = true;
            auto_scroll(app, width);
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
                let click_row = (row as usize)
                    .saturating_sub(rect.y as usize)
                    + app.task_scroll;
                let click_col = col.saturating_sub(rect.x) as usize;
                app.task_cursor =
                    visual_pos_to_byte_index(&app.task_input, click_row, click_col, rect.width);
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


