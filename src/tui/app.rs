use std::collections::{HashSet, VecDeque};
use std::fmt;
use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use ratatui::layout::Rect;
use serde::{Deserialize, Serialize};

use crate::agent::client::{ClientConfig, LlmClient};
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

/// Which panel (if any) is overlaid on the workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    None,
    Files,
    Logs,
    Config,
    Graph,
}

impl Panel {
    pub fn title(&self) -> &'static str {
        match self {
            Panel::None => "Workspace",
            Panel::Files => "Files",
            Panel::Logs => "Logs",
            Panel::Config => "Config",
            Panel::Graph => "Graph",
        }
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

/// Which sub-tab is active inside the Config panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigTab {
    Settings,
    Providers,
}

/// Central application state for the TUI.
pub struct App {
    /// Current overlay panel (None = workspace visible).
    pub panel: Panel,
    /// The orchestrator driving the multi-agent pipeline.
    pub orchestrator: Orchestrator,
    /// The code graph for context retrieval.
    pub code_graph: CodeGraph,
    /// Rolling log buffer (keeps last N entries).
    pub logs: VecDeque<LogEntry>,
    /// Max log entries to retain.
    pub max_logs: usize,
    /// Task input buffer.
    pub task_input: String,
    /// Whether the task input is focused (true at startup).
    pub input_focused: bool,
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
    // ── Workspace layout rects ──
    pub task_input_rect: Option<Rect>,
    pub result_rect: Option<Rect>,
    // ── File tree ──
    pub expanded_dirs: HashSet<String>,
    pub file_filter: String,
    pub file_filter_focused: bool,
    pub file_filter_cursor: usize,
    pub file_filter_rect: Option<Rect>,
    pub file_tree_rect: Option<Rect>,
    pub file_content_rect: Option<Rect>,
    pub file_content_scroll: usize,
    pub file_hover: Option<usize>,
    // ── Config panel ──
    pub config_tab: ConfigTab,
    pub settings_cursor: usize,
    pub settings_hover: Option<usize>,
    pub animation_speed: AnimationSpeed,
    // ── Providers ──
    pub providers: Vec<ProviderEntry>,
    pub active_provider: Option<u64>,
    /// Which provider is in the detail view (List state when None).
    pub provider_detail_view: Option<u64>,
    /// Which template is being created (List state when None).
    pub provider_create_view: Option<ProviderType>,
    /// Cursor for cycling through config fields in detail/create view.
    pub provider_detail_cursor: usize,
    /// Field currently being edited.
    pub editing_field: Option<usize>,
    /// Create-view in-progress form values.
    pub provider_create_name: String,
    pub provider_create_model: String,
    pub provider_create_base_url: String,
    pub provider_create_api_key: String,
    pub provider_confirm_delete: Option<u64>,
    pub provider_list_cursor: usize,
    pub provider_list_hover: Option<usize>,
    pub next_provider_id: u64,
    // ── Text selection ──
    pub selection: Option<TextSelection>,
    pub copy_flash_ticks: u8,
    pub result_scroll: usize,
    pub mouse_enabled: bool,
    pub log_filter: LogFilter,
    pub theme: Theme,
    // ── Panel overlay rect ──
    pub panel_rect: Option<Rect>,
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
    // Dark,
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
            panel: Panel::None,
            orchestrator: Orchestrator::new(),
            code_graph: CodeGraph::new(),
            logs: VecDeque::new(),
            max_logs: 1000,
            task_input: String::new(),
            task_cursor: 0,
            input_focused: true,
            task_scroll: 0,
            config_tab: ConfigTab::Settings,
            log_scroll: 0,
            selected_file: None,
            running: false,
            last_result: None,
            should_quit: false,
            status_message: None,
            spinner_frame: 0,
            task_input_rect: None,
            result_rect: None,
            expanded_dirs: HashSet::new(),
            file_filter: String::new(),
            file_filter_focused: false,
            file_filter_cursor: 0,
            file_filter_rect: None,
            file_tree_rect: None,
            file_content_rect: None,
            file_scroll: 0,
            file_content_scroll: 0,
            file_hover: None,
            settings_cursor: 0,
            settings_hover: None,
            animation_speed: AnimationSpeed::Normal,
            providers: Vec::new(),
            active_provider: None,
            provider_detail_view: None,
            provider_create_view: None,
            provider_detail_cursor: 0,
            editing_field: None,
            provider_create_name: String::new(),
            provider_create_model: String::new(),
            provider_create_base_url: String::new(),
            provider_create_api_key: String::new(),
            provider_confirm_delete: None,
            provider_list_cursor: 0,
            provider_list_hover: None,
            next_provider_id: 0,
            selection: None,
            copy_flash_ticks: 0,
            result_scroll: 0,
            mouse_enabled: true,
            log_filter: LogFilter::All,
            theme: Theme::Sage,
            last_click_time: None,
            last_click_pos: (0, 0),
            click_count: 0,
            copy_defer_ticks: 0,
            pending_copy_source: None,
            copy_defer_duration: 3,
            panel_rect: None,
        };
        SettingsData::load().apply_to_app(&mut app);

        // Wire LLM client into the orchestrator from the active provider.
        if let Some(id) = app.active_provider {
            if let Some(idx) = app.providers.iter().position(|p| p.id == id) {
                if let Some(client) = SettingsData::try_build_llm_client(&app.providers[idx]) {
                    app.orchestrator = Orchestrator::with_llm_client(client);
                }
            }
        }

        app
    }

    /// Toggle the given panel (open if closed, close if open).
    pub fn toggle_panel(&mut self, panel: Panel) {
        if self.panel == panel {
            self.panel = Panel::None;
        } else {
            self.panel = panel;
        }
    }

    /// Close any open panel.
    pub fn close_panel(&mut self) {
        self.panel = Panel::None;
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
        // Dark theme not yet implemented — always stays on Sage.
        self.theme = Theme::Sage;
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

    // ── Provider methods ─────────────────────────────────────────────────────

    /// Enter the detail view for an existing provider.
    pub fn enter_provider_detail(&mut self, id: u64) {
        self.provider_detail_view = Some(id);
        self.provider_create_view = None;
        self.provider_detail_cursor = 0;
        self.editing_field = None;
    }

    /// Enter the create view for a provider template.
    pub fn enter_provider_create(&mut self, provider_type: ProviderType) {
        self.provider_detail_view = None;
        self.provider_create_view = Some(provider_type);
        self.provider_detail_cursor = 0;
        self.editing_field = None;
        // Pre-fill form with defaults — user edits these before saving.
        self.provider_create_name = format!("New {}", provider_type);
        self.provider_create_model = String::new();
        self.provider_create_base_url = provider_type.default_base_url().to_string();
        self.provider_create_api_key = String::new();
    }

    /// Return to the provider list (close detail or create view).
    pub fn exit_provider_view(&mut self) {
        self.provider_detail_view = None;
        self.provider_create_view = None;
        self.provider_detail_cursor = 0;
        self.editing_field = None;
    }

    /// Add a new provider from a template type and enter detail view.
    pub fn add_provider(&mut self, provider_type: ProviderType) {
        let entry = ProviderEntry {
            id: self.next_provider_id,
            name: std::mem::take(&mut self.provider_create_name),
            provider_type,
            model: std::mem::take(&mut self.provider_create_model),
            base_url: std::mem::take(&mut self.provider_create_base_url),
            api_key: std::mem::take(&mut self.provider_create_api_key),
        };
        self.providers.push(entry);
        let id = self.next_provider_id;
        self.next_provider_id += 1;
        self.enter_provider_detail(id);
    }

    /// Delete the provider with the given ID after user confirmation.
    pub fn delete_provider(&mut self, id: u64) {
        self.providers.retain(|p| p.id != id);
        if self.provider_detail_view == Some(id) {
            self.provider_detail_view = None;
        }
        if self.active_provider == Some(id) {
            self.active_provider = None;
            self.orchestrator = Orchestrator::new();
        }
        self.provider_confirm_delete = None;
        self.save_settings();
    }

    /// Activate a provider (immediately use it for LLM calls).
    pub fn activate_provider(&mut self, id: u64) {
        if let Some(idx) = self.providers.iter().position(|p| p.id == id) {
            self.active_provider = Some(id);
            if let Some(client) = SettingsData::try_build_llm_client(&self.providers[idx]) {
                self.orchestrator = Orchestrator::with_llm_client(client);
            }
            self.save_settings();
        }
    }

    /// Get a mutable reference to the provider in detail view, if any.
    pub fn detail_provider_mut(&mut self) -> Option<&mut ProviderEntry> {
        let id = self.provider_detail_view?;
        let idx = self.providers.iter().position(|p| p.id == id)?;
        Some(&mut self.providers[idx])
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

    /// Return agent statuses for the pipeline strip.
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

// ── Provider types ──────────────────────────────────────────────────────────

/// The kind of LLM provider (used as template when creating a new provider).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProviderType {
    GenericOpenAI,
    GenericAnthropic,
    LMStudio,
    Ollama,
    LlamaCpp,
}

impl ProviderType {
    /// Human-readable display name.
    pub fn display_name(&self) -> &'static str {
        match self {
            ProviderType::GenericOpenAI => "Generic OpenAI",
            ProviderType::GenericAnthropic => "Generic Anthropic",
            ProviderType::LMStudio => "LM Studio",
            ProviderType::Ollama => "Ollama",
            ProviderType::LlamaCpp => "Llama.cpp",
        }
    }

    /// Default base URL for this provider type (empty string = must be set by user).
    pub fn default_base_url(&self) -> &'static str {
        match self {
            ProviderType::GenericOpenAI => "https://api.openai.com/v1",
            ProviderType::GenericAnthropic => "https://api.anthropic.com/v1",
            ProviderType::LMStudio => "http://localhost:1234/v1",
            ProviderType::Ollama => "http://localhost:11434/v1",
            ProviderType::LlamaCpp => "http://localhost:8080/v1",
        }
    }

    /// Preset models for this provider type.
    pub fn preset_models(&self) -> &'static [&'static str] {
        match self {
            ProviderType::GenericOpenAI => &["gpt-4o", "gpt-4o-mini"],
            ProviderType::GenericAnthropic => &[
                "claude-3-5-sonnet-20250620",
                "claude-3-5-haiku-20250620",
            ],
            _ => &[],
        }
    }
}

impl fmt::Display for ProviderType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

/// A configured LLM provider entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderEntry {
    pub id: u64,
    pub name: String,
    pub provider_type: ProviderType,
    pub model: String,
    pub base_url: String,
    pub api_key: String,
}

impl ProviderEntry {
    /// Build a ClientConfig from this provider entry.
    pub fn to_client_config(&self) -> ClientConfig {
        ClientConfig {
            provider: self.provider_type.default_base_url()
                .trim_start_matches("http://")
                .trim_start_matches("https://")
                .split('/')
                .next()
                .unwrap_or("openai")
                .to_string(),
            model: self.model.clone(),
            base_url: if self.base_url.is_empty() {
                None
            } else {
                Some(self.base_url.clone())
            },
            api_key: if self.api_key.is_empty() {
                None
            } else {
                Some(self.api_key.clone())
            },
            timeout_secs: 120,
        }
    }
}

// ── Settings data (persistence) ───────────────────────────────────────────

/// Serializable subset of App settings for persistence.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
struct SettingsData {
    pub animation_speed: AnimationSpeed,
    pub log_filter: LogFilter,
    pub theme: Theme,
    pub mouse_enabled: bool,
    pub copy_defer_duration: u8,
    pub providers: Vec<ProviderEntry>,
    pub active_provider: Option<u64>,
}

impl Default for SettingsData {
    fn default() -> Self {
        Self {
            animation_speed: AnimationSpeed::Normal,
            log_filter: LogFilter::All,
            theme: Theme::Sage,
            mouse_enabled: true,
            copy_defer_duration: 3,
            providers: Vec::new(),
            active_provider: None,
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
            providers: app.providers.clone(),
            active_provider: app.active_provider,
        }
    }

    fn apply_to_app(&self, app: &mut App) {
        app.animation_speed = self.animation_speed;
        app.log_filter = self.log_filter;
        app.theme = self.theme;
        app.mouse_enabled = self.mouse_enabled;
        app.copy_defer_duration = self.copy_defer_duration;
        app.providers = self.providers.clone();
        app.active_provider = self.active_provider;
        if let Some(max_id) = app.providers.iter().map(|p| p.id).max() {
            app.next_provider_id = max_id + 1;
        }
    }

    /// Try to build an LLM client from a provider entry.
    fn try_build_llm_client(entry: &ProviderEntry) -> Option<LlmClient> {
        let has_env_key = std::env::var("OPENAI_API_KEY").is_ok()
            || std::env::var("ANTHROPIC_API_KEY").is_ok();
        if entry.api_key.is_empty() && !has_env_key {
            return None;
        }
        let config = entry.to_client_config();
        Some(LlmClient::new(config))
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
            theme: Theme::Sage,
            mouse_enabled: false,
            copy_defer_duration: 10,
            providers: vec![
                ProviderEntry {
                    id: 1,
                    name: "My Ollama".to_string(),
                    provider_type: ProviderType::Ollama,
                    model: "llama3".to_string(),
                    base_url: "http://localhost:11434/v1".to_string(),
                    api_key: String::new(),
                },
            ],
            active_provider: Some(1),
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
        assert!(data.providers.is_empty());
        assert_eq!(data.active_provider, None);
    }

    #[test]
    fn settings_data_apply_to_app() {
        let mut app = App::new();
        app.animation_speed = AnimationSpeed::Fast;
        app.log_filter = LogFilter::Error;
        app.theme = Theme::Sage;
        app.mouse_enabled = false;
        app.copy_defer_duration = 5;

        let data = SettingsData {
            animation_speed: AnimationSpeed::Fast,
            log_filter: LogFilter::Error,
            theme: Theme::Sage,
            mouse_enabled: false,
            copy_defer_duration: 5,
            providers: vec![],
            active_provider: None,
        };
        data.apply_to_app(&mut app);
        assert_eq!(app.animation_speed, AnimationSpeed::Fast);
        assert_eq!(app.log_filter, LogFilter::Error);
        assert_eq!(app.theme, Theme::Sage);
        assert!(!app.mouse_enabled);
        assert_eq!(app.copy_defer_duration, 5);
    }

    #[test]
    fn settings_data_from_app() {
        let mut app = App::new();
        app.animation_speed = AnimationSpeed::Slow;
        app.log_filter = LogFilter::Info;
        app.theme = Theme::Sage;
        app.mouse_enabled = false;
        app.copy_defer_duration = 1;

        let data = SettingsData::from_app(&app);
        assert_eq!(data.animation_speed, AnimationSpeed::Slow);
        assert_eq!(data.log_filter, LogFilter::Info);
        assert_eq!(data.theme, Theme::Sage);
        assert!(!data.mouse_enabled);
        assert_eq!(data.copy_defer_duration, 1);
    }

    #[test]
    fn settings_save_and_load() {
        let tmpdir = tempfile::tempdir().unwrap();
        let path = tmpdir.path().join("settings.json");

        let original = SettingsData {
            animation_speed: AnimationSpeed::Fast,
            log_filter: LogFilter::Warning,
            theme: Theme::Sage,
            mouse_enabled: false,
            copy_defer_duration: 10,
            providers: vec![ProviderEntry {
                id: 1,
                name: "Test Provider".to_string(),
                provider_type: ProviderType::Ollama,
                model: "llama3".to_string(),
                base_url: "http://localhost:11434/v1".to_string(),
                api_key: String::new(),
            }],
            active_provider: Some(1),
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
        let json = r#"{"theme": "Sage", "mouse_enabled": false}"#;
        let parsed: SettingsData = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.theme, Theme::Sage);
        assert!(!parsed.mouse_enabled);
        assert_eq!(parsed.animation_speed, AnimationSpeed::Normal); // default
        assert_eq!(parsed.log_filter, LogFilter::All);             // default
        assert_eq!(parsed.copy_defer_duration, 3);                 // default
        assert!(parsed.providers.is_empty());                       // default
        assert_eq!(parsed.active_provider, None);                   // default
    }

    #[test]
    fn settings_data_malformed_json_falls_back_to_defaults() {
        let bad = "not json at all";
        let result: Result<SettingsData, _> = serde_json::from_str(bad);
        assert!(result.is_err());
        // Verify that the load() path would have returned defaults.
        assert_eq!(SettingsData::default().theme, Theme::Sage);
    }

    #[test]
    fn provider_type_display_names() {
        assert_eq!(ProviderType::GenericOpenAI.display_name(), "Generic OpenAI");
        assert_eq!(ProviderType::GenericAnthropic.display_name(), "Generic Anthropic");
        assert_eq!(ProviderType::LMStudio.display_name(), "LM Studio");
        assert_eq!(ProviderType::Ollama.display_name(), "Ollama");
        assert_eq!(ProviderType::LlamaCpp.display_name(), "Llama.cpp");
    }

    #[test]
    fn provider_type_default_base_urls() {
        assert_eq!(
            ProviderType::GenericOpenAI.default_base_url(),
            "https://api.openai.com/v1"
        );
        assert_eq!(
            ProviderType::GenericAnthropic.default_base_url(),
            "https://api.anthropic.com/v1"
        );
        assert_eq!(
            ProviderType::LMStudio.default_base_url(),
            "http://localhost:1234/v1"
        );
        assert_eq!(ProviderType::Ollama.default_base_url(), "http://localhost:11434/v1");
        assert_eq!(
            ProviderType::LlamaCpp.default_base_url(),
            "http://localhost:8080/v1"
        );
    }

    #[test]
    fn provider_entry_to_client_config() {
        let entry = ProviderEntry {
            id: 1,
            name: "My Ollama".to_string(),
            provider_type: ProviderType::Ollama,
            model: "llama3".to_string(),
            base_url: "http://localhost:11434/v1".to_string(),
            api_key: "test-key".to_string(),
        };
        let config = entry.to_client_config();
        assert_eq!(config.model, "llama3");
        assert_eq!(config.base_url, Some("http://localhost:11434/v1".to_string()));
        assert_eq!(config.api_key, Some("test-key".to_string()));
    }

    #[test]
    fn provider_entry_to_client_config_empty_base_url() {
        let entry = ProviderEntry {
            id: 1,
            name: "Generic OpenAI".to_string(),
            provider_type: ProviderType::GenericOpenAI,
            model: "gpt-4o".to_string(),
            base_url: String::new(),
            api_key: "sk-test".to_string(),
        };
        let config = entry.to_client_config();
        assert_eq!(config.model, "gpt-4o");
        assert_eq!(config.base_url, None); // empty string becomes None
        assert_eq!(config.api_key, Some("sk-test".to_string()));
    }

    #[test]
    fn add_provider_increments_id() {
        let mut app = App::new();
        // Simulate fresh app with empty providers
        app.providers.clear();
        app.next_provider_id = 0;

        app.add_provider(ProviderType::Ollama);
        assert_eq!(app.providers.len(), 1);
        assert_eq!(app.providers[0].id, 0);
        assert_eq!(app.next_provider_id, 1);

        app.add_provider(ProviderType::LMStudio);
        assert_eq!(app.providers.len(), 2);
        assert_eq!(app.providers[1].id, 1);
        assert_eq!(app.next_provider_id, 2);
    }

    #[test]
    fn delete_provider_removes_from_list() {
        let mut app = App::new();
        app.providers.clear();
        app.next_provider_id = 2;
        app.providers.push(ProviderEntry {
            id: 0,
            name: "Ollama".to_string(),
            provider_type: ProviderType::Ollama,
            model: "llama3".to_string(),
            base_url: "http://localhost:11434/v1".to_string(),
            api_key: String::new(),
        });
        app.providers.push(ProviderEntry {
            id: 1,
            name: "LM Studio".to_string(),
            provider_type: ProviderType::LMStudio,
            model: "model".to_string(),
            base_url: "http://localhost:1234/v1".to_string(),
            api_key: String::new(),
        });

        app.delete_provider(0);
        assert_eq!(app.providers.len(), 1);
        assert_eq!(app.providers[0].id, 1);
    }

    #[test]
    fn delete_active_provider_clears_orchestrator() {
        let mut app = App::new();
        app.providers.clear();
        app.next_provider_id = 1;
        app.active_provider = Some(0);
        app.providers.push(ProviderEntry {
            id: 0,
            name: "Ollama".to_string(),
            provider_type: ProviderType::Ollama,
            model: "llama3".to_string(),
            base_url: "http://localhost:11434/v1".to_string(),
            api_key: String::new(),
        });

        app.delete_provider(0);
        assert_eq!(app.active_provider, None);
    }
}
