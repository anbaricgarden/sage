use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Margin, Position, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{
        Block, Borders, List, ListItem, Paragraph, Scrollbar, ScrollbarOrientation,
        ScrollbarState,
    },
    Frame,
};

use super::app::{App, ConfigTab, LogLevel, Panel, ProviderType, SelectionSource, StatusKind};
use super::events::{byte_index_to_visual_pos, wrap_text};
use super::file_tree::build_visible_tree;
// ── Palette ──

const BG: Color = Color::Rgb(28, 28, 34);
const SURFACE: Color = Color::Rgb(40, 40, 48);
const SURFACE_HOVER: Color = Color::Rgb(50, 50, 60);
const ACCENT: Color = Color::Rgb(156, 175, 136); // sage
const ACCENT_DIM: Color = Color::Rgb(120, 140, 100);
const ACCENT_BRIGHT: Color = Color::Rgb(185, 205, 165);
const TEXT: Color = Color::Rgb(220, 220, 224);
const TEXT_SECONDARY: Color = Color::Rgb(150, 150, 160);
const TEXT_MUTED: Color = Color::Rgb(100, 100, 110);
const ERROR: Color = Color::Rgb(220, 120, 120);
const WARNING: Color = Color::Rgb(220, 180, 100);
const SUCCESS: Color = Color::Rgb(140, 200, 140);
const INFO: Color = Color::Rgb(130, 170, 210);
const BORDER: Color = Color::Rgb(60, 60, 70);
const BORDER_ACTIVE: Color = Color::Rgb(156, 175, 136);
const SELECT_BG: Color = Color::Rgb(55, 55, 75);
const SELECT_FLASH_BG: Color = Color::Rgb(80, 80, 110);

// ── Top-level render ──

pub fn render(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    frame.render_widget(Block::default().style(Style::default().bg(BG)), area);

    // Reset cursor to a safe corner so it doesn't linger from a previous frame.
    frame.set_cursor_position(Position::new(0, 0));

    let main_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // top bar
            Constraint::Min(0),    // content area (output + optional panel)
            Constraint::Length(3), // task input
            Constraint::Length(1), // bottom bar
        ])
        .split(area);

    render_top_bar(frame, app, main_layout[0]);
    render_content_area(frame, app, main_layout[1]);
    render_task_input(frame, app, main_layout[2]);
    render_bottom_bar(frame, app, main_layout[3]);
}

// ── Top Bar ──

fn render_top_bar(frame: &mut Frame, app: &App, area: Rect) {
    let version = env!("CARGO_PKG_VERSION");
    let top_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(16), Constraint::Min(0)])
        .split(area);

    // Logo.
    let logo = Paragraph::new(Line::from(vec![
        Span::styled("◆ ", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
        Span::styled("sage!", Style::default().fg(TEXT).add_modifier(Modifier::BOLD)),
        Span::styled(format!(" v{}", version), Style::default().fg(TEXT_MUTED)),
    ]));
    frame.render_widget(logo, top_layout[0]);

    // Pipeline strip.
    let statuses = app.agent_statuses();
    let mut pipeline_spans: Vec<Span> = Vec::new();
    for (i, (name, label, active)) in statuses.iter().enumerate() {
        let color = if *active {
            ACCENT_BRIGHT
        } else if *label == "Done" {
            SUCCESS
        } else {
            TEXT_MUTED
        };
        let marker = if *active { "◈ " } else { "◆ " };
        pipeline_spans.push(Span::styled(
            format!("{}{} {}", marker, name, label),
            Style::default().fg(color),
        ));
        if i < statuses.len() - 1 {
            pipeline_spans.push(Span::styled("  │  ", Style::default().fg(BORDER)));
        }
    }
    let pipeline = Paragraph::new(Line::from(pipeline_spans)).alignment(Alignment::Center);
    frame.render_widget(pipeline, top_layout[1]);
}

// ── Content Area (output + optional panel overlay) ──

fn render_content_area(frame: &mut Frame, app: &mut App, area: Rect) {
    match app.panel {
        Panel::None => {
            app.panel_rect = None;
            render_output(frame, app, area);
        }
        Panel::Files => {
            let split = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
                .split(area);
            app.panel_rect = Some(split[0]);
            render_files_panel(frame, app, split[0]);
            render_output(frame, app, split[1]);
        }
        Panel::Logs => {
            let split = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Percentage(65), Constraint::Percentage(35)])
                .split(area);
            app.panel_rect = Some(split[1]);
            render_output(frame, app, split[0]);
            render_logs_panel(frame, app, split[1]);
        }
        Panel::Config => {
            let split = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(65), Constraint::Percentage(35)])
                .split(area);
            app.panel_rect = Some(split[1]);
            render_output(frame, app, split[0]);
            render_config_panel(frame, app, split[1]);
        }
        Panel::Graph => {
            let split = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(65), Constraint::Percentage(35)])
                .split(area);
            app.panel_rect = Some(split[1]);
            render_output(frame, app, split[0]);
            render_graph_panel(frame, app, split[1]);
        }
    }
}

// ── Output Body ──

fn render_output(frame: &mut Frame, app: &mut App, area: Rect) {
    let inner = area.inner(Margin::new(1, 0));
    app.result_rect = Some(inner);

    if let Some(result) = &app.last_result {
        let result_width = inner.width;
        let visible_height = inner.height as usize;

        // Build combined output text (result + history).
        let mut output_text = result.clone();
        if !app.orchestrator.history.is_empty() {
            output_text.push_str("\n\nHistory:\n");
            for entry in &app.orchestrator.history {
                output_text.push_str(&format!("  • {}\n", entry));
            }
        }

        let total_lines = wrap_text(&output_text, result_width).len();
        app.result_scroll = app.result_scroll.min(total_lines.saturating_sub(visible_height));

        // Only apply selection highlight if it falls entirely within the result portion.
        let sel = app
            .selection
            .as_ref()
            .filter(|s| s.source == SelectionSource::Result && s.end <= result.len())
            .map(|s| (s.start, s.end));
        let flash = app.copy_flash_ticks > 0;
        let select_bg = if flash { SELECT_FLASH_BG } else { SELECT_BG };

        let output_lines = lines_with_selection(
            &output_text,
            result_width,
            app.result_scroll,
            visible_height,
            sel,
            Style::default().fg(TEXT),
            select_bg,
        );

        let block = Block::default()
            .title(" Output ")
            .title_style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(BORDER));

        let output_widget = Paragraph::new(Text::from(output_lines)).block(block);
        frame.render_widget(output_widget, area);

        if total_lines > visible_height {
            let mut state = ScrollbarState::new(total_lines).position(app.result_scroll);
            let sb = Scrollbar::default()
                .orientation(ScrollbarOrientation::VerticalRight)
                .style(Style::default().fg(TEXT_MUTED));
            frame.render_stateful_widget(sb, area, &mut state);
        }
    } else if app.running {
        let spinner = app.spinner();
        let state_label = format!("{} Running — State: {:?}", spinner, app.orchestrator.state);
        let running_hint = Paragraph::new(Text::from(vec![
            Line::from(Span::styled(state_label, Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))),
            Line::from(Span::styled("", Style::default())),
            Line::from(Span::styled(
                "The multi-agent pipeline is working on your task.",
                Style::default().fg(TEXT_MUTED),
            )),
            Line::from(Span::styled(
                "Open the Logs panel (Ctrl+L) to see live progress.",
                Style::default().fg(TEXT_MUTED),
            )),
        ]))
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .title(" Output ")
                .title_style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
                .borders(Borders::BOTTOM)
                .border_style(Style::default().fg(BORDER)),
        );
        frame.render_widget(running_hint, area);
    } else {
        let hint = Paragraph::new(Text::from(vec![
            Line::from(vec![
                Span::styled("Welcome to ", Style::default().fg(TEXT_SECONDARY)),
                Span::styled("sage!", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
            ]),
            Line::from(Span::styled("", Style::default())),
            Line::from(Span::styled(
                "Type a coding task below and press Enter to run the multi-agent pipeline.",
                Style::default().fg(TEXT_MUTED),
            )),
            Line::from(Span::styled("", Style::default())),
            Line::from(vec![
                Span::styled("Ctrl+F ", Style::default().fg(ACCENT)),
                Span::styled("Files  ", Style::default().fg(TEXT_SECONDARY)),
                Span::styled("Ctrl+L ", Style::default().fg(ACCENT)),
                Span::styled("Logs  ", Style::default().fg(TEXT_SECONDARY)),
                Span::styled("Ctrl+, ", Style::default().fg(ACCENT)),
                Span::styled("Config  ", Style::default().fg(TEXT_SECONDARY)),
                Span::styled("Ctrl+G ", Style::default().fg(ACCENT)),
                Span::styled("Graph", Style::default().fg(TEXT_SECONDARY)),
            ]),
        ]))
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .title(" Output ")
                .title_style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
                .borders(Borders::BOTTOM)
                .border_style(Style::default().fg(BORDER)),
        );
        frame.render_widget(hint, area);
    }
}

// ── Task Input ──

fn render_task_input(frame: &mut Frame, app: &mut App, area: Rect) {
    let input_style = if app.input_focused {
        Style::default().fg(TEXT).bg(SURFACE)
    } else {
        Style::default().fg(TEXT).bg(BG)
    };
    let input_block = Block::default()
        .title(" Task ")
        .title_style(Style::default().fg(ACCENT))
        .borders(Borders::ALL)
        .border_style(if app.input_focused {
            Style::default().fg(BORDER_ACTIVE)
        } else {
            Style::default().fg(BORDER)
        })
        .style(input_style);

    let input_inner = area.inner(Margin::new(1, 1));
    let width = input_inner.width;
    let visible_height = input_inner.height as usize;
    app.task_input_rect = Some(input_inner);

    // Compute wrapped lines and clamp scroll.
    let wrapped = wrap_text(&app.task_input, width);
    let total_lines = wrapped.len().max(1);
    app.task_scroll = app.task_scroll.min(total_lines.saturating_sub(visible_height));

    // Build visible lines with selection highlight.
    let sel = app
        .selection
        .as_ref()
        .filter(|s| s.source == SelectionSource::TaskInput)
        .map(|s| (s.start, s.end));
    let flash = app.copy_flash_ticks > 0;
    let select_bg = if flash { SELECT_FLASH_BG } else { SELECT_BG };
    let mut visible_lines = lines_with_selection(
        &app.task_input,
        width,
        app.task_scroll,
        visible_height,
        sel,
        Style::default().fg(TEXT),
        select_bg,
    );

    // Placeholder when empty and not focused.
    if app.task_input.is_empty() && !app.input_focused && app.task_scroll == 0 {
        visible_lines = vec![Line::from(Span::styled(
            "Type a task and press Enter...",
            Style::default().fg(TEXT_MUTED),
        ))];
    }

    frame.render_widget(
        Paragraph::new(Text::from(visible_lines)).block(input_block),
        area,
    );

    // Draw cursor.
    if app.input_focused {
        let (cursor_row, cursor_col) = byte_index_to_visual_pos(&app.task_input, app.task_cursor, width);
        let visible_cursor_row = cursor_row.saturating_sub(app.task_scroll);
        if visible_cursor_row < visible_height {
            let cursor_x = input_inner.x + cursor_col as u16;
            let cursor_y = input_inner.y + visible_cursor_row as u16;
            frame.set_cursor_position(Position::new(cursor_x, cursor_y));
        }
    } else {
        // Park cursor at the start of the input box when unfocused.
        frame.set_cursor_position(Position::new(input_inner.x, input_inner.y));
    }
}

// ── Bottom Bar ──

fn render_bottom_bar(frame: &mut Frame, app: &App, area: Rect) {
    let (msg, color) = match &app.status_message {
        Some((text, kind)) => {
            let c = match kind {
                StatusKind::Info => INFO,
                StatusKind::Success => SUCCESS,
                StatusKind::Warning => WARNING,
                StatusKind::Error => ERROR,
            };
            (text.as_str(), c)
        }
        None => {
            let hint = if app.running {
                "Running..."
            } else if app.panel == Panel::None {
                "Enter Submit | Shift+Enter Newline | ↑↓ Scroll | Ctrl+F Files | Ctrl+L Logs | Ctrl+, Config | Ctrl+C Quit"
            } else {
                "Esc Close Panel | Enter Submit | Shift+Enter Newline | Ctrl+C Quit"
            };
            (hint, TEXT_MUTED)
        }
    };

    let bar = Paragraph::new(Line::from(Span::styled(msg, Style::default().fg(color))))
        .style(Style::default().bg(SURFACE))
        .block(Block::default());
    frame.render_widget(bar, area);
}

// ── Panel: Files ──

fn render_files_panel(frame: &mut Frame, app: &mut App, area: Rect) {
    let block = Block::default()
        .title(" Files ")
        .title_style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER));
    let inner = area.inner(Margin::new(1, 1));
    frame.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(inner);

    // Filter input.
    let filter_block = Block::default()
        .title(" Filter ")
        .title_style(Style::default().fg(ACCENT))
        .borders(Borders::ALL)
        .border_style(if app.file_filter_focused {
            Style::default().fg(BORDER_ACTIVE)
        } else {
            Style::default().fg(BORDER)
        });
    let filter_inner = chunks[0].inner(Margin::new(1, 1));
    app.file_filter_rect = Some(filter_inner);
    frame.render_widget(filter_block, chunks[0]);

    let filter_text = if app.file_filter.is_empty() && !app.file_filter_focused {
        Line::from(Span::styled("Search files... (press /)", Style::default().fg(TEXT_MUTED)))
    } else {
        let sel = app
            .selection
            .as_ref()
            .filter(|s| s.source == SelectionSource::FileFilter)
            .map(|s| (s.start, s.end));
        if let Some((sel_start, sel_end)) = sel {
            let (start, end) = (sel_start.min(sel_end), sel_start.max(sel_end));
            let mut spans = Vec::new();
            if start > 0 {
                spans.push(Span::styled(&app.file_filter[..start], Style::default().fg(TEXT)));
            }
            if end > start {
                spans.push(Span::styled(
                    &app.file_filter[start..end],
                    Style::default().fg(TEXT).bg(SELECT_BG),
                ));
            }
            if end < app.file_filter.len() {
                spans.push(Span::styled(&app.file_filter[end..], Style::default().fg(TEXT)));
            }
            Line::from(spans)
        } else {
            Line::from(Span::styled(&app.file_filter, Style::default().fg(TEXT)))
        }
    };
    frame.render_widget(Paragraph::new(filter_text), filter_inner);

    if app.file_filter_focused {
        let cursor_x = filter_inner.x + app.file_filter_cursor as u16;
        let cursor_y = filter_inner.y;
        frame.set_cursor_position(Position::new(cursor_x, cursor_y));
    }

    // File tree list.
    let list_area = chunks[1];
    app.file_tree_rect = Some(list_area);

    let paths: Vec<String> = app.orchestrator.file_contents.keys().cloned().collect();
    let visible = build_visible_tree(&paths, &app.expanded_dirs, &app.file_filter);

    let tree_items: Vec<ListItem> = visible
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let is_selected = app.selected_file.as_ref() == Some(&entry.path);
            let is_hovered = app.file_hover == Some(i);
            let is_cursor = i == app.file_scroll;
            let style = if is_selected {
                Style::default()
                    .fg(ACCENT_BRIGHT)
                    .bg(SURFACE)
                    .add_modifier(Modifier::BOLD)
            } else if is_hovered {
                Style::default().fg(TEXT).bg(SURFACE_HOVER)
            } else if is_cursor {
                Style::default().fg(TEXT).bg(SURFACE)
            } else {
                Style::default().fg(TEXT_SECONDARY)
            };

            let indent = "  ".repeat(entry.depth);
            let prefix = if entry.is_dir {
                if entry.is_expanded {
                    "▼ "
                } else {
                    "▶ "
                }
            } else if is_selected {
                "▸ "
            } else {
                "  "
            };

            let text = format!("{}{}{}", indent, prefix, entry.name);
            ListItem::new(Line::from(Span::styled(text, style)))
        })
        .collect();

    let tree = List::new(tree_items).block(Block::default());
    frame.render_widget(tree, list_area);

    // File content preview.
    let content_area = chunks[2];
    app.file_content_rect = Some(content_area);
    if let Some(content) = app.selected_file.as_ref().and_then(|f| app.orchestrator.file_contents.get(f)) {
        let content_width = content_area.width;
        let visible_height = content_area.height as usize;
        let total_lines = wrap_text(content, content_width).len();
        app.file_content_scroll = app.file_content_scroll.min(total_lines.saturating_sub(visible_height));

        let sel = app
            .selection
            .as_ref()
            .filter(|s| s.source == SelectionSource::FileContent)
            .map(|s| (s.start, s.end));
        let flash = app.copy_flash_ticks > 0;
        let select_bg = if flash { SELECT_FLASH_BG } else { SELECT_BG };
        let content_lines = lines_with_selection(
            content,
            content_width,
            app.file_content_scroll,
            visible_height,
            sel,
            Style::default().fg(TEXT),
            select_bg,
        );

        let content_block = Block::default()
            .title(" Content ")
            .title_style(Style::default().fg(ACCENT))
            .borders(Borders::TOP)
            .border_style(Style::default().fg(BORDER));
        frame.render_widget(
            Paragraph::new(Text::from(content_lines)).block(content_block),
            content_area,
        );

        if total_lines > visible_height {
            let mut state = ScrollbarState::new(total_lines).position(app.file_content_scroll);
            let sb = Scrollbar::default()
                .orientation(ScrollbarOrientation::VerticalRight)
                .style(Style::default().fg(TEXT_MUTED));
            frame.render_stateful_widget(sb, content_area, &mut state);
        }
    } else {
        let empty_block = Block::default()
            .title(" Content ")
            .title_style(Style::default().fg(ACCENT))
            .borders(Borders::TOP)
            .border_style(Style::default().fg(BORDER));
        frame.render_widget(
            Paragraph::new(Span::styled("No file selected.", Style::default().fg(TEXT_MUTED)))
                .alignment(Alignment::Center)
                .block(empty_block),
            content_area,
        );
    }
}

// ── Panel: Logs ──

fn render_logs_panel(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .title(" Logs ")
        .title_style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER));
    let inner = area.inner(Margin::new(1, 1));
    frame.render_widget(block, area);

    let visible_height = inner.height as usize;
    let total = app.logs.len();
    let scroll = app.log_scroll.min(total.saturating_sub(1));

    let start = scroll;
    let end = (scroll + visible_height).min(total);

    let log_lines: Vec<Line> = app
        .logs
        .iter()
        .skip(start)
        .take(end.saturating_sub(start))
        .map(|entry| {
            let color = match entry.level {
                LogLevel::Info => INFO,
                LogLevel::Success => SUCCESS,
                LogLevel::Warning => WARNING,
                LogLevel::Error => ERROR,
            };
            Line::from(vec![
                Span::styled(
                    format!("{:10}", entry.source),
                    Style::default().fg(ACCENT_DIM).add_modifier(Modifier::BOLD),
                ),
                Span::styled(" ", Style::default()),
                Span::styled(&entry.message, Style::default().fg(color)),
            ])
        })
        .collect();

    frame.render_widget(Paragraph::new(log_lines), inner);

    if total > visible_height {
        let mut state = ScrollbarState::new(total).position(scroll);
        let sb = Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight)
            .style(Style::default().fg(TEXT_MUTED));
        frame.render_stateful_widget(sb, area, &mut state);
    }
}

// ── Panel: Config (Settings + Providers) ──

fn render_config_panel(frame: &mut Frame, app: &mut App, area: Rect) {
    let block = Block::default()
        .title(" Config ")
        .title_style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER));
    let inner = area.inner(Margin::new(1, 1));
    frame.render_widget(block, area);

    // Tab bar at the top of the panel.
    let tab_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(inner);

    let tabs = vec![ConfigTab::Settings, ConfigTab::Providers];
    let tab_spans: Vec<Span> = tabs
        .iter()
        .enumerate()
        .map(|(i, tab)| {
            let label = match tab {
                ConfigTab::Settings => "Settings",
                ConfigTab::Providers => "Providers",
            };
            let is_active = app.config_tab == *tab;
            let style = if is_active {
                Style::default()
                    .fg(ACCENT_BRIGHT)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(TEXT_MUTED)
            };
            let sep = if i < tabs.len() - 1 { " │ " } else { "" };
            Span::styled(format!("{}{}", label, sep), style)
        })
        .collect();
    let tab_line = Paragraph::new(Line::from(tab_spans)).alignment(Alignment::Center);
    frame.render_widget(tab_line, tab_layout[0]);

    match app.config_tab {
        ConfigTab::Settings => render_settings_inner(frame, app, tab_layout[1]),
        ConfigTab::Providers => render_providers_inner(frame, app, tab_layout[1]),
    }
}

fn render_settings_inner(frame: &mut Frame, app: &mut App, area: Rect) {
    app.panel_rect = Some(area);
    let settings: Vec<(&str, String)> = vec![
        ("Animation Speed", format!("{:?}", app.animation_speed)),
        (
            "Mouse Support",
            if app.mouse_enabled {
                "On"
            } else {
                "Off"
            }
            .to_string(),
        ),
        ("Log Filter", format!("{:?}", app.log_filter)),
        ("Theme", format!("{:?}", app.theme)),
        ("Copy Defer", format!("{}00 ms", app.copy_defer_duration)),
    ];

    let items: Vec<ListItem> = settings
        .iter()
        .enumerate()
        .map(|(i, (label, value))| {
            let is_selected = i == app.settings_cursor;
            let is_hovered = app.settings_hover == Some(i);
            let marker = if is_selected { "▸ " } else { "  " };
            let style = if is_selected {
                Style::default()
                    .fg(ACCENT_BRIGHT)
                    .bg(SURFACE)
                    .add_modifier(Modifier::BOLD)
            } else if is_hovered {
                Style::default().fg(TEXT).bg(SURFACE_HOVER)
            } else {
                Style::default().fg(TEXT)
            };
            let value_style = if is_selected {
                Style::default().fg(ACCENT)
            } else {
                Style::default().fg(ACCENT_DIM)
            };
            let content = Line::from(vec![
                Span::styled(marker, Style::default().fg(ACCENT)),
                Span::styled(format!("{:20}", label), style),
                Span::styled(format!("{:30}", value), value_style),
            ]);
            ListItem::new(content)
        })
        .collect();

    let list_area = Rect::new(area.x, area.y, area.width, area.height.saturating_sub(1));
    let list = List::new(items).block(Block::default());
    frame.render_widget(list, list_area);

    // Hint bar.
    let hint = Paragraph::new(Line::from(vec![
        Span::styled("↑↓/j/k ", Style::default().fg(TEXT_MUTED)),
        Span::styled("Navigate", Style::default().fg(TEXT_SECONDARY)),
        Span::raw("  |  "),
        Span::styled("Enter/Space ", Style::default().fg(TEXT_MUTED)),
        Span::styled("Cycle", Style::default().fg(TEXT_SECONDARY)),
        Span::raw("  |  "),
        Span::styled("Tab ", Style::default().fg(TEXT_MUTED)),
        Span::styled("Providers", Style::default().fg(TEXT_SECONDARY)),
    ]))
    .alignment(Alignment::Center);
    frame.render_widget(hint, Rect::new(area.x, area.y + area.height.saturating_sub(1), area.width, 1));
}

fn render_providers_inner(frame: &mut Frame, app: &mut App, area: Rect) {
    let inner = area;
    app.panel_rect = Some(inner);

    if app.provider_detail_view.is_some() {
        render_provider_detail(frame, app, inner);
    } else if app.provider_create_view.is_some() {
        render_provider_create(frame, app, inner);
    } else {
        render_provider_list_view(frame, app, inner);
    }
}

/// List view: shows all providers + add templates. ↑↓ always navigates list.
fn render_provider_list_view(frame: &mut Frame, app: &mut App, area: Rect) {
    let n = app.providers.len();
    let total = if n > 0 { n + 1 + 5 } else { 5 };

    // Clamp hover to valid range.
    if app.provider_list_hover.is_some_and(|h| h >= total) {
        app.provider_list_hover = None;
    }

    // Build all list rows.
    let mut rows: Vec<Line> = Vec::new();

    for (i, provider) in app.providers.iter().enumerate() {
        let is_selected = app.provider_list_cursor == i;
        let is_active = app.active_provider == Some(provider.id);
        let is_hovered = app.provider_list_hover == Some(i);
        let marker = if is_selected { "▸ " } else { "  " };
        let active_dot = if is_active { " ●" } else { "  " };
        let style = if is_selected {
            Style::default()
                .fg(ACCENT_BRIGHT)
                .bg(SURFACE)
                .add_modifier(Modifier::BOLD)
        } else if is_hovered {
            Style::default().fg(TEXT).bg(SURFACE_HOVER)
        } else {
            Style::default().fg(TEXT)
        };
        let active_style = if is_active {
            Style::default().fg(SUCCESS)
        } else {
            Style::default().fg(TEXT_SECONDARY)
        };
        rows.push(Line::from(vec![
            Span::styled(marker, Style::default().fg(ACCENT)),
            Span::styled(&provider.name, style),
            Span::styled(active_dot, active_style),
        ]));
    }

    // Separator.
    if n > 0 {
        rows.push(Line::from(vec![Span::styled(
            "─── Add Provider ───",
            Style::default().fg(TEXT_MUTED),
        )]));
    }

    // Add-provider templates.
    let add_start = if n > 0 { n + 1 } else { 0 };
    for (di, pt) in [
        ProviderType::GenericOpenAI,
        ProviderType::GenericAnthropic,
        ProviderType::LMStudio,
        ProviderType::Ollama,
        ProviderType::LlamaCpp,
    ]
    .iter()
    .enumerate()
    {
        let row_idx = add_start + di;
        let is_selected = app.provider_list_cursor == row_idx;
        let is_hovered = app.provider_list_hover == Some(row_idx);
        let marker = if is_selected { "▸ " } else { "  " };
        let style = if is_selected {
            Style::default()
                .fg(ACCENT_BRIGHT)
                .bg(SURFACE)
                .add_modifier(Modifier::BOLD)
        } else if is_hovered {
            Style::default().fg(TEXT).bg(SURFACE_HOVER)
        } else {
            Style::default().fg(TEXT_SECONDARY)
        };
        rows.push(Line::from(vec![
            Span::styled(marker, Style::default().fg(TEXT_MUTED)),
            Span::styled(format!("[+] {}", pt), style),
        ]));
    }

    let visible = rows.into_iter().take(area.height.saturating_sub(1) as usize).collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(visible), area);

    // Hint bar at bottom.
    let hint_y = area.y + area.height.saturating_sub(1);
    let hint = Paragraph::new(Line::from(vec![
        Span::styled("↑↓/j/k ", Style::default().fg(TEXT_MUTED)),
        Span::styled("Navigate", Style::default().fg(TEXT_SECONDARY)),
        Span::raw("  |  "),
        Span::styled("Enter ", Style::default().fg(TEXT_MUTED)),
        Span::styled("Open", Style::default().fg(TEXT_SECONDARY)),
        Span::raw("  |  "),
        Span::styled("Esc ", Style::default().fg(TEXT_MUTED)),
        Span::styled("Close", Style::default().fg(TEXT_SECONDARY)),
        Span::raw("  |  "),
        Span::styled("d ", Style::default().fg(TEXT_MUTED)),
        Span::styled("Delete", Style::default().fg(TEXT_SECONDARY)),
        Span::raw("  |  "),
        Span::styled("Tab ", Style::default().fg(TEXT_MUTED)),
        Span::styled("Settings", Style::default().fg(TEXT_SECONDARY)),
    ]));
    frame.render_widget(hint, Rect::new(area.x, hint_y, area.width, 1));
}

/// Detail view: shows one provider's config fields. ↑↓ cycles fields, Esc goes back.
fn render_provider_detail(frame: &mut Frame, app: &mut App, area: Rect) {
    let Some(id) = app.provider_detail_view else {
        return;
    };
    let Some(provider) = app.providers.iter().find(|p| p.id == id) else {
        app.exit_provider_view();
        return;
    };

    let block = Block::default()
        .title(format!(" {} ", provider.name))
        .title_style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER));
    frame.render_widget(block, area);
    let inner = area.inner(Margin::new(1, 1));

    // Active indicator.
    let is_active = app.active_provider == Some(id);
    let status_line = if is_active {
        Line::from(vec![
            Span::styled(
                "● Active",
                Style::default().fg(SUCCESS).add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(
                "(immediately used for LLM calls)",
                Style::default().fg(TEXT_MUTED),
            ),
        ])
    } else {
        Line::from(vec![
            Span::styled("○ Inactive", Style::default().fg(TEXT_SECONDARY)),
            Span::raw("  "),
            Span::styled("Press Enter to activate", Style::default().fg(TEXT_MUTED)),
        ])
    };
    frame.render_widget(
        Paragraph::new(status_line),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );

    // Field rows: 0=Name, 1=Type, 2=Model, 3=BaseUrl, 4=ApiKey, 5=Activate
    let field_start = inner.y + 2;
    let api_key_display = if provider.api_key.is_empty() {
        "(not set)".to_string()
    } else {
        "••••••".to_string()
    };
    let activate_label = if is_active { "● Active" } else { "○ Activate" };
    let editing = app.editing_field;
    let field_rows: Vec<Line> = vec![
        ("Name", provider.name.as_str()),
        ("Type", provider.provider_type.to_string().as_str()),
        ("Model", provider.model.as_str()),
        ("Base URL", provider.base_url.as_str()),
        ("API Key", provider.api_key.as_str()),
        ("", activate_label),
    ]
    .into_iter()
    .enumerate()
    .map(|(fi, (label, value))| {
        let is_focused = app.provider_detail_cursor == fi;
        let is_editing = editing == Some(fi);
        let value_style = if is_editing {
            // Edit mode: bright text on surface, inverted look
            Style::default()
                .fg(BG)
                .bg(ACCENT)
                .add_modifier(Modifier::BOLD)
        } else if is_focused {
            Style::default()
                .fg(TEXT)
                .bg(SURFACE)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(TEXT_SECONDARY)
        };
        let marker = if is_editing {
            "▸ "
        } else if is_focused {
            "▸ "
        } else {
            "   "
        };
        let display_value = if fi == 5 {
            // Activate row: no truncation
            value.to_string()
        } else if is_editing {
            // In edit mode: show the full value (no truncation) so user can see what they're typing
            value.to_string()
        } else if fi == 4 && editing != Some(4) {
            // API Key: show masked when not editing
            api_key_display.clone()
        } else if value.len() > inner.width as usize - 30 {
            format!("{}…", &value[..(inner.width as usize - 33).max(1)])
        } else {
            value.to_string()
        };
        Line::from(vec![
            Span::styled(
                marker,
                Style::default().fg(if is_editing {
                    ACCENT
                } else if is_focused {
                    ACCENT
                } else {
                    TEXT_MUTED
                }),
            ),
            Span::styled(
                format!("{:12}", label),
                Style::default().fg(if is_editing {
                    TEXT
                } else if is_focused {
                    TEXT
                } else {
                    TEXT_SECONDARY
                }),
            ),
            Span::styled(": ", Style::default().fg(TEXT_MUTED)),
            Span::styled(display_value, value_style),
        ])
    })
    .collect();
    let field_rows_h = field_rows.len() as u16;
    frame.render_widget(
        Paragraph::new(field_rows),
        Rect::new(inner.x, field_start, inner.width, field_rows_h),
    );

    // Delete confirmation overlay.
    if let Some(confirm_id) = app.provider_confirm_delete {
        let name = app
            .providers
            .iter()
            .find(|p| p.id == confirm_id)
            .map(|p| p.name.as_str())
            .unwrap_or("this provider");
        let confirm_block = Block::default()
            .title(" Confirm Delete ")
            .title_style(Style::default().fg(ERROR).add_modifier(Modifier::BOLD))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(ERROR));
        let confirm_text = Paragraph::new(Text::from(vec![
            Line::from(vec![
                Span::styled("Delete ", Style::default().fg(ERROR)),
                Span::styled(
                    name,
                    Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
                ),
                Span::styled("?", Style::default().fg(ERROR)),
            ]),
            Line::from(vec![
                Span::styled("Enter ", Style::default().fg(TEXT_MUTED)),
                Span::styled("Confirm  ", Style::default().fg(SUCCESS)),
                Span::styled("Esc ", Style::default().fg(TEXT_MUTED)),
                Span::styled("Cancel", Style::default().fg(TEXT_SECONDARY)),
            ]),
        ]))
        .alignment(Alignment::Center);
        let confirm_rect = Rect::new(
            inner.x + inner.width.saturating_sub(30) / 2,
            inner.y + inner.height.saturating_sub(5) / 2,
            30,
            5,
        );
        frame.render_widget(confirm_block, confirm_rect);
        frame.render_widget(confirm_text, confirm_rect.inner(Margin::new(1, 1)));
        return;
    }

    // Hint bar.
    let hint_y = inner.y + inner.height.saturating_sub(1);
    let hint_line = if app.provider_confirm_delete.is_some() {
        Line::from(vec![
            Span::styled("Enter", Style::default().fg(TEXT_MUTED)),
            Span::styled(" Confirm  ", Style::default().fg(SUCCESS)),
            Span::styled("Esc", Style::default().fg(TEXT_MUTED)),
            Span::styled(" Cancel", Style::default().fg(TEXT_SECONDARY)),
        ])
    } else if app.editing_field.is_some() {
        // Edit mode: show how to confirm/back out
        Line::from(vec![
            Span::styled("Enter ", Style::default().fg(TEXT_MUTED)),
            Span::styled("Confirm", Style::default().fg(ACCENT)),
            Span::raw("  |  "),
            Span::styled("Esc ", Style::default().fg(TEXT_MUTED)),
            Span::styled("Cancel Edit", Style::default().fg(TEXT_SECONDARY)),
        ])
    } else {
        Line::from(vec![
            Span::styled("↑↓/j/k ", Style::default().fg(TEXT_MUTED)),
            Span::styled("Navigate", Style::default().fg(TEXT_SECONDARY)),
            Span::raw("  |  "),
            Span::styled("Enter ", Style::default().fg(TEXT_MUTED)),
            Span::styled("Edit Field", Style::default().fg(ACCENT)),
            Span::raw("  |  "),
            Span::styled("Tab ", Style::default().fg(TEXT_MUTED)),
            Span::styled("Next", Style::default().fg(TEXT_SECONDARY)),
            Span::raw("  |  "),
            Span::styled("d ", Style::default().fg(TEXT_MUTED)),
            Span::styled("Delete", Style::default().fg(TEXT_SECONDARY)),
            Span::raw("  |  "),
            Span::styled("Esc ", Style::default().fg(TEXT_MUTED)),
            Span::styled("← Back", Style::default().fg(TEXT_SECONDARY)),
        ])
    };
    frame.render_widget(
        Paragraph::new(hint_line),
        Rect::new(inner.x, hint_y, inner.width, 1),
    );
}

/// Create view: shows a blank form for a provider template. ↑↓ cycles fields, Esc goes back.
fn render_provider_create(frame: &mut Frame, app: &mut App, area: Rect) {
    let Some(pt) = app.provider_create_view else {
        return;
    };

    let block = Block::default()
        .title(format!(" New {} ", pt))
        .title_style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER));
    frame.render_widget(block, area);
    let inner = area.inner(Margin::new(1, 1));

    // Active indicator (new providers are inactive by default).
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("○ Inactive", Style::default().fg(TEXT_SECONDARY)),
            Span::raw("  "),
            Span::styled("Save to activate", Style::default().fg(TEXT_MUTED)),
        ])),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );

    // Field rows: 0=Name, 1=Type, 2=Model, 3=BaseUrl, 4=ApiKey, 5=Save & Open
    let field_start = inner.y + 2;
    let editing = app.editing_field;
    let field_rows: Vec<Line> = vec![
        ("Name", app.provider_create_name.clone()),
        ("Type", pt.to_string()),
        ("Model", app.provider_create_model.clone()),
        ("Base URL", app.provider_create_base_url.clone()),
        ("API Key", app.provider_create_api_key.clone()),
        ("", "Save & Open →".to_string()),
    ]
    .into_iter()
    .enumerate()
    .map(|(fi, (label, value))| {
            let is_focused = app.provider_detail_cursor == fi;
            let is_editing = editing == Some(fi);
            let value_style = if is_editing {
                Style::default()
                    .fg(BG)
                    .bg(ACCENT)
                    .add_modifier(Modifier::BOLD)
            } else if is_focused {
                Style::default()
                    .fg(TEXT)
                    .bg(SURFACE)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(TEXT_SECONDARY)
            };
            let marker = if is_editing {
                "▸ "
            } else if is_focused {
                "▸ "
            } else {
                "   "
            };
            let display_value = if fi == 5 {
                value.clone()
            } else if is_editing {
                value.clone()
            } else if value.len() > inner.width as usize - 30 {
                format!("{}…", &value[..(inner.width as usize - 33).max(1)])
            } else {
                value
            };
            Line::from(vec![
                Span::styled(
                    marker,
                    Style::default().fg(if is_editing {
                        ACCENT
                    } else if is_focused {
                        ACCENT
                    } else {
                        TEXT_MUTED
                    }),
                ),
                Span::styled(
                    format!("{:12}", label),
                    Style::default().fg(if is_editing {
                        TEXT
                    } else if is_focused {
                        TEXT
                    } else {
                        TEXT_SECONDARY
                    }),
                ),
                Span::styled(": ", Style::default().fg(TEXT_MUTED)),
                Span::styled(display_value, value_style),
            ])
        })
        .collect();
    let field_rows_h = field_rows.len() as u16;
    frame.render_widget(
        Paragraph::new(field_rows),
        Rect::new(inner.x, field_start, inner.width, field_rows_h),
    );

    // Hint bar.
    let hint_y = inner.y + inner.height.saturating_sub(1);
    let hint_line = if app.editing_field.is_some() {
        Line::from(vec![
            Span::styled("Enter ", Style::default().fg(TEXT_MUTED)),
            Span::styled("Confirm", Style::default().fg(ACCENT)),
            Span::raw("  |  "),
            Span::styled("Esc ", Style::default().fg(TEXT_MUTED)),
            Span::styled("Cancel Edit", Style::default().fg(TEXT_SECONDARY)),
        ])
    } else {
        Line::from(vec![
            Span::styled("↑↓/j/k ", Style::default().fg(TEXT_MUTED)),
            Span::styled("Navigate", Style::default().fg(TEXT_SECONDARY)),
            Span::raw("  |  "),
            Span::styled("Enter ", Style::default().fg(TEXT_MUTED)),
            Span::styled("Edit Field", Style::default().fg(ACCENT)),
            Span::raw("  |  "),
            Span::styled("Tab ", Style::default().fg(TEXT_MUTED)),
            Span::styled("Next", Style::default().fg(TEXT_SECONDARY)),
            Span::raw("  |  "),
            Span::styled("Esc ", Style::default().fg(TEXT_MUTED)),
            Span::styled("← Back", Style::default().fg(TEXT_SECONDARY)),
        ])
    };
    frame.render_widget(
        Paragraph::new(hint_line),
        Rect::new(inner.x, hint_y, inner.width, 1),
    );
}

// ── Panel: Graph ──

fn render_graph_panel(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .title(" Graph ")
        .title_style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER));
    let inner = area.inner(Margin::new(1, 1));
    frame.render_widget(block, area);

    let node_count = app.code_graph.nodes().len();
    let edge_count: usize = app
        .code_graph
        .nodes()
        .keys()
        .map(|id| app.code_graph.get_outgoing(id).len())
        .sum();

    let stats_lines = vec![
        Line::from(vec![
            Span::styled("Nodes: ", Style::default().fg(TEXT_SECONDARY)),
            Span::styled(
                node_count.to_string(),
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::styled("     ", Style::default()),
            Span::styled("Edges: ", Style::default().fg(TEXT_SECONDARY)),
            Span::styled(
                edge_count.to_string(),
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("Files in workspace: ", Style::default().fg(TEXT_SECONDARY)),
            Span::styled(
                app.orchestrator.file_contents.len().to_string(),
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
        ]),
    ];

    let mut all_lines = stats_lines;
    all_lines.push(Line::from(Span::styled("", Style::default())));
    all_lines.push(Line::from(
        Span::styled("Top Symbols (by PageRank):", Style::default().fg(TEXT).add_modifier(Modifier::BOLD)),
    ));

    let mut nodes: Vec<_> = app.code_graph.nodes().values().collect();
    let ranks = app.code_graph.page_rank(&[], 0.85, 1e-6, 50);
    nodes.sort_by(|a, b| {
        let ra = ranks.get(&b.id).unwrap_or(&0.0);
        let rb = ranks.get(&a.id).unwrap_or(&0.0);
        ra.partial_cmp(rb).unwrap()
    });

    for node in nodes.iter().take(inner.height.saturating_sub(5) as usize) {
        let sym = &node.symbol;
        let pr = ranks.get(&node.id).unwrap_or(&0.0);
        all_lines.push(Line::from(vec![
            Span::styled(format!("{:20}", sym.name), Style::default().fg(TEXT)),
            Span::styled(format!("{:?}", sym.kind), Style::default().fg(ACCENT_DIM)),
            Span::styled(format!("  pr={:.4}", pr), Style::default().fg(TEXT_MUTED)),
        ]));
    }

    frame.render_widget(Paragraph::new(all_lines), inner);
}

// ── Helper: wrap plain text and apply a selection highlight to overlapping portions ──

fn lines_with_selection<'a>(
    text: &'a str,
    width: u16,
    scroll: usize,
    visible_height: usize,
    selection: Option<(usize, usize)>,
    base_style: Style,
    select_bg: Color,
) -> Vec<Line<'a>> {
    let sel = selection.map(|(s, e)| (s.min(e), s.max(e)));
    let wrapped = wrap_text(text, width);
    let mut lines = Vec::new();

    for (start, end) in wrapped.iter().skip(scroll).take(visible_height) {
        let line_text = &text[*start..*end];
        if let Some((sel_start, sel_end)) = sel
            && sel_start < *end
            && sel_end > *start
        {
            let sel_start_in_line = sel_start.saturating_sub(*start).min(line_text.len());
            let sel_end_in_line = sel_end.min(*end).saturating_sub(*start).min(line_text.len());
            let mut spans = Vec::new();
            if sel_start_in_line > 0 {
                spans.push(Span::styled(&line_text[..sel_start_in_line], base_style));
            }
            if sel_end_in_line > sel_start_in_line {
                let sel_style = base_style.bg(select_bg);
                spans.push(Span::styled(
                    &line_text[sel_start_in_line..sel_end_in_line],
                    sel_style,
                ));
            }
            if sel_end_in_line < line_text.len() {
                spans.push(Span::styled(&line_text[sel_end_in_line..], base_style));
            }
            lines.push(Line::from(spans));
            continue;
        }
        lines.push(Line::from(Span::styled(line_text, base_style)));
    }
    lines
}
