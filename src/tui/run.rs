use std::io;

use crossterm::{
    cursor::Show,
    event::{
        DisableMouseCapture, EnableMouseCapture,
        KeyboardEnhancementFlags, PopKeyboardEnhancementFlags,
        PushKeyboardEnhancementFlags,
    },
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{
    backend::CrosstermBackend,
    Terminal,
};

use super::app::App;
use super::events::{handle_event, tick_deferred_copy};
use super::ui::render;

/// Run the TUI application.
pub fn run_tui() -> io::Result<()> {
    // Setup terminal.
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    stdout.execute(EnterAlternateScreen)?;
    stdout.execute(Show)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();
    if app.mouse_enabled {
        terminal.backend_mut().execute(EnableMouseCapture)?;
    }

    // Enable Kitty keyboard enhancement protocol so terminals report modifiers
    // on keys like Enter, Tab, Backspace, etc. (Shift+Enter, Ctrl+Enter, …).
    // Note: this just writes an escape sequence; we cannot know whether the
    // terminal actually supports it until a key event arrives.
    let _ = terminal.backend_mut().execute(PushKeyboardEnhancementFlags(
        KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
            | KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS,
    ));

    app.ingest_demo_files();
    app.log(
        "System",
        crate::tui::app::LogLevel::Info,
        "sage TUI started. Press 1-6 or Tab to navigate, Ctrl+C to quit.",
    );

    let res = run_loop(&mut terminal, &mut app);

    // Restore terminal.
    let stdout = terminal.backend_mut();
    if app.mouse_enabled {
        let _ = stdout.execute(DisableMouseCapture);
    }
    let _ = stdout.execute(PopKeyboardEnhancementFlags);
    stdout.execute(Show)?;
    disable_raw_mode()?;
    stdout.execute(LeaveAlternateScreen)?;

    res
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> io::Result<()> {
    let mut last_tick = std::time::Instant::now();
    let tick_rate = std::time::Duration::from_millis(100);

    while !app.should_quit {
        // Render.
        terminal.draw(|frame| render(frame, app))?;

        // Handle events.
        if !handle_event(app)? {
            break;
        }

        // Tick animation.
        if last_tick.elapsed() >= tick_rate {
            app.tick_spinner();
            if app.copy_flash_ticks > 0 {
                app.copy_flash_ticks -= 1;
                if app.copy_flash_ticks == 0 {
                    app.selection = None;
                }
            }
            tick_deferred_copy(app);
            last_tick = std::time::Instant::now();
        }
    }

    Ok(())
}
