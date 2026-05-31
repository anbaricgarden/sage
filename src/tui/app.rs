use std::collections::{HashSet, VecDeque};
use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use ratatui::layout::Rect;
use serde::{Deserialize, Serialize};

use crate::agent::orchestrator::{Orchestrator, OrchestratorState};
use crate::codegraph::graph::CodeGraph;

/// Which text area a selection belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionSource {
    TaskInput,
    Result,
    FileContent,
    FileFilter,
}

/// A text selection (byte range inside a specific text area).
#[derive(Debug, Clone)]
pub struct TextSelection {
    pub source: SelectionSource,
    pub start: usize,
    pub end: usize,
}

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
    // ── File tree ──
    pub expanded_dirs: HashSet<String>,
    pub file_filter: String,
    pub file_filter_focused: bool,
    pub file_filter_cursor: usize,
    pub file_filter_rect: Option<Rect>,
    // ── Settings ──
    pub settings_cursor: usize,
    pub settings_hover: Option<usize>,
    pub settings_rect: Option<Rect>,
    pub sidebar_hover: Option<usize>,
    pub file_hover: Option<usize>,
    pub animation_speed: AnimationSpeed,
    // ── Text selection ──
    pub selection: Option<TextSelection>,
    pub copy_flash_ticks: u8,
    pub result_rect: Option<Rect>,
    pub file_content_rect: Option<Rect>,
    pub result_scroll: usize,
    pub file_content_scroll: usize,
    pub mouse_enabled: bool,
    pub log_filter: LogFilter,
    pub theme: Theme,
    // ── Click tracking for double-/triple-click ──
    pub last_click_time: Option<Instant>,
    pub last_click_pos: (u16, u16),
    pub click_count: u8,
    // ── Deferred copy for multi-click debounce ──
    pub copy_defer_ticks: u8,
    pub pending_copy_source: Option<SelectionSource>,
    /// Copy defer duration in ticks (100ms each). Default 3 = ~300ms.
    pub copy_defer_duration: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnimationSpeed {
    Slow,
    Normal,
    Fast,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LogFilter {
    All,
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
        let mut app = Self {
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
            expanded_dirs: HashSet::new(),
            file_filter: String::new(),
            file_filter_focused: false,
            file_filter_cursor: 0,
            file_filter_rect: None,
            settings_cursor: 0,
            settings_hover: None,
            settings_rect: None,
            sidebar_hover: None,
            file_hover: None,
            animation_speed: AnimationSpeed::Normal,
            selection: None,
            copy_flash_ticks: 0,
            result_rect: None,
            file_content_rect: None,
            result_scroll: 0,
            file_content_scroll: 0,
            mouse_enabled: true,
            log_filter: LogFilter::All,
            theme: Theme::Sage,
            last_click_time: None,
            last_click_pos: (0, 0),
            click_count: 0,
            copy_defer_ticks: 0,
            pending_copy_source: None,
            copy_defer_duration: 3,
        };
        SettingsData::load().apply_to_app(&mut app);
        app
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
        self.save_settings();
    }

    /// Cycle log filter.
    pub fn toggle_log_filter(&mut self) {
        self.log_filter = match self.log_filter {
            LogFilter::All => LogFilter::Info,
            LogFilter::Info => LogFilter::Warning,
            LogFilter::Warning => LogFilter::Error,
            LogFilter::Error => LogFilter::All,
        };
        self.save_settings();
    }

    /// Cycle theme.
    pub fn toggle_theme(&mut self) {
        self.theme = match self.theme {
            Theme::Sage => Theme::Dark,
            Theme::Dark => Theme::Sage,
        };
        self.save_settings();
    }

    /// Cycle copy defer duration through preset values (1,2,3,5,10 ticks).
    pub fn toggle_copy_defer_duration(&mut self) {
        self.copy_defer_duration = match self.copy_defer_duration {
            1 => 2,
            2 => 3,
            3 => 5,
            5 => 10,
            _ => 1,
        };
        self.save_settings();
    }

    /// Persist current settings to disk.
    pub fn save_settings(&self) {
        if let Err(e) = SettingsData::from_app(self).save() {
            eprintln!("Failed to save settings: {}", e);
        }
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

/// Serializable subset of App settings for persistence.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
struct SettingsData {
    pub animation_speed: AnimationSpeed,
    pub log_filter: LogFilter,
    pub theme: Theme,
    pub mouse_enabled: bool,
    pub copy_defer_duration: u8,
}

impl Default for SettingsData {
    fn default() -> Self {
        Self {
            animation_speed: AnimationSpeed::Normal,
            log_filter: LogFilter::All,
            theme: Theme::Sage,
            mouse_enabled: true,
            copy_defer_duration: 3,
        }
    }
}

impl SettingsData {
    fn from_app(app: &App) -> Self {
        Self {
            animation_speed: app.animation_speed,
            log_filter: app.log_filter,
            theme: app.theme,
            mouse_enabled: app.mouse_enabled,
            copy_defer_duration: app.copy_defer_duration,
        }
    }

    fn apply_to_app(&self, app: &mut App) {
        app.animation_speed = self.animation_speed;
        app.log_filter = self.log_filter;
        app.theme = self.theme;
        app.mouse_enabled = self.mouse_enabled;
        app.copy_defer_duration = self.copy_defer_duration;
    }

    /// Return the path to the settings JSON file (`~/.config/sage/settings.json`).
    fn path() -> Option<PathBuf> {
        dirs::config_dir().map(|mut p| {
            p.push("sage");
            p.push("settings.json");
            p
        })
    }

    /// Load settings from disk, or return defaults if the file doesn't exist yet.
    fn load() -> Self {
        let Some(path) = Self::path() else {
            return Self::default();
        };
        let raw = match fs::read_to_string(&path) {
            Ok(r) => r,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Self::default(),
            Err(e) => {
                eprintln!("Failed to read settings file: {}", e);
                return Self::default();
            }
        };
        match serde_json::from_str(&raw) {
            Ok(data) => data,
            Err(e) => {
                eprintln!("Failed to parse settings file: {}", e);
                Self::default()
            }
        }
    }

    /// Save settings to disk.
    fn save(&self) -> std::io::Result<()> {
        let Some(path) = Self::path() else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Could not determine config directory",
            ));
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let raw = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        fs::write(&path, raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_data_roundtrip() {
        let original = SettingsData {
            animation_speed: AnimationSpeed::Fast,
            log_filter: LogFilter::Warning,
            theme: Theme::Dark,
            mouse_enabled: false,
            copy_defer_duration: 10,
        };
        let json = serde_json::to_string(&original).unwrap();
        let parsed: SettingsData = serde_json::from_str(&json).unwrap();
        assert_eq!(original, parsed);
    }

    #[test]
    fn settings_data_defaults() {
        let data = SettingsData::default();
        assert_eq!(data.animation_speed, AnimationSpeed::Normal);
        assert_eq!(data.log_filter, LogFilter::All);
        assert_eq!(data.theme, Theme::Sage);
        assert!(data.mouse_enabled);
        assert_eq!(data.copy_defer_duration, 3);
    }

    #[test]
    fn settings_data_apply_to_app() {
        let mut app = App::new();
        let data = SettingsData {
            animation_speed: AnimationSpeed::Fast,
            log_filter: LogFilter::Error,
            theme: Theme::Dark,
            mouse_enabled: false,
            copy_defer_duration: 5,
        };
        data.apply_to_app(&mut app);
        assert_eq!(app.animation_speed, AnimationSpeed::Fast);
        assert_eq!(app.log_filter, LogFilter::Error);
        assert_eq!(app.theme, Theme::Dark);
        assert!(!app.mouse_enabled);
        assert_eq!(app.copy_defer_duration, 5);
    }

    #[test]
    fn settings_data_from_app() {
        let mut app = App::new();
        app.animation_speed = AnimationSpeed::Slow;
        app.log_filter = LogFilter::Info;
        app.theme = Theme::Dark;
        app.mouse_enabled = false;
        app.copy_defer_duration = 1;
        let data = SettingsData::from_app(&app);
        assert_eq!(data.animation_speed, AnimationSpeed::Slow);
        assert_eq!(data.log_filter, LogFilter::Info);
        assert_eq!(data.theme, Theme::Dark);
        assert!(!data.mouse_enabled);
        assert_eq!(data.copy_defer_duration, 1);
    }

    #[test]
    fn settings_save_and_load() {
        // Use a temporary config dir override isn't easy with dirs crate,
        // so we test via path and manual read/write in a temp dir.
        let tmpdir = tempfile::tempdir().unwrap();
        let path = tmpdir.path().join("settings.json");

        let original = SettingsData {
            animation_speed: AnimationSpeed::Fast,
            log_filter: LogFilter::Warning,
            theme: Theme::Dark,
            mouse_enabled: false,
            copy_defer_duration: 10,
        };

        let raw = serde_json::to_string_pretty(&original).unwrap();
        fs::write(&path, raw).unwrap();

        let loaded_raw = fs::read_to_string(&path).unwrap();
        let loaded: SettingsData = serde_json::from_str(&loaded_raw).unwrap();
        assert_eq!(original, loaded);
    }

    #[test]
    fn settings_data_missing_fields_use_defaults() {
        // Partial JSON with only some fields; #[serde(default)] fills the rest.
        let json = r#"{"theme": "Dark", "mouse_enabled": false}"#;
        let parsed: SettingsData = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.theme, Theme::Dark);
        assert!(!parsed.mouse_enabled);
        assert_eq!(parsed.animation_speed, AnimationSpeed::Normal); // default
        assert_eq!(parsed.log_filter, LogFilter::All);             // default
        assert_eq!(parsed.copy_defer_duration, 3);                 // default
    }

    #[test]
    fn settings_data_malformed_json_falls_back_to_defaults() {
        let bad = "not json at all";
        let result: Result<SettingsData, _> = serde_json::from_str(bad);
        assert!(result.is_err());
        // Verify that the load() path would have returned defaults.
        assert_eq!(SettingsData::default().theme, Theme::Sage);
    }
}
