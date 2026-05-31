use std::collections::VecDeque;

use ratatui::layout::Rect;

use crate::agent::orchestrator::{Orchestrator, OrchestratorState};
use crate::codegraph::graph::CodeGraph;

/// Which screen is currently visible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Dashboard,
    Task,
    Files,
    Logs,
    Graph,
    Settings,
}

impl Screen {
    pub fn title(&self) -> &'static str {
        match self {
            Screen::Dashboard => "Dashboard",
            Screen::Task => "Task",
            Screen::Files => "Files",
            Screen::Logs => "Logs",
            Screen::Graph => "Graph",
            Screen::Settings => "Settings",
        }
    }

    pub fn all() -> &'static [Screen] {
        &[
            Screen::Dashboard,
            Screen::Task,
            Screen::Files,
            Screen::Logs,
            Screen::Graph,
            Screen::Settings,
        ]
    }
}

/// A log entry from the system or an agent.
#[derive(Debug, Clone)]
pub struct LogEntry {
    pub timestamp: String,
    pub source: String,
    pub level: LogLevel,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Info,
    Success,
    Warning,
    Error,
}

/// Central application state for the TUI.
pub struct App {
    /// Current screen.
    pub screen: Screen,
    /// The orchestrator driving the multi-agent pipeline.
    pub orchestrator: Orchestrator,
    /// The code graph for context retrieval.
    pub code_graph: CodeGraph,
    /// Rolling log buffer (keeps last N entries).
    pub logs: VecDeque<LogEntry>,
    /// Max log entries to retain.
    pub max_logs: usize,
    /// Task input buffer (for the Task screen).
    pub task_input: String,
    /// Whether the task input is focused.
    pub task_input_focused: bool,
    /// Scroll offset for the log viewer.
    pub log_scroll: usize,
    /// Selected file in the file tree.
    pub selected_file: Option<String>,
    /// File list scroll offset.
    pub file_scroll: usize,
    /// Cursor position inside `task_input` (byte index).
    pub task_cursor: usize,
    /// Visual scroll offset for the multi-line task input (line index).
    pub task_scroll: usize,
    /// Whether a task is currently running.
    pub running: bool,
    /// Last task result summary.
    pub last_result: Option<String>,
    /// Should the app quit?
    pub should_quit: bool,
    /// Status message (transient, shown at bottom).
    pub status_message: Option<(String, StatusKind)>,
    /// Animation frame counter for spinner.
    pub spinner_frame: usize,
    /// Hit-test rects from last render (for mouse clicks).
    pub sidebar_rect: Option<Rect>,
    pub file_tree_rect: Option<Rect>,
    pub task_input_rect: Option<Rect>,
    pub log_area_rect: Option<Rect>,
    // ── Settings ──
    pub settings_cursor: usize,
    pub animation_speed: AnimationSpeed,
    pub mouse_enabled: bool,
    pub log_filter: LogFilter,
    pub theme: Theme,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimationSpeed {
    Slow,
    Normal,
    Fast,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFilter {
    All,
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    Sage,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusKind {
    Info,
    Success,
    Warning,
    Error,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    pub fn new() -> Self {
        Self {
            screen: Screen::Dashboard,
            orchestrator: Orchestrator::new(),
            code_graph: CodeGraph::new(),
            logs: VecDeque::new(),
            max_logs: 1000,
            task_input: String::new(),
            task_cursor: 0,
            task_input_focused: false,
            task_scroll: 0,
            log_scroll: 0,
            selected_file: None,
            file_scroll: 0,
            running: false,
            last_result: None,
            should_quit: false,
            status_message: None,
            spinner_frame: 0,
            sidebar_rect: None,
            file_tree_rect: None,
            task_input_rect: None,
            log_area_rect: None,
            settings_cursor: 0,
            animation_speed: AnimationSpeed::Normal,
            mouse_enabled: true,
            log_filter: LogFilter::All,
            theme: Theme::Sage,
        }
    }

    /// Switch to the next screen in the sidebar.
    pub fn next_screen(&mut self) {
        let all = Screen::all();
        let idx = all.iter().position(|s| *s == self.screen).unwrap_or(0);
        self.screen = all[(idx + 1) % all.len()];
    }

    /// Switch to the previous screen in the sidebar.
    pub fn prev_screen(&mut self) {
        let all = Screen::all();
        let idx = all.iter().position(|s| *s == self.screen).unwrap_or(0);
        self.screen = all[(idx + all.len() - 1) % all.len()];
    }

    /// Push a log entry.
    pub fn log(&mut self, source: &str, level: LogLevel, message: &str) {
        let entry = LogEntry {
            timestamp: format!("{:02}:{:02}:{:02}", 0, 0, 0), // Placeholder; real time not critical
            source: source.to_string(),
            level,
            message: message.to_string(),
        };
        if self.logs.len() >= self.max_logs {
            self.logs.pop_front();
        }
        self.logs.push_back(entry);
    }

    /// Set a transient status message.
    pub fn set_status(&mut self, message: &str, kind: StatusKind) {
        self.status_message = Some((message.to_string(), kind));
    }

    /// Clear the status message.
    pub fn clear_status(&mut self) {
        self.status_message = None;
    }

    /// Advance the spinner animation frame.
    pub fn tick_spinner(&mut self) {
        let step = match self.animation_speed {
            AnimationSpeed::Slow => 1,
            AnimationSpeed::Normal => 2,
            AnimationSpeed::Fast => 4,
        };
        self.spinner_frame = (self.spinner_frame + step) % 8;
    }

    /// Cycle animation speed.
    pub fn toggle_animation_speed(&mut self) {
        self.animation_speed = match self.animation_speed {
            AnimationSpeed::Slow => AnimationSpeed::Normal,
            AnimationSpeed::Normal => AnimationSpeed::Fast,
            AnimationSpeed::Fast => AnimationSpeed::Slow,
        };
    }

    /// Cycle log filter.
    pub fn toggle_log_filter(&mut self) {
        self.log_filter = match self.log_filter {
            LogFilter::All => LogFilter::Info,
            LogFilter::Info => LogFilter::Warning,
            LogFilter::Warning => LogFilter::Error,
            LogFilter::Error => LogFilter::All,
        };
    }

    /// Cycle theme.
    pub fn toggle_theme(&mut self) {
        self.theme = match self.theme {
            Theme::Sage => Theme::Dark,
            Theme::Dark => Theme::Sage,
        };
    }

    /// Get the current spinner character.
    pub fn spinner(&self) -> char {
        const FRAMES: [char; 8] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧'];
        FRAMES[self.spinner_frame]
    }

    /// Execute the current task input.
    pub fn execute_task(&mut self) {
        let task = self.task_input.trim().to_string();
        if task.is_empty() {
            self.set_status("Task description cannot be empty.", StatusKind::Warning);
            return;
        }

        self.running = true;
        self.last_result = None;
        self.log("User", LogLevel::Info, &format!("Task: {}", task));
        self.set_status("Running task...", StatusKind::Info);

        // In a real async TUI this would be spawned; for now we block briefly
        // and then update state. We run the orchestrator synchronously.
        match self.orchestrator.run_task(&task, &self.code_graph) {
            Ok(state) => {
                let msg = format!("Task completed: {:?}", state);
                self.log("Orchestrator", LogLevel::Success, &msg);
                self.last_result = Some(msg.clone());
                self.set_status(&msg, StatusKind::Success);
            }
            Err(err) => {
                let msg = format!("Task failed: {}", err);
                self.log("Orchestrator", LogLevel::Error, &msg);
                self.last_result = Some(msg.clone());
                self.set_status(&msg, StatusKind::Error);
            }
        }
        self.running = false;
    }

    /// Populate code_graph and orchestrator with a demo file tree.
    pub fn ingest_demo_files(&mut self) {
        let files = [
            (
                "src/main.rs",
                "fn main() {\n    println!(\"Hello, world!\");\n}\n",
            ),
            (
                "src/lib.rs",
                "pub mod agent;\npub mod ast;\npub mod blob_store;\npub mod codegraph;\npub mod diff;\n",
            ),
            (
                "src/agent/mod.rs",
                "pub trait Agent {\n    fn name(&self) -> &'static str;\n}\n",
            ),
            (
                "src/agent/editor.rs",
                "pub struct EditorAgent;\n\nimpl EditorAgent {\n    pub fn new() -> Self {\n        Self\n    }\n}\n",
            ),
            (
                "Cargo.toml",
                "[package]\nname = \"sage\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
            ),
        ];
        for (path, content) in &files {
            self.orchestrator.ingest_file(path, content);
        }
        // Initialize selected_file to the first file (sorted) so file_scroll and
        // selected_file are never out of sync on first render.
        let mut sorted_files: Vec<String> = self.orchestrator.file_contents.keys().cloned().collect();
        sorted_files.sort();
        if let Some(first) = sorted_files.first() {
            self.selected_file = Some(first.clone());
        }
        self.log("System", LogLevel::Info, "Ingested demo workspace (5 files)");
    }

    /// Return agent statuses for the dashboard.
    pub fn agent_statuses(&self) -> Vec<(&'static str, &'static str, bool)> {
        vec![
            (
                "Planner",
                self.agent_state_label(&self.orchestrator.state, OrchestratorState::Planning),
                self.orchestrator.state == OrchestratorState::Planning,
            ),
            (
                "Editor",
                self.agent_state_label(&self.orchestrator.state, OrchestratorState::Editing),
                self.orchestrator.state == OrchestratorState::Editing,
            ),
            (
                "Executor",
                self.agent_state_label(&self.orchestrator.state, OrchestratorState::Executing),
                self.orchestrator.state == OrchestratorState::Executing,
            ),
            (
                "Reviewer",
                self.agent_state_label(&self.orchestrator.state, OrchestratorState::Reviewing),
                self.orchestrator.state == OrchestratorState::Reviewing,
            ),
        ]
    }

    fn agent_state_label(&self, state: &OrchestratorState, target: OrchestratorState) -> &'static str {
        if *state == target {
            "Active"
        } else {
            match state {
                OrchestratorState::Done | OrchestratorState::Rollback => {
                    if target == OrchestratorState::Reviewing {
                        "Done"
                    } else {
                        "Idle"
                    }
                }
                _ => "Idle",
            }
        }
    }
}
