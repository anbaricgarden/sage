use std::io;

use crossterm::{
    cursor::Show,
    event::{DisableMouseCapture, EnableMouseCapture},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{
    backend::CrosstermBackend,
    Terminal,
};

use super::app::App;
use super::events::handle_event;
use super::ui::render;

/// Run the TUI application.
pub fn run_tui() -> io::Result<()> {
    // Setup terminal.
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    stdout.execute(EnterAlternateScreen)?;
    stdout.execute(EnableMouseCapture)?;
    stdout.execute(Show)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();
    app.ingest_demo_files();
    app.log(
        "System",
        crate::tui::app::LogLevel::Info,
        "sage TUI started. Press 1-5 or Tab to navigate, Ctrl+C to quit.",
    );

    let res = run_loop(&mut terminal, &mut app);

    // Restore terminal.
    let stdout = terminal.backend_mut();
    stdout.execute(DisableMouseCapture)?;
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
            last_tick = std::time::Instant::now();
        }
    }

    Ok(())
}
