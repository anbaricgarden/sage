# Workspace Redesign — TUI Architecture

## Status

Proposed 2026-06-01. In progress.

---

## Problem

The current 7-screen sidebar model treats a coding assistant like a settings app. It fragments the core workflow (type task → see results → inspect files) across opaque screens, wastes 25% of terminal width on a permanent sidebar, and buries sage's key differentiator (the 4-agent pipeline) in a Dashboard screen that isn't even the default.

## Solution: Workspace + Panels + Overlays

Replace the screen-based navigation with a **workspace-first** model inspired by Claude Code but extended for multi-agent orchestration:

- **Default view is the workspace** — task input + agent pipeline + conversation body.
- **Everything else is a toggleable panel or overlay** — files (Ctrl+F), logs (Ctrl+L), config (Ctrl+,).
- **No sidebar.** Full terminal width for content.
- **Agent pipeline always visible** — sage's signature UI element.

---

## Layout

```
┌─ sage! ───────────────────────── [LM Studio · codestral] ── [⠹ Planning] ─┐
│                                                                              │
│  Planner   →   Editor   →   Executor   →   Reviewer                          │
│  ⠹ active      ○ idle       ○ idle         ○ idle                           │
│                                                                              │
│  I'll refactor the TUI events module into smaller, focused handlers.         │
│                                                                              │
│  ┌─ Plan ───────────────────────────────────────────────────────────────┐   │
│  │  1. Extract provider key handling into provider_events.rs             │   │
│  │  2. Extract task input handling into task_events.rs                   │   │
│  │  3. Update mod.rs                                                     │   │
│  └──────────────────────────────────────────────────────────────────────┘   │
│                                                                              │
│  ⠹ Editor is generating diffs...                                            │
│                                                                              │
│ > Refactor the TUI events module                                            │
│                                                                              │
│  /files  /logs  /config  /help  │  v0.1.0  │  Tokens: 1.2k  │  Ctrl+C quit  │
└──────────────────────────────────────────────────────────────────────────────┘
```

### Zones (top to bottom)

| Zone | Height | Contents |
|------|--------|----------|
| **Top bar** | 1 row | Logo + app name, active provider/model, orchestrator state indicator |
| **Pipeline strip** | 2 rows | 4 agent cards: Planner → Editor → Executor → Reviewer, each showing status |
| **Conversation body** | fill | Agent output: plans, diffs, results. Scrollable. Supports text selection. |
| **Task input** | 1+ rows | Multi-line input (Shift+Enter for newline). Slash commands on `/`. |
| **Bottom bar** | 1 row | Available slash commands, version, token usage, quit hint |

### Panel States (overlay on conversation body)

| Panel | Trigger | Behavior |
|-------|---------|----------|
| **Files** | `Ctrl+F` or `/files` | Slides in from left (30%). Tree + filter + content viewer. |
| **Logs** | `Ctrl+L` or `/logs` | Slides up from bottom (resizable). Filterable log stream. |
| **Config** | `Ctrl+,` or `/config` | Centered modal overlay. Providers (left) + Settings (right). |

---

## State Changes (app.rs)

### Removed

- `screen: Screen` — replaced by workspace model
- `Screen` enum — no more screen switching
- `sidebar_rect`, `sidebar_hover` — no sidebar
- `settings_rect`, `settings_hover` — settings merged into config overlay
- `provider_rect`, `provider_list_hover` — providers merged into config overlay
- All screen-specific rects consolidated

### Added

```rust
/// Which panel (if any) is currently overlaid on the workspace.
pub enum Panel {
    None,           // Default workspace
    Files,           // File tree + content viewer (left overlay)
    Logs,            // Log viewer (bottom overlay)
    Config,          // Settings + Providers (centered overlay)
    Graph,           // Debug graph view (overlay)
}

pub panel: Panel,
pub task_input_focused: bool,  // true at launch, toggled by Esc
pub slash_command: Option<String>, // e.g. "files", "logs", "config"
```

### Kept (with modifications)

- `providers`, `active_provider` — unchanged data model
- `provider_detail_view`, `provider_create_view`, `provider_detail_cursor`, `editing_field` — unchanged, now rendered inside Config overlay
- `logs`, `max_logs`, `log_scroll`, `log_filter` — unchanged, now rendered in Logs panel
- `task_input`, `task_cursor`, `task_input_focused` — unchanged
- `selected_file`, `file_scroll`, `expanded_dirs`, `file_filter` — unchanged, now rendered in Files panel
- `orchestrator`, `code_graph` — unchanged
- `running`, `last_result`, `should_quit` — unchanged
- `selection`, `copy_flash_ticks`, `copy_defer_ticks` — unchanged
- `theme`, `animation_speed`, `mouse_enabled` — unchanged

---

## Navigation Model

### Global shortcuts (always available)

| Key | Action |
|-----|--------|
| `Ctrl+C` | Quit |
| `Ctrl+F` | Toggle Files panel |
| `Ctrl+L` | Toggle Logs panel |
| `Ctrl+,` | Toggle Config overlay |
| `Esc` | Close active panel, or stop running agent, or blur task input |
| `Ctrl+G` | Toggle Graph debug view |

### Task input (when focused)

| Key | Action |
|-----|--------|
| `Enter` | Submit task |
| `Shift+Enter` | Newline |
| `Esc` | Blur (defocus) input |
| `Ctrl+A` | Select all |
| Arrow keys | Cursor movement |
| Characters | Type text |
| `/` at start | Enter slash command mode |

### Slash commands

When the task input starts with `/`, a completion hint appears:

| Command | Action |
|---------|--------|
| `/files` | Toggle Files panel |
| `/logs` | Toggle Logs panel |
| `/config` | Open Config overlay |
| `/graph` | Toggle Graph debug view |
| `/clear` | Clear conversation |
| `/help` | Show help overlay |
| `/quit` | Quit sage |

### Conversation body (when task input is blurred)

| Key | Action |
|-----|--------|
| `j` / `↓` | Scroll down |
| `k` / `↑` | Scroll up |
| `PgUp` / `PgDn` | Page scroll |
| `Esc` | Focus task input |
| `i` or `/` | Focus task input (vim-like) |
| Mouse wheel | Scroll |

---

## Visual Design

### Color Palette (sage theme)

```
BG:             #1c1c22   (deep charcoal)
SURFACE:        #282830   (card/panel background)
SURFACE_HOVER:  #32323c   (hover highlight)
ACCENT:         #9caf88   (sage green)
ACCENT_DIM:     #788c64   (muted sage)
ACCENT_BRIGHT:  #b9cda5   (bright sage)
TEXT:           #dcdce0   (primary text)
TEXT_SECONDARY: #9696a0   (secondary text)
TEXT_MUTED:     #64646e   (muted/placeholder)
ERROR:          #dc7878   (red)
WARNING:        #dcb464   (yellow/amber)
SUCCESS:        #8cc88c   (green)
INFO:           #82aad2   (blue)
BORDER:         #3c3c46   (panel borders)
BORDER_ACTIVE:  #9caf88   (focused border)
SELECT_BG:      #37374b   (text selection)
```

### Top Bar

- Left: `sage!` logo in ACCENT + bold
- Center/right: `[{provider_short_name} · {model}]` in INFO
- Far right: `[orchestrator_state_indicator]` with spinner when running
- Background: SURFACE

### Pipeline Strip

- 4 equally-spaced agent cards in a horizontal row
- Each card: agent name + status icon + state label
- Active agent: ACCENT_BRIGHT background, spinner icon
- Done agent: ✓ in SUCCESS
- Idle agent: ○ in TEXT_MUTED
- Error agent: ✗ in ERROR
- Separated by `→` arrows in TEXT_MUTED

### Conversation Body

- Agent output scrolls naturally, newest at bottom
- Plan blocks: bordered in INFO
- Diff blocks: +/- colored (SUCCESS green for additions, ERROR red for removals)
- Status/thinking messages: dimmed, spinner-prefixed
- Text is selectable with mouse

### Task Input

- Bordered box at bottom of conversation area
- Border: BORDER_ACTIVE when focused, BORDER when blurred
- Hint text inside when empty: "Describe your coding task... (Enter to submit, Shift+Enter for newline, / for commands)"
- Multi-line, auto-scrolls

### Bottom Bar

- Left: available slash commands (contextual, based on open panels)
- Center: version
- Right: token usage + quit hint
- Background: SURFACE

### Panel: Files (left overlay, 30% width)

- Vertical split: tree (left 30%) + content (right 70%)
- Tree: same file tree as current Files screen
- Content: syntax-highlighted file preview
- Filter: `/` to focus, Esc to blur
- Ctrl+F closes the panel

### Panel: Logs (bottom overlay, resizable)

- Slides up from bottom, taking ~40% of conversation area
- Header: "Logs" + filter dropdown
- Scrollable log entries with colored severity
- Auto-scrolls to newest unless user has scrolled up
- Ctrl+L closes the panel

### Overlay: Config (centered modal)

- Dims background
- Split left (providers) / right (settings)
- Left: provider list + add templates (same as current)
- Right: settings toggles (same as current)
- Click-to-expand provider detail works identically
- Esc closes the overlay

---

## Keyboard Reference (Complete)

| Context | Key | Action |
|---------|-----|--------|
| **Global** | `Ctrl+C` | Quit |
| **Global** | `Ctrl+F` | Toggle Files panel |
| **Global** | `Ctrl+L` | Toggle Logs panel |
| **Global** | `Ctrl+,` | Toggle Config overlay |
| **Global** | `Ctrl+G` | Toggle Graph debug view |
| **Global** | `Esc` | Close panel / stop agent / blur input |
| **Task input (focused)** | `Enter` | Submit task |
| **Task input (focused)** | `Shift+Enter` | Insert newline |
| **Task input (focused)** | `Esc` | Blur input |
| **Task input (focused)** | `/` at start | Slash command mode |
| **Conversation (blurred)** | `j` / `↓` | Scroll down |
| **Conversation (blurred)** | `k` / `↑` | Scroll up |
| **Conversation (blurred)** | `PgUp` / `PgDn` | Page scroll |
| **Conversation (blurred)** | `i` or `/` | Focus input |
| **Files panel** | `j` / `k` / `↑` / `↓` | Navigate tree |
| **Files panel** | `Enter` | Open file |
| **Files panel** | `/` | Focus filter |
| **Files panel** | `Esc` | Close panel |
| **Logs panel** | `j` / `k` / `↑` / `↓` | Scroll |
| **Logs panel** | `Esc` | Close panel |
| **Config overlay** | `Esc` | Close overlay |
| **Config - Provider list** | `↑↓` / `Enter` / `d` | Same as current |
| **Config - Provider detail** | `↑↓` / `Enter` / `Esc` / typing | Same as current |
| **Config - Settings** | `↑↓` / `Enter` | Same as current |

---

## Files Modified

- `src/tui/app.rs` — State: remove Screen, add Panel, restructure
- `src/tui/ui.rs` — Layout: complete rewrite
- `src/tui/events.rs` — Keys: new shortcuts, slash commands
- `src/tui/run.rs` — Startup message update
- `tests/orchestrator_tests.rs` — Update if referencing Screen
- `tests/integration_tests.rs` — Update if referencing Screen

---

## Implementation Order

1. Write spec (this document)
2. Refactor `app.rs` state
3. Rewrite `ui.rs` layout engine
4. Rewrite `events.rs` key handling
5. Update `run.rs`
6. Build, fix type errors
7. Run tests, fix test failures
8. Manual smoke test
9. Commit
