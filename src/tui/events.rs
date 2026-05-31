use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};

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
        Event::Resize(_, _) => {}
        _ => {}
    }

    Ok(!app.should_quit)
}

/// Handle a key press. Returns true to continue, false to quit.
fn handle_key(app: &mut App, code: KeyCode, modifiers: KeyModifiers) -> bool {
    // Global quit.
    if code == KeyCode::Char('q') && modifiers.contains(KeyModifiers::CONTROL) {
        app.should_quit = true;
        return false;
    }

    // Global navigation.
    match code {
        KeyCode::Char('1') => app.screen = Screen::Dashboard,
        KeyCode::Char('2') => app.screen = Screen::Task,
        KeyCode::Char('3') => app.screen = Screen::Files,
        KeyCode::Char('4') => app.screen = Screen::Logs,
        KeyCode::Char('5') => app.screen = Screen::Graph,
        KeyCode::Tab => app.next_screen(),
        KeyCode::BackTab => app.prev_screen(),
        _ => {}
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

    match code {
        KeyCode::Char(c) if !modifiers.contains(KeyModifiers::CONTROL) => {
            app.task_input.push(c);
            app.task_input_focused = true;
        }
        KeyCode::Backspace => {
            app.task_input.pop();
            app.task_input_focused = true;
        }
        KeyCode::Enter => {
            app.execute_task();
            app.task_input.clear();
        }
        KeyCode::Esc => {
            app.task_input_focused = false;
        }
        _ => {}
    }
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
    let files: Vec<String> = app.orchestrator.file_contents.keys().cloned().collect();
    if files.is_empty() {
        return;
    }

    match code {
        KeyCode::Up | KeyCode::Char('k') => {
            app.file_scroll = app.file_scroll.saturating_sub(1);
            app.selected_file = Some(files[app.file_scroll].clone());
        }
        KeyCode::Down | KeyCode::Char('j') => {
            app.file_scroll = (app.file_scroll + 1).min(files.len() - 1);
            app.selected_file = Some(files[app.file_scroll].clone());
        }
        _ => {}
    }
}
