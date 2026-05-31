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

use super::app::{App, LogLevel, Screen, SelectionSource, StatusKind};
use super::events::{byte_index_to_visual_pos, wrap_text};
use super::file_tree::build_visible_tree;
use crate::agent::orchestrator::OrchestratorState;

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

    let h_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(20), Constraint::Min(0)])
        .split(area);

    let sidebar_area = h_layout[0];
    let main_area = h_layout[1];

    render_sidebar(frame, app, sidebar_area);

    let v_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(main_area);

    let content_area = v_layout[0];
    let status_area = v_layout[1];

    // Content with inner padding.
    let inner = content_area.inner(Margin::new(1, 1));
    match app.screen {
        Screen::Dashboard => render_dashboard(frame, app, inner),
        Screen::Task => render_task(frame, app, inner),
        Screen::Files => render_files(frame, app, inner),
        Screen::Logs => render_logs(frame, app, inner),
        Screen::Graph => render_graph(frame, app, inner),
        Screen::Settings => render_settings(frame, app, inner),
    }

    render_status_bar(frame, app, status_area);
}

// ── Sidebar ──

fn render_sidebar(frame: &mut Frame, app: &mut App, area: Rect) {
    app.sidebar_rect = Some(area);

    let block = Block::default()
        .borders(Borders::RIGHT)
        .border_style(Style::default().fg(BORDER));
    frame.render_widget(block, area);

    let inner = area.inner(Margin::new(1, 1));

    let version = env!("CARGO_PKG_VERSION");
    let title = Paragraph::new(Text::from(vec![
        Line::from(Span::styled("◆", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))),
        Line::from(Span::styled("sage", Style::default().fg(TEXT).add_modifier(Modifier::BOLD))),
        Line::from(Span::styled(format!("v{}", version), Style::default().fg(TEXT_MUTED))),
    ]))
    .alignment(Alignment::Center);

    let title_h = 3;
    frame.render_widget(title, Rect::new(inner.x, inner.y, inner.width, title_h));

    let items: Vec<ListItem> = Screen::all()
        .iter()
        .enumerate()
        .map(|(i, screen)| {
            let is_active = *screen == app.screen;
            let is_hovered = app.sidebar_hover == Some(i);
            let shortcut = format!("{}", i + 1);
            let label = screen.title();
            let style = if is_active {
                Style::default()
                    .fg(ACCENT_BRIGHT)
                    .bg(SURFACE)
                    .add_modifier(Modifier::BOLD)
            } else if is_hovered {
                Style::default().fg(TEXT).bg(SURFACE_HOVER)
            } else {
                Style::default().fg(TEXT_SECONDARY)
            };
            let content = Line::from(vec![
                Span::styled(format!(" {} ", shortcut), Style::default().fg(TEXT_MUTED)),
                Span::styled(label, style),
            ]);
            ListItem::new(content).style(style)
        })
        .collect();

    let list = List::new(items)
        .highlight_symbol(" ")
        .block(Block::default());

    let list_area = Rect::new(
        inner.x,
        inner.y + title_h + 1,
        inner.width,
        inner.height - title_h - 1,
    );
    frame.render_widget(list, list_area);

    // Update sidebar rect to cover only the clickable list area for hit-testing.
    app.sidebar_rect = Some(list_area);
}

// ── Dashboard ──

fn render_dashboard(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // state header
            Constraint::Length(12), // agent cards
            Constraint::Length(8), // token budget
            Constraint::Min(0),    // history / recent
        ])
        .split(area);

    // State machine header.
    let state_block = Block::default()
        .title(" Orchestrator State ")
        .title_style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER));
    let state_text = Paragraph::new(state_indicator(&app.orchestrator.state))
        .block(state_block)
        .alignment(Alignment::Center);
    frame.render_widget(state_text, chunks[0]);

    // Agent status cards.
    let agent_block = Block::default()
        .title(" Agents ")
        .title_style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER));
    let agent_inner = chunks[1].inner(Margin::new(1, 1));
    frame.render_widget(agent_block, chunks[1]);

    let statuses = app.agent_statuses();
    let agent_cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(vec![Constraint::Percentage(25); statuses.len()])
        .split(agent_inner);

    for (i, (name, label, active)) in statuses.iter().enumerate() {
        let color = if *active {
            ACCENT
        } else if *label == "Done" {
            SUCCESS
        } else {
            TEXT_MUTED
        };
        let card = Paragraph::new(Text::from(vec![
            Line::from(Span::styled(*name, Style::default().fg(TEXT).add_modifier(Modifier::BOLD))),
            Line::from(Span::styled(*label, Style::default().fg(color))),
        ]))
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(if *active { BORDER_ACTIVE } else { BORDER })),
        );
        frame.render_widget(card, agent_cols[i]);
    }

    // Token budget.
    let token_block = Block::default()
        .title(" Token Ledger ")
        .title_style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER));
    let token_inner = chunks[2].inner(Margin::new(1, 1));
    frame.render_widget(token_block, chunks[2]);

    let mut token_lines: Vec<Line> = Vec::new();
    if app.orchestrator.token_ledger.is_empty() {
        token_lines.push(Line::from(Span::styled(
            "No tokens consumed yet.",
            Style::default().fg(TEXT_MUTED),
        )));
    } else {
        let total: usize = app.orchestrator.token_ledger.values().sum();
        for (agent, tokens) in &app.orchestrator.token_ledger {
            let pct = (*tokens as f64 / total.max(1) as f64) * 100.0;
            let bar_width = (pct / 100.0 * token_inner.width.saturating_sub(20) as f64) as usize;
            let bar = "█".repeat(bar_width);
            token_lines.push(Line::from(vec![
                Span::styled(format!("{:12}", agent), Style::default().fg(TEXT)),
                Span::styled(bar, Style::default().fg(ACCENT)),
                Span::styled(
                    format!(" {:>6} tokens ({:.1}%)", tokens, pct),
                    Style::default().fg(TEXT_SECONDARY),
                ),
            ]));
        }
        token_lines.push(Line::from(Span::styled(
            format!("Total: {} tokens", total),
            Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
        )));
    }
    frame.render_widget(Paragraph::new(token_lines), token_inner);

    // Recent history.
    let hist_block = Block::default()
        .title(" Recent Activity ")
        .title_style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER));
    let hist_inner = chunks[3].inner(Margin::new(1, 1));
    frame.render_widget(hist_block, chunks[3]);

    let hist_lines: Vec<Line> = app
        .orchestrator
        .history
        .iter()
        .rev()
        .take(hist_inner.height as usize)
        .map(|h| Line::from(Span::styled(h.clone(), Style::default().fg(TEXT_SECONDARY))))
        .collect();
    frame.render_widget(Paragraph::new(hist_lines), hist_inner);
}

fn state_indicator(state: &OrchestratorState) -> Text<'_> {
    let (emoji, color, desc) = match state {
        OrchestratorState::Idle => ("◆", TEXT_MUTED, "Idle — waiting for task"),
        OrchestratorState::Planning => ("◈", INFO, "Planning — decomposing task"),
        OrchestratorState::Editing => ("◈", ACCENT, "Editing — generating diffs"),
        OrchestratorState::Executing => ("◈", WARNING, "Executing — applying changes"),
        OrchestratorState::Reviewing => ("◈", SUCCESS, "Reviewing — validating results"),
        OrchestratorState::Done => ("◆", SUCCESS, "Done — task completed"),
        OrchestratorState::Rollback => ("◆", ERROR, "Rollback — reverting changes"),
    };
    Text::from(vec![
        Line::from(vec![
            Span::styled(emoji, Style::default().fg(color).add_modifier(Modifier::BOLD)),
            Span::styled(" ", Style::default()),
            Span::styled(
                format!("{:?}", state),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(Span::styled(desc, Style::default().fg(TEXT_SECONDARY))),
    ])
}

// ── Task Screen ──

fn render_task(frame: &mut Frame, app: &mut App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // header
            Constraint::Length(12), // multi-line input
            Constraint::Length(3),  // actions
            Constraint::Min(0),     // result
        ])
        .split(area);

    // Header.
    let header = Paragraph::new(Line::from(vec![
        Span::styled("Enter a coding task for the multi-agent pipeline.", Style::default().fg(TEXT_SECONDARY)),
    ]))
    .block(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(BORDER)),
    );
    frame.render_widget(header, chunks[0]);

    // Input box.
    let input_style = if app.task_input_focused {
        Style::default().fg(TEXT).bg(SURFACE)
    } else {
        Style::default().fg(TEXT).bg(BG)
    };
    let input_block = Block::default()
        .title(" Task Description ")
        .title_style(Style::default().fg(ACCENT))
        .borders(Borders::ALL)
        .border_style(if app.task_input_focused {
            Style::default().fg(BORDER_ACTIVE)
        } else {
            Style::default().fg(BORDER)
        })
        .style(input_style);

    let input_inner = chunks[1].inner(Margin::new(1, 1));
    let width = input_inner.width;
    let visible_height = input_inner.height as usize;
    app.task_input_rect = Some(input_inner);

    // Compute wrapped lines and clamp scroll.
    let wrapped = wrap_text(&app.task_input, width);
    let total_lines = wrapped.len().max(1);
    app.task_scroll = app.task_scroll.min(total_lines.saturating_sub(visible_height));

    // Build visible lines with selection highlight.
    let sel = app.selection.as_ref().filter(|s| s.source == SelectionSource::TaskInput).map(|s| (s.start, s.end));
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
    if app.task_input.is_empty() && !app.task_input_focused && app.task_scroll == 0 {
        visible_lines = vec![Line::from(Span::styled(
            "Type a task and press Enter...",
            Style::default().fg(TEXT_MUTED),
        ))];
    }

    frame.render_widget(
        Paragraph::new(Text::from(visible_lines)).block(input_block),
        chunks[1],
    );

    // Draw cursor.
    if app.task_input_focused {
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

    // Actions.
    let action_text = if app.running {
        Line::from(vec![
            Span::styled(
                format!("{} Running...", app.spinner()),
                Style::default().fg(WARNING).add_modifier(Modifier::BOLD),
            ),
        ])
    } else {
        Line::from(vec![
            Span::styled("Enter ", Style::default().fg(TEXT_SECONDARY)),
            Span::styled("Submit", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
            Span::styled("  |  ", Style::default().fg(TEXT_MUTED)),
            Span::styled("Shift+Enter", Style::default().fg(ACCENT)),
            Span::styled(" Newline", Style::default().fg(TEXT_SECONDARY)),
            Span::styled("  |  ", Style::default().fg(TEXT_MUTED)),
            Span::styled("Ctrl+C", Style::default().fg(ACCENT)),
            Span::styled(" Quit", Style::default().fg(TEXT_SECONDARY)),
        ])
    };
    frame.render_widget(Paragraph::new(action_text).alignment(Alignment::Center), chunks[2]);

    // Result area.
    let result_block = Block::default()
        .title(" Result ")
        .title_style(Style::default().fg(ACCENT))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER));
    let result_inner = chunks[3].inner(Margin::new(1, 1));
    app.result_rect = Some(result_inner);

    if let Some(result) = &app.last_result {
        let result_width = result_inner.width;
        let result_visible_height = result_inner.height as usize;
        let total_lines = wrap_text(result, result_width).len();
        app.result_scroll = app.result_scroll.min(total_lines.saturating_sub(result_visible_height));

        let sel = app.selection.as_ref().filter(|s| s.source == SelectionSource::Result).map(|s| (s.start, s.end));
        let flash = app.copy_flash_ticks > 0;
        let select_bg = if flash { SELECT_FLASH_BG } else { SELECT_BG };
        let result_lines = lines_with_selection(
            result,
            result_width,
            app.result_scroll,
            result_visible_height,
            sel,
            Style::default().fg(TEXT),
            select_bg,
        );
        let result_text = Paragraph::new(Text::from(result_lines)).block(result_block);
        frame.render_widget(result_text, chunks[3]);

        if total_lines > result_visible_height {
            let mut state = ScrollbarState::new(total_lines).position(app.result_scroll);
            let sb = Scrollbar::default()
                .orientation(ScrollbarOrientation::VerticalRight)
                .style(Style::default().fg(TEXT_MUTED));
            frame.render_stateful_widget(sb, chunks[3], &mut state);
        }
    } else {
        frame.render_widget(result_block, chunks[3]);
    }
}

// ── Files Screen ──

fn render_files(frame: &mut Frame, app: &mut App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
        .split(area);

    // File tree panel.
    let tree_block = Block::default()
        .title(" Files ")
        .title_style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER));
    let tree_inner = chunks[0].inner(Margin::new(1, 1));
    frame.render_widget(tree_block, chunks[0]);

    // Split tree panel into filter box + list.
    let inner_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(tree_inner);

    let filter_area = inner_layout[0];
    let list_area = inner_layout[1];

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
    let filter_inner = filter_area.inner(Margin::new(1, 1));
    app.file_filter_rect = Some(filter_inner);
    frame.render_widget(filter_block, filter_area);

    let filter_text = if app.file_filter.is_empty() && !app.file_filter_focused {
        Line::from(Span::styled("Search files... (press /)", Style::default().fg(TEXT_MUTED)))
    } else {
        let sel = app.selection.as_ref().filter(|s| s.source == SelectionSource::FileFilter).map(|s| (s.start, s.end));
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
                Style::default().fg(ACCENT_BRIGHT).bg(SURFACE).add_modifier(Modifier::BOLD)
            } else if is_hovered {
                Style::default().fg(TEXT).bg(SURFACE_HOVER)
            } else if is_cursor {
                Style::default().fg(TEXT).bg(SURFACE)
            } else {
                Style::default().fg(TEXT_SECONDARY)
            };

            let indent = "  ".repeat(entry.depth);
            let prefix = if entry.is_dir {
                if entry.is_expanded { "▼ " } else { "▶ " }
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

    // Content viewer.
    let selected_title = app.selected_file.as_deref().unwrap_or("Content");
    let content_block = Block::default()
        .title(format!(" {} ", selected_title))
        .title_style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER));
    let content_inner = chunks[1].inner(Margin::new(1, 1));
    app.file_content_rect = Some(content_inner);
    frame.render_widget(content_block, chunks[1]);

    let content_text = if let Some(content) = app.selected_file.as_ref().and_then(|f| app.orchestrator.file_contents.get(f)) {
        let visible_height = content_inner.height as usize;
        let total_lines = wrap_text(content, content_inner.width).len();
        app.file_content_scroll = app.file_content_scroll.min(total_lines.saturating_sub(visible_height));

        let sel = app.selection.as_ref().filter(|s| s.source == SelectionSource::FileContent).map(|s| (s.start, s.end));
        let flash = app.copy_flash_ticks > 0;
        let select_bg = if flash { SELECT_FLASH_BG } else { SELECT_BG };
        let lines = render_file_content_with_selection(
            content,
            content_inner.width,
            app.file_content_scroll,
            visible_height,
            sel,
            select_bg,
        );
        Text::from(lines)
    } else {
        Text::from("Select a file to view its contents.")
    };

    frame.render_widget(Paragraph::new(content_text), content_inner);

    if let Some(content) = app.selected_file.as_ref().and_then(|f| app.orchestrator.file_contents.get(f)) {
        let total_lines = wrap_text(content, content_inner.width).len();
        let visible_height = content_inner.height as usize;
        if total_lines > visible_height {
            let mut state = ScrollbarState::new(total_lines).position(app.file_content_scroll);
            let sb = Scrollbar::default()
                .orientation(ScrollbarOrientation::VerticalRight)
                .style(Style::default().fg(TEXT_MUTED));
            frame.render_stateful_widget(sb, chunks[1], &mut state);
        }
    }
}

/// Wrap plain text and apply a selection highlight to overlapping portions.
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
        let line_text = &text[*start..*end];            if let Some((sel_start, sel_end)) = sel && sel_start < *end && sel_end > *start {
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

/// Render syntax-highlighted file content with an optional selection overlay.
fn render_file_content_with_selection<'a>(
    content: &'a str,
    width: u16,
    scroll: usize,
    visible_height: usize,
    selection: Option<(usize, usize)>,
    select_bg: Color,
) -> Vec<Line<'a>> {
    let sel = selection.map(|(s, e)| (s.min(e), s.max(e)));
    let mut result = Vec::new();
    let mut byte_offset = 0usize;

    for original_line in content.split('\n') {
        let line_len = original_line.len();

        let trimmed = original_line.trim_start();
        let base_style = if trimmed.starts_with("fn ") || trimmed.starts_with("pub fn ") {
            Style::default().fg(Color::Rgb(140, 200, 220))
        } else if trimmed.starts_with("struct ") || trimmed.starts_with("pub struct ") {
            Style::default().fg(Color::Rgb(220, 180, 140))
        } else if trimmed.starts_with("use ") || trimmed.starts_with("mod ") {
            Style::default().fg(Color::Rgb(180, 160, 220))
        } else if trimmed.starts_with("//") || trimmed.starts_with("///") || trimmed.starts_with("*") {
            Style::default().fg(TEXT_MUTED)
        } else if trimmed.starts_with("impl ") || trimmed.starts_with("trait ") {
            Style::default().fg(Color::Rgb(220, 200, 140))
        } else {
            Style::default().fg(TEXT)
        };

        let wrapped = wrap_text(original_line, width);
        for (seg_start, seg_end) in wrapped {
            let seg_abs_start = byte_offset + seg_start;
            let seg_abs_end = byte_offset + seg_end;
            let seg_text = &original_line[seg_start..seg_end];

            if let Some((sel_start, sel_end)) = sel && sel_start < seg_abs_end && sel_end > seg_abs_start {
                let sel_start_in_seg = sel_start.saturating_sub(seg_abs_start).min(seg_text.len());
                let sel_end_in_seg = sel_end.min(seg_abs_end).saturating_sub(seg_abs_start).min(seg_text.len());
                let mut spans = Vec::new();
                if sel_start_in_seg > 0 {
                    spans.push(Span::styled(&seg_text[..sel_start_in_seg], base_style));
                }
                if sel_end_in_seg > sel_start_in_seg {
                    let sel_style = base_style.bg(select_bg);
                    spans.push(Span::styled(
                        &seg_text[sel_start_in_seg..sel_end_in_seg],
                        sel_style,
                    ));
                }
                if sel_end_in_seg < seg_text.len() {
                    spans.push(Span::styled(&seg_text[sel_end_in_seg..], base_style));
                }
                result.push(Line::from(spans));
                continue;
            }
            result.push(Line::from(Span::styled(seg_text, base_style)));
        }

        byte_offset += line_len + 1; // +1 for \n
    }

    result.into_iter().skip(scroll).take(visible_height).collect()
}

// ── Logs Screen ──

fn render_logs(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .title(" System Logs ")
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

    // Scrollbar.
    if total > visible_height {
        let mut state = ScrollbarState::new(total).position(scroll);
        let sb = Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight)
            .style(Style::default().fg(TEXT_MUTED));
        frame.render_stateful_widget(sb, area, &mut state);
    }
}

// ── Graph Screen ──

fn render_graph(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(6), Constraint::Min(0)])
        .split(area);

    // Stats.
    let stats_block = Block::default()
        .title(" CodeGraph Stats ")
        .title_style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER));
    let stats_inner = chunks[0].inner(Margin::new(1, 1));
    frame.render_widget(stats_block, chunks[0]);

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
    frame.render_widget(Paragraph::new(stats_lines), stats_inner);

    // Node list.
    let list_block = Block::default()
        .title(" Symbols ")
        .title_style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER));
    let list_inner = chunks[1].inner(Margin::new(1, 1));
    frame.render_widget(list_block, chunks[1]);

    let mut nodes: Vec<_> = app.code_graph.nodes().values().collect();
    // Compute global pageRank for display (no seeds = uniform teleportation).
    let ranks = app.code_graph.page_rank(&[], 0.85, 1e-6, 50);
    nodes.sort_by(|a, b| {
        let ra = ranks.get(&b.id).unwrap_or(&0.0);
        let rb = ranks.get(&a.id).unwrap_or(&0.0);
        ra.partial_cmp(rb).unwrap()
    });

    let list_items: Vec<ListItem> = nodes
        .iter()
        .take(list_inner.height as usize)
        .map(|node| {
            let sym = &node.symbol;
            let style = Style::default().fg(TEXT_SECONDARY);
            let pr = ranks.get(&node.id).unwrap_or(&0.0);
            let content = Line::from(vec![
                Span::styled(format!("{:20}", sym.name), Style::default().fg(TEXT)),
                Span::styled(format!("{:?}", sym.kind), Style::default().fg(ACCENT_DIM)),
                Span::styled(
                    format!("  pr={:.4}", pr),
                    Style::default().fg(TEXT_MUTED),
                ),
            ]);
            ListItem::new(content).style(style)
        })
        .collect();

    let list = List::new(list_items).block(Block::default());
    frame.render_widget(list, list_inner);
}

// ── Settings Screen ──

fn render_settings(frame: &mut Frame, app: &mut App, area: Rect) {
    let block = Block::default()
        .title(" Settings ")
        .title_style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER));
    let inner = area.inner(Margin::new(1, 1));
    frame.render_widget(block, area);

    let settings: Vec<(&str, String)> = vec![
        (
            "Animation Speed",
            format!("{:?}", app.animation_speed),
        ),
        (
            "Mouse Support",
            if app.mouse_enabled { "On".to_string() } else { "Off".to_string() },
        ),
        (
            "Log Filter",
            format!("{:?}", app.log_filter),
        ),
        (
            "Theme",
            format!("{:?}", app.theme),
        ),
    ];

    // Reserve space for the hint at the bottom.
    let hint_height = 1;
    let list_area = Rect::new(inner.x, inner.y, inner.width, inner.height.saturating_sub(hint_height + 1));
    app.settings_rect = Some(list_area);

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
            let content = Line::from(vec![
                Span::styled(marker, Style::default().fg(ACCENT)),
                Span::styled(format!("{:20}", label), style),
                Span::styled(value.clone(), Style::default().fg(ACCENT)),
            ]);
            ListItem::new(content)
        })
        .collect();

    let list = List::new(items).block(Block::default());
    frame.render_widget(list, list_area);

    // Hint at the bottom.
    let hint = Paragraph::new(Line::from(vec![
        Span::styled("↑↓/j/k ", Style::default().fg(TEXT_MUTED)),
        Span::styled("Navigate  ", Style::default().fg(TEXT_SECONDARY)),
        Span::styled("Enter/Space/Click ", Style::default().fg(TEXT_MUTED)),
        Span::styled("Toggle", Style::default().fg(TEXT_SECONDARY)),
    ]))
    .alignment(Alignment::Center);
    let hint_area = Rect::new(inner.x, inner.y + inner.height - hint_height, inner.width, hint_height);
    frame.render_widget(hint, hint_area);
}

// ── Status Bar ──

fn render_status_bar(frame: &mut Frame, app: &App, area: Rect) {
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
            let hint = "Tab/1-6 Navigate  |  Enter Submit  |  Shift+Enter Newline  |  ↑↓ Scroll  |  Ctrl+C Quit";
            (hint, TEXT_MUTED)
        }
    };

    let bar = Paragraph::new(Line::from(Span::styled(msg, Style::default().fg(color))))
        .style(Style::default().bg(SURFACE))
        .block(Block::default());
    frame.render_widget(bar, area);
}
