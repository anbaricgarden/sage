use std::io::{self, Write};
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers, MouseEventKind};

use super::app::{App, Screen, SelectionSource, TextSelection};
use super::file_tree::build_visible_tree;

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

/// Write the given text to the system clipboard via the OSC 52 escape sequence.
pub fn copy_to_clipboard(text: &str) {
    use base64::{Engine as _, engine::general_purpose};
    let encoded = general_purpose::STANDARD.encode(text);
    let seq = format!("\x1b]52;c;{}\x07", encoded);
    let _ = io::stdout().write_all(seq.as_bytes());
    let _ = io::stdout().flush();
}

/// Tick the deferred-copy timer. If it reaches zero, perform the copy and clear state.
pub fn tick_deferred_copy(app: &mut App) {
    if app.copy_defer_ticks == 0 {
        return;
    }
    app.copy_defer_ticks -= 1;
    if app.copy_defer_ticks != 0 {
        return;
    }
    let Some(source) = app.pending_copy_source.take() else {
        return;
    };
    let Some(sel) = app.selection.as_ref().filter(|s| s.source == source) else {
        return;
    };
    let (start, end) = (sel.start.min(sel.end), sel.start.max(sel.end));
    let source = sel.source;
    if end <= start {
        return;
    }
    let text = match source {
        SelectionSource::TaskInput => app.task_input[start..end].to_string(),
        SelectionSource::Result => {
            app.last_result.as_ref().map(|r| r[start..end].to_string()).unwrap_or_default()
        }
        SelectionSource::FileContent => {
            app.selected_file.as_ref()
                .and_then(|f| app.orchestrator.file_contents.get(f))
                .map(|c| c[start..end].to_string())
                .unwrap_or_default()
        }
        SelectionSource::FileFilter => app.file_filter[start..end].to_string(),
    };
    app.selection = None;
    copy_to_clipboard(&text);
    app.set_status("Copied to clipboard!", crate::tui::app::StatusKind::Success);
    app.selection = Some(TextSelection { source, start, end });
    app.copy_flash_ticks = 5;
}

/// Return the normalized byte range of an active selection for a given source.
fn selection_bounds(app: &App, source: SelectionSource) -> Option<(usize, usize)> {
    app.selection.as_ref().filter(|s| s.source == source).map(|s| {
        (s.start.min(s.end), s.start.max(s.end))
    })
}

/// Delete the selection for the given source and return its start position.
/// Returns `None` if there is no matching selection.
fn delete_selection(app: &mut App, source: SelectionSource) -> Option<usize> {
    if let Some(sel) = app.selection.take() {
        if sel.source == source {
            let (start, end) = (sel.start.min(sel.end), sel.start.max(sel.end));
            match source {
                SelectionSource::TaskInput => {
                    app.task_input.replace_range(start..end, "");
                }
                SelectionSource::FileFilter => {
                    app.file_filter.replace_range(start..end, "");
                }
                _ => {}
            }
            return Some(start);
        }
        app.selection = Some(sel);
    }
    None
}

/// Compute the byte index inside `text` from a mouse click within `rect`.
fn byte_index_at_click(text: &str, rect: ratatui::layout::Rect, scroll: usize, col: u16, row: u16) -> usize {
    let click_row = (row as usize).saturating_sub(rect.y as usize) + scroll;
    let click_col = col.saturating_sub(rect.x) as usize;
    visual_pos_to_byte_index(text, click_row, click_col, rect.width)
}

/// Threshold for multi-click detection.
const MULTI_CLICK_MS: u64 = 400;
const MULTI_CLICK_DIST: u16 = 1;

/// Detect whether this click is part of a double- or triple-click sequence.
fn update_click_tracking(app: &mut App, col: u16, row: u16) -> u8 {
    let now = Instant::now();
    let (last_col, last_row) = app.last_click_pos;
    let dx = col.abs_diff(last_col);
    let dy = row.abs_diff(last_row);
    let within_time = app.last_click_time.map(|t| now.duration_since(t) < Duration::from_millis(MULTI_CLICK_MS)).unwrap_or(false);
    let within_dist = dx <= MULTI_CLICK_DIST && dy <= MULTI_CLICK_DIST;

    if within_time && within_dist {
        app.click_count = (app.click_count + 1).min(3);
    } else {
        app.click_count = 1;
    }
    app.last_click_time = Some(now);
    app.last_click_pos = (col, row);
    app.click_count
}

/// Select the word surrounding `byte_idx` in `text`.
/// Returns `(start, end)` byte indices.
fn select_word(text: &str, byte_idx: usize) -> (usize, usize) {
    let len = text.len();
    let idx = byte_idx.min(len);
    // Ensure we start at a char boundary.
    let mut start = idx;
    while start > 0 && !text.is_char_boundary(start) {
        start -= 1;
    }
    let mut end = idx;
    while end < len && !text.is_char_boundary(end) {
        end += 1;
    }

    // Expand start backward over word chars.
    let mut chars = text[..start].char_indices().rev().peekable();
    while let Some((i, c)) = chars.peek() {
        if c.is_alphanumeric() || *c == '_' {
            start = *i;
            chars.next();
        } else {
            break;
        }
    }
    // If the character at `start` is a word char, include it.
    if text[start..].chars().next().is_some_and(|c| !c.is_alphanumeric() && c != '_') {
        // The character at start is not a word char, move forward to the first word char.
        for (i, c) in text[start..].char_indices() {
            if c.is_alphanumeric() || c == '_' {
                start += i;
                break;
            }
        }
    }

    // Expand end forward over word chars.
    let mut new_end = end;
    for (i, c) in text[end..].char_indices() {
        if c.is_alphanumeric() || c == '_' {
            new_end = end + i + c.len_utf8();
        } else {
            break;
        }
    }
    (start.min(len), new_end.min(len))
}

/// Select the wrapped line containing `byte_idx`.
fn select_wrapped_line(text: &str, byte_idx: usize, width: u16) -> (usize, usize) {
    let lines = wrap_text(text, width);
    for (s, e) in lines {
        if byte_idx >= s && byte_idx <= e {
            return (s, e);
        }
    }
    (text.len(), text.len())
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
            handle_mouse(app, mouse.kind, mouse.column, mouse.row, mouse.modifiers);
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
            KeyCode::Char('6') => { app.screen = Screen::Settings; true }
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
        Screen::Files => handle_file_keys(app, code, modifiers),
        Screen::Settings => handle_settings_keys(app, code),
        _ => {}
    }

    true
}

fn handle_task_keys(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
    if app.running {
        return; // Block input while running.
    }

    let ctrl = modifiers.contains(KeyModifiers::CONTROL);
    let shift = modifiers.contains(KeyModifiers::SHIFT);
    let width = app.task_input_rect.map(|r| r.width).unwrap_or(40);

    match code {
        // ── Character insertion (replaces selection) ──
        KeyCode::Char(c) if !ctrl => {
            let pos = delete_selection(app, SelectionSource::TaskInput)
                .unwrap_or_else(|| app.task_cursor.min(app.task_input.len()));
            app.task_input.insert(pos, c);
            app.task_cursor = (pos + c.len_utf8()).min(app.task_input.len());
            app.task_input_focused = true;
            auto_scroll(app, width);
        }

        // ── Selection: Ctrl+A select all ──
        KeyCode::Char('a') if ctrl => {
            app.selection = Some(TextSelection {
                source: SelectionSource::TaskInput,
                start: 0,
                end: app.task_input.len(),
            });
            app.task_cursor = app.task_input.len();
            app.task_input_focused = true;
            auto_scroll(app, width);
        }

        // ── Backspace (deletes selection or previous char) ──
        KeyCode::Backspace if ctrl => {
            if let Some(start) = delete_selection(app, SelectionSource::TaskInput) {
                app.task_cursor = start;
            } else {
                let pos = app.task_cursor;
                let prev = prev_word_boundary(&app.task_input, pos);
                app.task_input.replace_range(prev..pos, "");
                app.task_cursor = prev;
            }
            app.task_input_focused = true;
            auto_scroll(app, width);
        }
        KeyCode::Char('w') if ctrl => {
            // Fallback for terminals that intercept Ctrl+Backspace.
            if let Some(start) = delete_selection(app, SelectionSource::TaskInput) {
                app.task_cursor = start;
            } else {
                let pos = app.task_cursor;
                let prev = prev_word_boundary(&app.task_input, pos);
                app.task_input.replace_range(prev..pos, "");
                app.task_cursor = prev;
            }
            app.task_input_focused = true;
            auto_scroll(app, width);
        }
        KeyCode::Backspace => {
            if let Some(start) = delete_selection(app, SelectionSource::TaskInput) {
                app.task_cursor = start;
            } else if app.task_cursor > 0 {
                let prev = prev_char_boundary(&app.task_input, app.task_cursor);
                app.task_input.replace_range(prev..app.task_cursor, "");
                app.task_cursor = prev;
            }
            app.task_input_focused = true;
            auto_scroll(app, width);
        }

        // ── Delete (deletes selection or next char) ──
        KeyCode::Delete if ctrl => {
            if let Some(start) = delete_selection(app, SelectionSource::TaskInput) {
                app.task_cursor = start;
            } else {
                let pos = app.task_cursor;
                let next = next_word_boundary(&app.task_input, pos);
                app.task_input.replace_range(pos..next, "");
            }
            app.task_input_focused = true;
            auto_scroll(app, width);
        }
        KeyCode::Delete => {
            if let Some(start) = delete_selection(app, SelectionSource::TaskInput) {
                app.task_cursor = start;
            } else {
                let next = next_char_boundary(&app.task_input, app.task_cursor);
                app.task_input.replace_range(app.task_cursor..next, "");
            }
            app.task_input_focused = true;
            auto_scroll(app, width);
        }

        // ── Left ──
        KeyCode::Left if ctrl && shift => {
            let new_cursor = prev_word_boundary(&app.task_input, app.task_cursor);
            extend_task_selection(app, new_cursor);
            app.task_cursor = new_cursor;
            app.task_input_focused = true;
            auto_scroll(app, width);
        }
        KeyCode::Left if ctrl => {
            if let Some((sel_start, _)) = selection_bounds(app, SelectionSource::TaskInput) {
                app.selection = None;
                app.task_cursor = sel_start;
            } else {
                app.task_cursor = prev_word_boundary(&app.task_input, app.task_cursor);
            }
            app.copy_flash_ticks = 0;
            app.task_input_focused = true;
            auto_scroll(app, width);
        }
        KeyCode::Left if shift => {
            let new_cursor = prev_char_boundary(&app.task_input, app.task_cursor);
            extend_task_selection(app, new_cursor);
            app.task_cursor = new_cursor;
            app.task_input_focused = true;
            auto_scroll(app, width);
        }
        KeyCode::Left => {
            if let Some((sel_start, _)) = selection_bounds(app, SelectionSource::TaskInput) {
                app.selection = None;
                app.task_cursor = sel_start;
            } else {
                app.task_cursor = prev_char_boundary(&app.task_input, app.task_cursor);
            }
            app.copy_flash_ticks = 0;
            app.task_input_focused = true;
            auto_scroll(app, width);
        }

        // ── Right ──
        KeyCode::Right if ctrl && shift => {
            let new_cursor = next_word_boundary(&app.task_input, app.task_cursor);
            extend_task_selection(app, new_cursor);
            app.task_cursor = new_cursor;
            app.task_input_focused = true;
            auto_scroll(app, width);
        }
        KeyCode::Right if ctrl => {
            if let Some((_, sel_end)) = selection_bounds(app, SelectionSource::TaskInput) {
                app.selection = None;
                app.task_cursor = sel_end;
            } else {
                app.task_cursor = next_word_boundary(&app.task_input, app.task_cursor);
            }
            app.copy_flash_ticks = 0;
            app.task_input_focused = true;
            auto_scroll(app, width);
        }
        KeyCode::Right if shift => {
            let new_cursor = next_char_boundary(&app.task_input, app.task_cursor);
            extend_task_selection(app, new_cursor);
            app.task_cursor = new_cursor;
            app.task_input_focused = true;
            auto_scroll(app, width);
        }
        KeyCode::Right => {
            if let Some((_, sel_end)) = selection_bounds(app, SelectionSource::TaskInput) {
                app.selection = None;
                app.task_cursor = sel_end;
            } else {
                app.task_cursor = next_char_boundary(&app.task_input, app.task_cursor);
            }
            app.copy_flash_ticks = 0;
            app.task_input_focused = true;
            auto_scroll(app, width);
        }

        // ── Up ──
        KeyCode::Up if shift => {
            let (row, col) = byte_index_to_visual_pos(&app.task_input, app.task_cursor, width);
            let new_cursor = if row > 0 {
                visual_pos_to_byte_index(&app.task_input, row - 1, col, width)
            } else {
                0
            };
            extend_task_selection(app, new_cursor);
            app.task_cursor = new_cursor;
            app.task_input_focused = true;
            auto_scroll(app, width);
        }
        KeyCode::Up => {
            app.selection = None;
            app.copy_flash_ticks = 0;
            let (row, col) = byte_index_to_visual_pos(&app.task_input, app.task_cursor, width);
            if row > 0 {
                app.task_cursor = visual_pos_to_byte_index(&app.task_input, row - 1, col, width);
            } else {
                app.task_cursor = 0;
            }
            app.task_input_focused = true;
            auto_scroll(app, width);
        }

        // ── Down ──
        KeyCode::Down if shift => {
            let lines = wrap_text(&app.task_input, width);
            let (row, col) = byte_index_to_visual_pos(&app.task_input, app.task_cursor, width);
            let new_cursor = if row + 1 < lines.len() {
                visual_pos_to_byte_index(&app.task_input, row + 1, col, width)
            } else {
                app.task_input.len()
            };
            extend_task_selection(app, new_cursor);
            app.task_cursor = new_cursor;
            app.task_input_focused = true;
            auto_scroll(app, width);
        }
        KeyCode::Down => {
            app.selection = None;
            app.copy_flash_ticks = 0;
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

        // ── Home ──
        KeyCode::Home if shift => {
            let (row, _) = byte_index_to_visual_pos(&app.task_input, app.task_cursor, width);
            let lines = wrap_text(&app.task_input, width);
            let new_cursor = lines.get(row).map(|(s, _)| *s).unwrap_or(0);
            extend_task_selection(app, new_cursor);
            app.task_cursor = new_cursor;
            app.task_input_focused = true;
            auto_scroll(app, width);
        }
        KeyCode::Home => {
            app.selection = None;
            app.copy_flash_ticks = 0;
            let (row, _) = byte_index_to_visual_pos(&app.task_input, app.task_cursor, width);
            let lines = wrap_text(&app.task_input, width);
            if let Some((s, _)) = lines.get(row) {
                app.task_cursor = *s;
            }
            app.task_input_focused = true;
            auto_scroll(app, width);
        }

        // ── End ──
        KeyCode::End if shift => {
            let (row, _) = byte_index_to_visual_pos(&app.task_input, app.task_cursor, width);
            let lines = wrap_text(&app.task_input, width);
            let new_cursor = lines.get(row).map(|(_, e)| *e).unwrap_or(app.task_input.len());
            extend_task_selection(app, new_cursor);
            app.task_cursor = new_cursor;
            app.task_input_focused = true;
            auto_scroll(app, width);
        }
        KeyCode::End => {
            app.selection = None;
            app.copy_flash_ticks = 0;
            let (row, _) = byte_index_to_visual_pos(&app.task_input, app.task_cursor, width);
            let lines = wrap_text(&app.task_input, width);
            if let Some((_, e)) = lines.get(row) {
                app.task_cursor = *e;
            }
            app.task_input_focused = true;
            auto_scroll(app, width);
        }

        // ── Newline (Shift+Enter) ──
        KeyCode::Enter if shift => {
            let pos = delete_selection(app, SelectionSource::TaskInput)
                .unwrap_or_else(|| app.task_cursor.min(app.task_input.len()));
            app.task_input.insert(pos, '\n');
            app.task_cursor = pos + 1;
            app.task_input_focused = true;
            auto_scroll(app, width);
        }

        // ── Submit (Enter) ──
        KeyCode::Enter => {
            delete_selection(app, SelectionSource::TaskInput);
            app.execute_task();
            app.task_input.clear();
            app.task_cursor = 0;
            app.task_scroll = 0;
            app.selection = None;
            app.copy_flash_ticks = 0;
        }

        KeyCode::PageUp => {
            app.result_scroll = app.result_scroll.saturating_sub(10);
        }
        KeyCode::PageDown => {
            if let Some(result) = &app.last_result {
                let total = wrap_text(result, app.result_rect.map(|r| r.width).unwrap_or(40)).len();
                app.result_scroll = (app.result_scroll + 10).min(total.saturating_sub(1));
            }
        }
        KeyCode::Esc => {
            app.task_input_focused = false;
            app.selection = None;
            app.copy_flash_ticks = 0;
        }
        _ => {}
    }
}

/// Extend or create the task-input selection so that its active end becomes `new_cursor`.
fn extend_task_selection(app: &mut App, new_cursor: usize) {
    if let Some(sel) = app.selection.as_mut().filter(|s| s.source == SelectionSource::TaskInput) {
        sel.end = new_cursor;
    } else {
        app.selection = Some(TextSelection {
            source: SelectionSource::TaskInput,
            start: app.task_cursor,
            end: new_cursor,
        });
    }
}

/// Extend or create the file-filter selection so that its active end becomes `new_cursor`.
fn extend_file_filter_selection(app: &mut App, new_cursor: usize) {
    if let Some(sel) = app.selection.as_mut().filter(|s| s.source == SelectionSource::FileFilter) {
        sel.end = new_cursor;
    } else {
        app.selection = Some(TextSelection {
            source: SelectionSource::FileFilter,
            start: app.file_filter_cursor,
            end: new_cursor,
        });
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

fn handle_settings_keys(app: &mut App, code: KeyCode) {
    const SETTINGS_COUNT: usize = 5;
    match code {
        KeyCode::Up | KeyCode::Char('k') => {
            app.settings_cursor = app.settings_cursor.saturating_sub(1);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            app.settings_cursor = (app.settings_cursor + 1).min(SETTINGS_COUNT - 1);
        }
        KeyCode::Enter | KeyCode::Char(' ') => {
            match app.settings_cursor {
                0 => app.toggle_animation_speed(),
                1 => app.mouse_enabled = !app.mouse_enabled,
                2 => app.toggle_log_filter(),
                3 => app.toggle_theme(),
                4 => app.toggle_copy_defer_duration(),
                _ => {}
            }
        }
        _ => {}
    }
}

fn handle_file_keys(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
    if app.file_filter_focused {
        handle_file_filter_keys(app, code, modifiers);
        return;
    }

    match code {
        KeyCode::PageUp => {
            app.file_content_scroll = app.file_content_scroll.saturating_sub(10);
            app.selection = None;
            app.copy_flash_ticks = 0;
            return;
        }
        KeyCode::PageDown => {
            if let Some(content) = app.selected_file.as_ref().and_then(|f| app.orchestrator.file_contents.get(f)) {
                let total = wrap_text(content, app.file_content_rect.map(|r| r.width).unwrap_or(40)).len();
                app.file_content_scroll = (app.file_content_scroll + 10).min(total.saturating_sub(1));
            }
            app.selection = None;
            app.copy_flash_ticks = 0;
            return;
        }
        _ => {}
    }

    let paths: Vec<String> = app.orchestrator.file_contents.keys().cloned().collect();
    let visible = build_visible_tree(&paths, &app.expanded_dirs, &app.file_filter);
    if visible.is_empty() {
        return;
    }
    app.file_scroll = app.file_scroll.min(visible.len().saturating_sub(1));

    match code {
        KeyCode::Up | KeyCode::Char('k') => {
            app.file_scroll = app.file_scroll.saturating_sub(1);
            if let Some(entry) = visible.get(app.file_scroll) && !entry.is_dir {
                app.selected_file = Some(entry.path.clone());
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            app.file_scroll = (app.file_scroll + 1).min(visible.len().saturating_sub(1));
            if let Some(entry) = visible.get(app.file_scroll) && !entry.is_dir {
                app.selected_file = Some(entry.path.clone());
            }
        }
        KeyCode::Left | KeyCode::Char('h') => {
            if let Some(entry) = visible.get(app.file_scroll) && entry.is_dir && entry.is_expanded {
                app.expanded_dirs.remove(&entry.path);
            }
        }
        KeyCode::Right | KeyCode::Char('l') => {
            if let Some(entry) = visible.get(app.file_scroll) && entry.is_dir && !entry.is_expanded {
                app.expanded_dirs.insert(entry.path.clone());
            }
        }
        KeyCode::Enter => {
            if let Some(entry) = visible.get(app.file_scroll) {
                if entry.is_dir {
                    if entry.is_expanded {
                        app.expanded_dirs.remove(&entry.path);
                    } else {
                        app.expanded_dirs.insert(entry.path.clone());
                    }
                } else {
                    app.selected_file = Some(entry.path.clone());
                }
            }
        }
        KeyCode::Char('/') => {
            app.file_filter_focused = true;
        }
        _ => {}
    }
}

fn handle_file_filter_keys(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
    let shift = modifiers.contains(KeyModifiers::SHIFT);
    let ctrl = modifiers.contains(KeyModifiers::CONTROL);

    match code {
        // ── Character insertion (replaces selection) ──
        KeyCode::Char(c) if !ctrl => {
            let pos = delete_selection(app, SelectionSource::FileFilter)
                .unwrap_or_else(|| app.file_filter_cursor.min(app.file_filter.len()));
            app.file_filter.insert(pos, c);
            app.file_filter_cursor = (pos + c.len_utf8()).min(app.file_filter.len());
        }

        // ── Select all ──
        KeyCode::Char('a') if ctrl => {
            app.selection = Some(TextSelection {
                source: SelectionSource::FileFilter,
                start: 0,
                end: app.file_filter.len(),
            });
            app.file_filter_cursor = app.file_filter.len();
        }

        // ── Backspace ──
        KeyCode::Backspace => {
            if let Some(start) = delete_selection(app, SelectionSource::FileFilter) {
                app.file_filter_cursor = start;
            } else if app.file_filter_cursor > 0 {
                let prev = prev_char_boundary(&app.file_filter, app.file_filter_cursor);
                app.file_filter.replace_range(prev..app.file_filter_cursor, "");
                app.file_filter_cursor = prev;
            }
        }

        // ── Delete ──
        KeyCode::Delete => {
            if let Some(start) = delete_selection(app, SelectionSource::FileFilter) {
                app.file_filter_cursor = start;
            } else {
                let next = next_char_boundary(&app.file_filter, app.file_filter_cursor);
                app.file_filter.replace_range(app.file_filter_cursor..next, "");
            }
        }

        // ── Left ──
        KeyCode::Left if shift => {
            let new_cursor = prev_char_boundary(&app.file_filter, app.file_filter_cursor);
            extend_file_filter_selection(app, new_cursor);
            app.file_filter_cursor = new_cursor;
        }
        KeyCode::Left => {
            if let Some((sel_start, _)) = selection_bounds(app, SelectionSource::FileFilter) {
                app.selection = None;
                app.file_filter_cursor = sel_start;
            } else {
                app.file_filter_cursor = prev_char_boundary(&app.file_filter, app.file_filter_cursor);
            }
        }

        // ── Right ──
        KeyCode::Right if shift => {
            let new_cursor = next_char_boundary(&app.file_filter, app.file_filter_cursor);
            extend_file_filter_selection(app, new_cursor);
            app.file_filter_cursor = new_cursor;
        }
        KeyCode::Right => {
            if let Some((_, sel_end)) = selection_bounds(app, SelectionSource::FileFilter) {
                app.selection = None;
                app.file_filter_cursor = sel_end;
            } else {
                app.file_filter_cursor = next_char_boundary(&app.file_filter, app.file_filter_cursor);
            }
        }

        KeyCode::Home if shift => {
            extend_file_filter_selection(app, 0);
            app.file_filter_cursor = 0;
        }
        KeyCode::Home => {
            app.selection = None;
            app.file_filter_cursor = 0;
        }

        KeyCode::End if shift => {
            let len = app.file_filter.len();
            extend_file_filter_selection(app, len);
            app.file_filter_cursor = len;
        }
        KeyCode::End => {
            app.selection = None;
            app.file_filter_cursor = app.file_filter.len();
        }

        KeyCode::Esc => {
            app.file_filter_focused = false;
            app.file_filter.clear();
            app.file_filter_cursor = 0;
            app.selection = None;
            app.copy_flash_ticks = 0;
        }
        KeyCode::Enter => {
            app.file_filter_focused = false;
            app.selection = None;
            app.copy_flash_ticks = 0;
        }
        _ => {}
    }

    // Re-clamp scroll after filter changes.
    let paths: Vec<String> = app.orchestrator.file_contents.keys().cloned().collect();
    let visible = build_visible_tree(&paths, &app.expanded_dirs, &app.file_filter);
    app.file_scroll = app.file_scroll.min(visible.len().saturating_sub(1));
    if let Some(entry) = visible.get(app.file_scroll) && !entry.is_dir {
        app.selected_file = Some(entry.path.clone());
    }
}

// ── Mouse handling ──

fn contains(rect: ratatui::layout::Rect, col: u16, row: u16) -> bool {
    rect.x <= col && col < rect.x + rect.width && rect.y <= row && row < rect.y + rect.height
}

fn handle_mouse(app: &mut App, kind: MouseEventKind, col: u16, row: u16, modifiers: KeyModifiers) {
    let in_sidebar = app.sidebar_rect.is_some_and(|r| contains(r, col, row));
    let in_file_tree = app.file_tree_rect.is_some_and(|r| contains(r, col, row));
    let in_task_input = app.task_input_rect.is_some_and(|r| contains(r, col, row));
    let in_settings = app.settings_rect.is_some_and(|r| contains(r, col, row));
    let in_result = app.result_rect.is_some_and(|r| contains(r, col, row));
    let in_file_content = app.file_content_rect.is_some_and(|r| contains(r, col, row));

    match kind {
        MouseEventKind::Down(_) => {
            handle_mouse_down(app, col, row, in_sidebar, in_file_tree, in_task_input, in_settings, in_result, in_file_content, modifiers);
        }
        MouseEventKind::Drag(_) => {
            handle_mouse_drag(app, col, row);
        }
        MouseEventKind::Up(_) => {
            handle_mouse_up(app);
        }
        MouseEventKind::Moved => {
            handle_mouse_moved(app, col, row, in_sidebar, in_file_tree, in_settings);
        }
        MouseEventKind::ScrollUp => {
            handle_mouse_scroll(app, in_file_tree, in_file_content, in_result, -1);
        }
        MouseEventKind::ScrollDown => {
            handle_mouse_scroll(app, in_file_tree, in_file_content, in_result, 1);
        }
        _ => {}
    }
}

/// Handle mouse button down: start text selection, navigate UI, or place cursor.
#[allow(clippy::too_many_arguments)]
fn handle_mouse_down(
    app: &mut App,
    col: u16,
    row: u16,
    in_sidebar: bool,
    in_file_tree: bool,
    in_task_input: bool,
    in_settings: bool,
    in_result: bool,
    in_file_content: bool,
    modifiers: KeyModifiers,
) {
    // Cancel any pending deferred copy when the user clicks again.
    app.copy_defer_ticks = 0;
    app.pending_copy_source = None;

    let clicks = update_click_tracking(app, col, row);

    // Start a new selection if clicking inside a text area.
    let started_selection = start_text_selection(app, col, row, clicks, in_task_input, in_result, in_file_content, modifiers);

    if !started_selection {
        // Click outside any text area: clear selection.
        app.selection = None;
        app.copy_flash_ticks = 0;
        app.click_count = 0;
    }

    // Existing click handlers (sidebar, file tree, settings, filter, task input cursor).
    if in_sidebar {
        let rect = app.sidebar_rect.unwrap();
        let idx = (row as usize).saturating_sub(rect.y as usize);
        let screens = Screen::all();
        if idx < screens.len() {
            app.screen = screens[idx];
        }
    } else if app.screen == Screen::Files && in_file_tree {
        let rect = app.file_tree_rect.unwrap();
        let idx = (row as usize).saturating_sub(rect.y as usize);
        let paths: Vec<String> = app.orchestrator.file_contents.keys().cloned().collect();
        let visible = build_visible_tree(&paths, &app.expanded_dirs, &app.file_filter);
        if idx < visible.len() {
            app.file_scroll = idx;
            let entry = &visible[idx];
            if entry.is_dir {
                if entry.is_expanded {
                    app.expanded_dirs.remove(&entry.path);
                } else {
                    app.expanded_dirs.insert(entry.path.clone());
                }
            } else {
                app.selected_file = Some(entry.path.clone());
            }
        }
    } else if app.screen == Screen::Settings && in_settings {
        let rect = app.settings_rect.unwrap();
        let idx = (row as usize).saturating_sub(rect.y as usize);
        const SETTINGS_COUNT: usize = 5;
        if idx < SETTINGS_COUNT {
            app.settings_cursor = idx;
            app.settings_hover = Some(idx);
            match idx {
                0 => app.toggle_animation_speed(),
                1 => app.mouse_enabled = !app.mouse_enabled,
                2 => app.toggle_log_filter(),
                3 => app.toggle_theme(),
                4 => app.toggle_copy_defer_duration(),
                _ => {}
            }
        }
    }
}

/// Compute selection bounds for a mouse click, extending from an existing anchor when Shift is held.
#[allow(clippy::too_many_arguments)]
fn selection_bounds_for_click(
    app: &App,
    source: SelectionSource,
    text: &str,
    idx: usize,
    width: u16,
    clicks: u8,
    shift: bool,
    fallback_anchor: usize,
) -> (usize, usize) {
    if !shift {
        return select_range(text, idx, width, clicks);
    }
    if let Some(sel) = app.selection.as_ref().filter(|s| s.source == source) {
        let anchor = sel.start;
        let end = match clicks {
            3 => {
                let (line_start, line_end) = select_wrapped_line(text, idx, width);
                if anchor <= idx { line_end } else { line_start }
            }
            2 => {
                let (word_start, word_end) = select_word(text, idx);
                if anchor <= idx { word_end } else { word_start }
            }
            _ => idx,
        };
        (anchor, end)
    } else {
        // No existing selection of this source: extend from fallback anchor.
        let (_, end) = select_range(text, idx, width, clicks);
        (fallback_anchor, end)
    }
}

/// Start a text selection on mouse down. Returns true if a selection was started.
#[allow(clippy::too_many_arguments)]
fn start_text_selection(
    app: &mut App,
    col: u16,
    row: u16,
    clicks: u8,
    in_task_input: bool,
    in_result: bool,
    in_file_content: bool,
    modifiers: KeyModifiers,
) -> bool {
    let shift = modifiers.contains(KeyModifiers::SHIFT);
    if app.screen == Screen::Task && in_task_input {
        let rect = app.task_input_rect.unwrap();
        let idx = byte_index_at_click(&app.task_input, rect, app.task_scroll, col, row);
        let (start, end) = selection_bounds_for_click(
            app, SelectionSource::TaskInput, &app.task_input, idx, rect.width, clicks, shift, app.task_cursor,
        );
        app.selection = Some(TextSelection {
            source: SelectionSource::TaskInput,
            start,
            end,
        });
        app.task_cursor = end;
        app.task_input_focused = true;
        app.copy_flash_ticks = 0;
        return true;
    }

    if app.screen == Screen::Task && in_result && let Some(result) = &app.last_result {
        let rect = app.result_rect.unwrap();
        let idx = byte_index_at_click(result, rect, app.result_scroll, col, row);
        let (start, end) = selection_bounds_for_click(
            app, SelectionSource::Result, result, idx, rect.width, clicks, shift, 0,
        );
        app.selection = Some(TextSelection {
            source: SelectionSource::Result,
            start,
            end,
        });
        app.copy_flash_ticks = 0;
        return true;
    }

    if app.screen == Screen::Files && in_file_content && let Some(content) = app.selected_file.as_ref().and_then(|f| app.orchestrator.file_contents.get(f)) {
        let rect = app.file_content_rect.unwrap();
        let idx = byte_index_at_click(content, rect, app.file_content_scroll, col, row);
        let (start, end) = selection_bounds_for_click(
            app, SelectionSource::FileContent, content, idx, rect.width, clicks, shift, 0,
        );
        app.selection = Some(TextSelection {
            source: SelectionSource::FileContent,
            start,
            end,
        });
        app.copy_flash_ticks = 0;
        return true;
    }

    if app.screen == Screen::Files && app.file_filter_rect.is_some_and(|r| contains(r, col, row)) {
        let rect = app.file_filter_rect.unwrap();
        let click_col = col.saturating_sub(rect.x) as usize;
        let idx = click_col.min(app.file_filter.len());
        let (start, end) = if shift && app.selection.as_ref().is_some_and(|s| s.source == SelectionSource::FileFilter) {
            let anchor = app.selection.as_ref().unwrap().start;
            let end = match clicks {
                3 => if anchor <= idx { app.file_filter.len() } else { 0 },
                2 => {
                    let (word_start, word_end) = select_word(&app.file_filter, idx);
                    if anchor <= idx { word_end } else { word_start }
                }
                _ => idx,
            };
            (anchor, end)
        } else if shift {
            let end = match clicks {
                3 => if app.file_filter_cursor <= idx { app.file_filter.len() } else { 0 },
                2 => {
                    let (word_start, word_end) = select_word(&app.file_filter, idx);
                    if app.file_filter_cursor <= idx { word_end } else { word_start }
                }
                _ => idx,
            };
            (app.file_filter_cursor, end)
        } else {
            match clicks {
                3 => (0, app.file_filter.len()),
                2 => select_word(&app.file_filter, idx),
                _ => (idx, idx),
            }
        };
        app.selection = Some(TextSelection {
            source: SelectionSource::FileFilter,
            start,
            end,
        });
        app.file_filter_focused = true;
        app.file_filter_cursor = end;
        app.copy_flash_ticks = 0;
        return true;
    }

    false
}

/// Select a range based on click count: triple = line, double = word, single = zero-width.
fn select_range(text: &str, idx: usize, width: u16, clicks: u8) -> (usize, usize) {
    if clicks == 3 {
        select_wrapped_line(text, idx, width)
    } else if clicks == 2 {
        select_word(text, idx)
    } else {
        (idx, idx)
    }
}

/// Handle mouse drag: extend the current text selection.
fn handle_mouse_drag(app: &mut App, col: u16, row: u16) {
    let Some(sel) = &mut app.selection else { return };
    let idx = match sel.source {
        SelectionSource::TaskInput => {
            byte_index_at_click(&app.task_input, app.task_input_rect.unwrap(), app.task_scroll, col, row)
        }
        SelectionSource::Result => {
            if let Some(result) = &app.last_result {
                byte_index_at_click(result, app.result_rect.unwrap(), app.result_scroll, col, row)
            } else {
                return;
            }
        }
        SelectionSource::FileContent => {
            if let Some(content) = app.selected_file.as_ref().and_then(|f| app.orchestrator.file_contents.get(f)) {
                byte_index_at_click(content, app.file_content_rect.unwrap(), app.file_content_scroll, col, row)
            } else {
                return;
            }
        }
        SelectionSource::FileFilter => {
            let rect = app.file_filter_rect.unwrap();
            let click_col = col.saturating_sub(rect.x) as usize;
            click_col.min(app.file_filter.len())
        }
    };
    sel.end = idx;
}

/// Handle mouse button up: copy selection or defer copy for multi-click.
fn handle_mouse_up(app: &mut App) {
    if let Some(sel) = app.selection.as_ref() {
        // Input boxes: keep selection for editing, do NOT copy.
        if sel.source == SelectionSource::TaskInput || sel.source == SelectionSource::FileFilter {
            return;
        }
        let (start, end) = (sel.start.min(sel.end), sel.start.max(sel.end));
        if end > start && app.click_count > 1 {
            // Defer copy for multi-click to avoid duplicate clipboard entries.
            app.copy_defer_ticks = app.copy_defer_duration;
            app.pending_copy_source = Some(sel.source);
            return;
        }
    }
    if let Some(sel) = app.selection.take() {
        let (start, end) = (sel.start.min(sel.end), sel.start.max(sel.end));
        if end > start {
            let text = match sel.source {
                SelectionSource::TaskInput => app.task_input[start..end].to_string(),
                SelectionSource::Result => {
                    app.last_result.as_ref().map(|r| r[start..end].to_string()).unwrap_or_default()
                }
                SelectionSource::FileContent => {
                    app.selected_file.as_ref()
                        .and_then(|f| app.orchestrator.file_contents.get(f))
                        .map(|c| c[start..end].to_string())
                        .unwrap_or_default()
                }
                SelectionSource::FileFilter => app.file_filter[start..end].to_string(),
            };
            copy_to_clipboard(&text);
            app.set_status("Copied to clipboard!", crate::tui::app::StatusKind::Success);
            // Keep selection visible for the flash duration.
            app.selection = Some(TextSelection {
                source: sel.source,
                start,
                end,
            });
            app.copy_flash_ticks = 5;
        }
    }
}

/// Handle mouse move: update hover states.
fn handle_mouse_moved(app: &mut App, _col: u16, row: u16, in_sidebar: bool, in_file_tree: bool, in_settings: bool) {
    // Clear all hover states first, then set whichever applies.
    app.sidebar_hover = None;
    app.file_hover = None;
    app.settings_hover = None;

    if in_sidebar {
        let rect = app.sidebar_rect.unwrap();
        let idx = (row as usize).saturating_sub(rect.y as usize);
        let screens = Screen::all();
        if idx < screens.len() {
            app.sidebar_hover = Some(idx);
        }
    } else if app.screen == Screen::Files && in_file_tree {
        let rect = app.file_tree_rect.unwrap();
        let idx = (row as usize).saturating_sub(rect.y as usize);
        let paths: Vec<String> = app.orchestrator.file_contents.keys().cloned().collect();
        let visible = build_visible_tree(&paths, &app.expanded_dirs, &app.file_filter);
        if idx < visible.len() {
            app.file_hover = Some(idx);
        }
    } else if app.screen == Screen::Settings && in_settings {
        let rect = app.settings_rect.unwrap();
        let idx = (row as usize).saturating_sub(rect.y as usize);
        const SETTINGS_COUNT: usize = 5;
        if idx < SETTINGS_COUNT {
            app.settings_hover = Some(idx);
        }
    }
}

/// Handle mouse scroll: direction -1 = up, 1 = down.
fn handle_mouse_scroll(
    app: &mut App,
    in_file_tree: bool,
    in_file_content: bool,
    in_result: bool,
    direction: i8,
) {
    let delta = 3usize;
    if app.screen == Screen::Logs {
        if direction < 0 {
            app.log_scroll = app.log_scroll.saturating_sub(delta);
        } else {
            app.log_scroll = (app.log_scroll + delta).min(app.logs.len().saturating_sub(1));
        }
    } else if app.screen == Screen::Files && in_file_tree {
        let paths: Vec<String> = app.orchestrator.file_contents.keys().cloned().collect();
        let visible = build_visible_tree(&paths, &app.expanded_dirs, &app.file_filter);
        if direction < 0 {
            if !visible.is_empty() && app.file_scroll > 0 {
                app.file_scroll -= 1;
                if let Some(entry) = visible.get(app.file_scroll) && !entry.is_dir {
                    app.selected_file = Some(entry.path.clone());
                }
            }
        } else if app.file_scroll + 1 < visible.len() {
            app.file_scroll += 1;
            if let Some(entry) = visible.get(app.file_scroll) && !entry.is_dir {
                app.selected_file = Some(entry.path.clone());
            }
        }
    } else if app.screen == Screen::Files && in_file_content {
        if let Some(content) = app.selected_file.as_ref().and_then(|f| app.orchestrator.file_contents.get(f)) {
            let total = wrap_text(content, app.file_content_rect.map(|r| r.width).unwrap_or(40)).len();
            if direction < 0 {
                app.file_content_scroll = app.file_content_scroll.saturating_sub(delta);
            } else {
                app.file_content_scroll = (app.file_content_scroll + delta).min(total.saturating_sub(1));
            }
        }
    } else if app.screen == Screen::Task && in_result && let Some(result) = &app.last_result {
        let total = wrap_text(result, app.result_rect.map(|r| r.width).unwrap_or(40)).len();
        if direction < 0 {
            app.result_scroll = app.result_scroll.saturating_sub(delta);
        } else {
            app.result_scroll = (app.result_scroll + delta).min(total.saturating_sub(1));
        }
    }
}

// ── Unit tests ──

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------------------
    // select_word
    // -------------------------------------------------------------------------

    #[test]
    fn select_word_basic() {
        let text = "hello world foo";
        assert_eq!(select_word(text, 2), (0, 5));   // inside "hello"
        assert_eq!(select_word(text, 8), (6, 11));  // inside "world"
        assert_eq!(select_word(text, 12), (12, 15)); // inside "foo"
    }

    #[test]
    fn select_word_click_on_whitespace() {
        let text = "hello world";
        // Clicking on the space between words selects the preceding word
        // because the backward expansion walks over "hello" before hitting the space.
        assert_eq!(select_word(text, 5), (0, 5)); // byte 5 is the space
    }

    #[test]
    fn select_word_empty() {
        assert_eq!(select_word("", 0), (0, 0));
    }

    #[test]
    fn select_word_non_word_chars() {
        let text = "   ";
        assert_eq!(select_word(text, 1), (1, 1)); // zero-width on space
    }

    #[test]
    fn select_word_unicode() {
        let text = "hello 世界 foo";
        // "世界": '世' = bytes 6..9, '界' = bytes 9..12
        assert_eq!(select_word(text, 8), (6, 12));
        assert_eq!(select_word(text, 0), (0, 5)); // "hello"
    }

    #[test]
    fn select_word_out_of_bounds() {
        let text = "abc";
        assert_eq!(select_word(text, 100), (0, 3));
    }

    // -------------------------------------------------------------------------
    // select_wrapped_line
    // -------------------------------------------------------------------------

    #[test]
    fn select_wrapped_line_basic() {
        let text = "hello world foo bar";
        // width 10: "hello worl" | "d foo bar"
        assert_eq!(select_wrapped_line(text, 0, 10), (0, 10));
        assert_eq!(select_wrapped_line(text, 5, 10), (0, 10));
        assert_eq!(select_wrapped_line(text, 12, 10), (10, 19));
    }

    #[test]
    fn select_wrapped_line_with_newlines() {
        let text = "line1\nline2\nline3";
        assert_eq!(select_wrapped_line(text, 3, 20), (0, 5));
        assert_eq!(select_wrapped_line(text, 8, 20), (6, 11));
    }

    #[test]
    fn select_wrapped_line_empty() {
        assert_eq!(select_wrapped_line("", 0, 10), (0, 0));
    }

    #[test]
    fn select_wrapped_line_out_of_bounds() {
        let text = "abc";
        assert_eq!(select_wrapped_line(text, 100, 10), (3, 3));
    }

    // -------------------------------------------------------------------------
    // select_range
    // -------------------------------------------------------------------------

    #[test]
    fn select_range_single_click() {
        let text = "hello world";
        assert_eq!(select_range(text, 3, 10, 1), (3, 3));
    }

    #[test]
    fn select_range_double_click() {
        let text = "hello world";
        assert_eq!(select_range(text, 3, 10, 2), (0, 5));
    }

    #[test]
    fn select_range_triple_click() {
        let text = "hello world foo";
        assert_eq!(select_range(text, 3, 10, 3), (0, 10));
    }

    // -------------------------------------------------------------------------
    // update_click_tracking
    // -------------------------------------------------------------------------

    #[test]
    fn click_tracking_first_click() {
        let mut app = App::new();
        let count = update_click_tracking(&mut app, 5, 5);
        assert_eq!(count, 1);
        assert_eq!(app.click_count, 1);
        assert_eq!(app.last_click_pos, (5, 5));
    }

    #[test]
    fn click_tracking_double_click() {
        let mut app = App::new();
        update_click_tracking(&mut app, 5, 5);
        let count = update_click_tracking(&mut app, 5, 5);
        assert_eq!(count, 2);
        assert_eq!(app.click_count, 2);
    }

    #[test]
    fn click_tracking_triple_click() {
        let mut app = App::new();
        update_click_tracking(&mut app, 5, 5);
        update_click_tracking(&mut app, 5, 5);
        let count = update_click_tracking(&mut app, 5, 5);
        assert_eq!(count, 3);
        assert_eq!(app.click_count, 3);
    }

    #[test]
    fn click_tracking_caps_at_three() {
        let mut app = App::new();
        for _ in 0..10 {
            update_click_tracking(&mut app, 5, 5);
        }
        assert_eq!(app.click_count, 3);
    }

    #[test]
    fn click_tracking_resets_on_distance() {
        let mut app = App::new();
        update_click_tracking(&mut app, 5, 5);
        // Move more than MULTI_CLICK_DIST (1 cell) away
        let count = update_click_tracking(&mut app, 10, 10);
        assert_eq!(count, 1);
    }

    #[test]
    fn click_tracking_resets_on_time() {
        let mut app = App::new();
        update_click_tracking(&mut app, 5, 5);
        // Manually set last_click_time to be long ago
        app.last_click_time = Some(Instant::now() - Duration::from_secs(10));
        let count = update_click_tracking(&mut app, 5, 5);
        assert_eq!(count, 1);
    }

    // -------------------------------------------------------------------------
    // tick_deferred_copy
    // -------------------------------------------------------------------------

    #[test]
    fn deferred_copy_counts_down() {
        let mut app = App::new();
        app.copy_defer_ticks = 3;
        app.pending_copy_source = Some(SelectionSource::Result);
        app.last_result = Some("hello world".to_string());
        app.selection = Some(TextSelection {
            source: SelectionSource::Result,
            start: 0,
            end: 5,
        });

        tick_deferred_copy(&mut app);
        assert_eq!(app.copy_defer_ticks, 2);
        assert!(app.pending_copy_source.is_some());
    }

    #[test]
    fn deferred_copy_fires_when_timer_reaches_zero() {
        let mut app = App::new();
        app.copy_defer_ticks = 1;
        app.pending_copy_source = Some(SelectionSource::Result);
        app.last_result = Some("hello world".to_string());
        app.selection = Some(TextSelection {
            source: SelectionSource::Result,
            start: 0,
            end: 5,
        });

        tick_deferred_copy(&mut app);
        assert_eq!(app.copy_defer_ticks, 0);
        assert!(app.pending_copy_source.is_none());
        // Selection should be restored with flash
        assert!(app.selection.is_some());
        assert_eq!(app.copy_flash_ticks, 5);
        let sel = app.selection.unwrap();
        assert_eq!(sel.source, SelectionSource::Result);
        assert_eq!(sel.start, 0);
        assert_eq!(sel.end, 5);
    }

    #[test]
    fn deferred_copy_noop_when_timer_zero() {
        let mut app = App::new();
        app.copy_defer_ticks = 0;
        app.pending_copy_source = Some(SelectionSource::Result);

        tick_deferred_copy(&mut app);
        assert!(app.pending_copy_source.is_some()); // not consumed
    }

    #[test]
    fn deferred_copy_canceled_when_selection_cleared() {
        let mut app = App::new();
        app.copy_defer_ticks = 1;
        app.pending_copy_source = Some(SelectionSource::Result);
        app.last_result = Some("hello world".to_string());
        // No selection
        app.selection = None;

        tick_deferred_copy(&mut app);
        assert_eq!(app.copy_defer_ticks, 0);
        assert!(app.pending_copy_source.is_none());
        assert!(app.selection.is_none());
        assert_eq!(app.copy_flash_ticks, 0);
    }

    #[test]
    fn deferred_copy_canceled_when_selection_source_mismatches() {
        let mut app = App::new();
        app.copy_defer_ticks = 1;
        app.pending_copy_source = Some(SelectionSource::Result);
        app.last_result = Some("hello world".to_string());
        // Selection is for a different source
        app.selection = Some(TextSelection {
            source: SelectionSource::FileContent,
            start: 0,
            end: 5,
        });

        tick_deferred_copy(&mut app);
        assert_eq!(app.copy_defer_ticks, 0);
        assert!(app.pending_copy_source.is_none());
        // Selection should NOT be consumed
        assert!(app.selection.is_some());
        assert_eq!(app.copy_flash_ticks, 0);
    }

    #[test]
    fn deferred_copy_zero_width_selection_noop() {
        let mut app = App::new();
        app.copy_defer_ticks = 1;
        app.pending_copy_source = Some(SelectionSource::Result);
        app.last_result = Some("hello world".to_string());
        app.selection = Some(TextSelection {
            source: SelectionSource::Result,
            start: 5,
            end: 5, // zero-width
        });

        tick_deferred_copy(&mut app);
        assert_eq!(app.copy_defer_ticks, 0);
        assert!(app.pending_copy_source.is_none());
        // Selection preserved but not copied
        assert!(app.selection.is_some());
        assert_eq!(app.copy_flash_ticks, 0);
    }

    // -------------------------------------------------------------------------
    // handle_mouse_down (deferred-copy cancellation)
    // -------------------------------------------------------------------------

    #[test]
    fn mouse_down_cancels_deferred_copy() {
        let mut app = App::new();
        app.copy_defer_ticks = 2;
        app.pending_copy_source = Some(SelectionSource::Result);

        // Simulate a click outside all text areas (no rects set)
        handle_mouse_down(&mut app, 0, 0, false, false, false, false, false, false, KeyModifiers::empty());

        assert_eq!(app.copy_defer_ticks, 0);
        assert!(app.pending_copy_source.is_none());
    }

    // -------------------------------------------------------------------------
    // Shift+Click selection extension
    // -------------------------------------------------------------------------

    #[test]
    fn shift_click_extends_task_selection_forward() {
        let mut app = App::new();
        app.task_input = "hello world foo bar".to_string();
        app.task_input_rect = Some(ratatui::layout::Rect::new(0, 0, 40, 10));
        app.screen = Screen::Task;

        // First click at byte 0, double-click to select "hello" (0..5)
        let modifiers = KeyModifiers::empty();
        let started = start_text_selection(&mut app, 0, 0, 2, true, false, false, modifiers);
        assert!(started);
        assert_eq!(app.selection.as_ref().unwrap().start, 0);
        assert_eq!(app.selection.as_ref().unwrap().end, 5);

        // Shift+click at byte 12 (col 12 maps to byte 12 for ASCII single-line)
        let shift = KeyModifiers::SHIFT;
        let started = start_text_selection(&mut app, 12, 0, 1, true, false, false, shift);
        assert!(started);
        let sel = app.selection.as_ref().unwrap();
        assert_eq!(sel.source, SelectionSource::TaskInput);
        assert_eq!(sel.start, 0); // anchor preserved
        assert_eq!(sel.end, 12);  // extended to click position
    }

    #[test]
    fn shift_click_extends_task_selection_backward() {
        let mut app = App::new();
        app.task_input = "hello world foo bar".to_string();
        app.task_input_rect = Some(ratatui::layout::Rect::new(0, 0, 40, 10));
        app.screen = Screen::Task;

        // First click at byte 16, double-click to select "bar" (16..19)
        let modifiers = KeyModifiers::empty();
        start_text_selection(&mut app, 16, 0, 2, true, false, false, modifiers);
        let anchor = app.selection.as_ref().unwrap().start;

        // Shift+click at byte 0 to extend backward
        let shift = KeyModifiers::SHIFT;
        let started = start_text_selection(&mut app, 0, 0, 1, true, false, false, shift);
        assert!(started);
        let sel = app.selection.as_ref().unwrap();
        assert_eq!(sel.start, anchor); // anchor preserved
        assert_eq!(sel.end, 0);         // extended backward to click position
    }

    #[test]
    fn shift_double_click_extends_by_word_forward() {
        let mut app = App::new();
        app.task_input = "hello world foo bar".to_string();
        app.task_input_rect = Some(ratatui::layout::Rect::new(0, 0, 40, 10));
        app.screen = Screen::Task;

        // Single click at byte 0 creates zero-width selection (0,0)
        let modifiers = KeyModifiers::empty();
        start_text_selection(&mut app, 0, 0, 1, true, false, false, modifiers);

        // Shift+double-click at byte 12 extends to word "foo" (12..15)
        let shift = KeyModifiers::SHIFT;
        let started = start_text_selection(&mut app, 12, 0, 2, true, false, false, shift);
        assert!(started);
        let sel = app.selection.as_ref().unwrap();
        assert_eq!(sel.start, 0);
        assert_eq!(sel.end, 15); // "foo" ends at byte 15
    }

    #[test]
    fn shift_double_click_extends_by_word_backward() {
        let mut app = App::new();
        app.task_input = "hello world foo bar".to_string();
        app.task_input_rect = Some(ratatui::layout::Rect::new(0, 0, 40, 10));
        app.screen = Screen::Task;

        // Double-click at byte 16 to select "bar" (16..19)
        let modifiers = KeyModifiers::empty();
        start_text_selection(&mut app, 16, 0, 2, true, false, false, modifiers);
        let anchor = app.selection.as_ref().unwrap().start;

        // Shift+double-click at byte 0 extends backward to "hello" start
        let shift = KeyModifiers::SHIFT;
        let started = start_text_selection(&mut app, 0, 0, 2, true, false, false, shift);
        assert!(started);
        let sel = app.selection.as_ref().unwrap();
        assert_eq!(sel.start, anchor);
        assert_eq!(sel.end, 0); // "hello" starts at 0
    }

    #[test]
    fn shift_click_no_selection_uses_cursor_as_anchor() {
        let mut app = App::new();
        app.task_input = "hello world foo bar".to_string();
        app.task_input_rect = Some(ratatui::layout::Rect::new(0, 0, 40, 10));
        app.screen = Screen::Task;
        app.task_cursor = 6; // cursor at "world"
        app.selection = None;

        // Shift+click at byte 12
        let shift = KeyModifiers::SHIFT;
        let started = start_text_selection(&mut app, 12, 0, 1, true, false, false, shift);
        assert!(started);
        let sel = app.selection.as_ref().unwrap();
        assert_eq!(sel.start, 6); // cursor as anchor
        assert_eq!(sel.end, 12); // click position
    }

    #[test]
    fn shift_click_different_source_creates_new_selection() {
        let mut app = App::new();
        app.task_input = "hello world".to_string();
        app.last_result = Some("result text here".to_string());
        app.task_input_rect = Some(ratatui::layout::Rect::new(0, 0, 40, 10));
        app.result_rect = Some(ratatui::layout::Rect::new(0, 0, 40, 10));
        app.screen = Screen::Task;

        // Create selection in task input
        let modifiers = KeyModifiers::empty();
        start_text_selection(&mut app, 0, 0, 1, true, false, false, modifiers);
        assert_eq!(app.selection.as_ref().unwrap().source, SelectionSource::TaskInput);

        // Shift+click in result area should create a new selection, not extend task input
        let shift = KeyModifiers::SHIFT;
        let started = start_text_selection(&mut app, 0, 0, 1, false, true, false, shift);
        assert!(started);
        let sel = app.selection.as_ref().unwrap();
        assert_eq!(sel.source, SelectionSource::Result);
    }

    #[test]
    fn shift_click_result_area_extends_from_anchor() {
        let mut app = App::new();
        app.last_result = Some("hello world foo bar".to_string());
        app.result_rect = Some(ratatui::layout::Rect::new(0, 0, 40, 10));
        app.screen = Screen::Task;

        // Create selection in result area (double-click at byte 0 selects "hello" 0..5)
        let modifiers = KeyModifiers::empty();
        start_text_selection(&mut app, 0, 0, 2, false, true, false, modifiers);
        let anchor = app.selection.as_ref().unwrap().start;

        // Shift+click at byte 12 to extend
        let shift = KeyModifiers::SHIFT;
        let started = start_text_selection(&mut app, 12, 0, 1, false, true, false, shift);
        assert!(started);
        let sel = app.selection.as_ref().unwrap();
        assert_eq!(sel.source, SelectionSource::Result);
        assert_eq!(sel.start, anchor);
        assert_eq!(sel.end, 12);
    }

    // -------------------------------------------------------------------------
    // copy_defer_duration configurable
    // -------------------------------------------------------------------------

    #[test]
    fn copy_defer_duration_defaults_to_three() {
        let app = App::new();
        assert_eq!(app.copy_defer_duration, 3);
    }

    #[test]
    fn copy_defer_duration_cycles() {
        let mut app = App::new();
        assert_eq!(app.copy_defer_duration, 3);
        app.toggle_copy_defer_duration();
        assert_eq!(app.copy_defer_duration, 5);
        app.toggle_copy_defer_duration();
        assert_eq!(app.copy_defer_duration, 10);
        app.toggle_copy_defer_duration();
        assert_eq!(app.copy_defer_duration, 1);
        app.toggle_copy_defer_duration();
        assert_eq!(app.copy_defer_duration, 2);
        app.toggle_copy_defer_duration();
        assert_eq!(app.copy_defer_duration, 3);
    }
}

