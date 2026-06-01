# Providers Screen — Click-to-Expand Redesign

## Status

Proposed 2026-05-31. Approved by user.

---

## Problem

The current two-panel Providers screen has two usability issues:

1. **Dual-role navigation:** `↑↓` means "navigate the list" when nothing is selected, but "cycle through config fields" when a provider is selected. Users have to constantly re-orient to which mode they're in.

2. **Decoupled state:** `provider_list_cursor` and `provider_selected` are independent — the cursor can be on provider A while the config panel shows provider B. This creates confusion about which provider you're actually editing.

The add-templates inline in the list are fine with proper visual distinction.

---

## Solution: Click-to-Expand (Single Panel)

Replace the two-panel split (list 35% | config 65%) with a single full-width panel that has three view states. `↑↓` always means one thing per state:

| State | Trigger | `↑↓` behavior | Other keys |
|---|---|---|---|
| **List** | Default view | Navigate the list | `Enter`/`Click` on provider → Detail; `Enter`/`Click` on template → Create |
| **Detail** | Enter on existing provider | Cycle through config fields | `Tab` next field, type to edit, `d` delete, `Esc` back to list |
| **Create** | Enter on add-template | Cycle through config fields | `Tab` next field, type to edit, `Enter` save, `Esc` cancel |

**Layout:** Single panel, full width. No more split. List view shows all items; Detail/Create views replace the list content with a form.

---

## View Specifications

### List View

Full panel shows a scrollable list:

```
Providers

  ▸ My OpenAI Key     ●
    Another Provider
    Local Llama

  ─── Add Provider ───

    [+] Generic OpenAI
    [+] Anthropic
    [+] LM Studio
    [+] Ollama
    [+] Llama.cpp

↑↓ Navigate  Enter Select  d Delete
```

- Provider rows: show name + active indicator (`●`) if this provider is `active_provider`
- `▸` marker = cursor position (`provider_list_cursor`)
- Separator line with muted text when providers exist
- Add templates: muted style, `[+]` prefix, sorted by ProviderType
- Active dot (`●`) in SUCCESS color when `active_provider == Some(id)`
- `Esc` does nothing in list view (nothing to deselect)
- Single-click or Enter on a provider row → **Detail view**
- Single-click or Enter on a template row → **Create view**

### Detail View

Replaces the list with the selected provider's config form:

```
← Back                    My OpenAI Key          ● Active

▸ Name       : My OpenAI Key
  Type       : GenericOpenAI
  Model      : gpt-4o
  Base URL   : https://api.openai.com/v1
  API Key    : ••••••••••••••••

↑↓ Field  Tab Next  Type to edit  d Delete  Esc Back
```

- Header: `← Back` on left, provider name + active status on right
- `▸` marker on the focused field row (cycles through Name/Model/Base URL/API Key on `↑↓`)
- Type row is display-only (no `▸`, grayed label/value)
- `↑↓` cycles field focus: Name → Model → Base URL → API Key → Name
- `Tab` advances to next field (same cycle as `↑↓`)
- `Esc` → return to **List view** (no changes saved automatically; edits were live-saved)
- `d` → show delete confirmation overlay

**Delete confirmation overlay:**
```
┌─ Confirm Delete ────────────────┐
│ Delete My OpenAI Key?           │
│                                 │
│ Enter Confirm  Esc Cancel       │
└─────────────────────────────────┘
```
- `Enter` → delete provider, return to list
- `Esc` → dismiss overlay, back to detail view

### Create View

Shown when Enter on an add-template. Pre-fills defaults from the template:

```
← Back                    New GenericOpenAI

▸ Name       : New GenericOpenAI
  Type       : GenericOpenAI
  Model      :
  Base URL   : https://api.openai.com/v1
  API Key    :

↑↓ Field  Tab Next  Type to edit  Enter Save  Esc Cancel
```

- Header: `← Back` on left, `New {ProviderType}` name on right
- `Name` pre-filled as `New {ProviderType}`, cursor starts on `Name` field
- `Type` display-only, shows the template type
- `Model` and `API Key` empty, `Base URL` pre-filled from `pt.default_base_url()`
- `↑↓` / `Tab` cycle through Name/Model/Base URL/API Key
- `Enter` → create provider with entered values, enter **Detail view** for the new provider, activate it
- `Esc` → cancel creation, return to **List view**

---

## State Model

**New fields added to `App`:**

| Field | Type | Description |
|---|---|---|
| `provider_view` | `ProviderView` enum | Current view: `List`, `Detail(id)`, `Create(pt)` |
| `provider_detail_cursor` | `ProviderConfigField` | Which field is focused in Detail/Create view |

**Fields removed (decoupled state eliminated):**
- `provider_selected` — replaced by `provider_view`
- `provider_creating` — replaced by `provider_view`
- `provider_config_field` — replaced by `provider_detail_cursor`

**Fields kept:**
- `provider_list_cursor: usize` — cursor position in List view only
- `provider_list_hover: Option<usize>` — mouse hover in List view only
- `providers: Vec<ProviderEntry>` — provider data
- `active_provider: Option<u64>` — which provider is active for LLM calls
- `provider_confirm_delete: Option<u64>` — delete confirmation state (overlays Detail view)
- `provider_rect: Option<Rect>` — hit-testing rect for list (no config rect needed)

**Behavior rules:**
- `provider_list_cursor` only used/updated while in **List** view
- `↑↓` in List view: `provider_list_cursor = clamp(cursor ± 1, 0, total-1)`, cursor wraps
- `Enter`/`Click` in List view: set `provider_view` to `Detail(id)` or `Create(pt)`
- `↑↓` in Detail/Create view: cycle `provider_detail_cursor` through Name→Model→BaseUrl→ApiKey→Name
- `Tab` in Detail/Create view: same cycle as `↑↓`
- `Esc` in Detail view: set `provider_view = List`, clear `provider_confirm_delete`
- `Esc` in Create view: set `provider_view = List`, no provider created
- `Enter` in Create view: create provider, set `provider_view = Detail(new_id)`, activate it

---

## Visual Specification

**Color palette (same as existing):**
- `ACCENT`: sage green — markers, active indicators
- `ACCENT_BRIGHT`: lighter sage — selected rows
- `TEXT`: white-ish — normal text
- `TEXT_SECONDARY`: gray — secondary labels, inactive dots
- `TEXT_MUTED`: darker gray — separators, placeholders
- `SURFACE`: panel background
- `SURFACE_HOVER`: hover highlight
- `SUCCESS`: green — active dot
- `ERROR`: red — delete, error states

**Markers:**
- List view cursor: `▸` in ACCENT color (same as other screens)
- Detail/Create field focus: `▸` in ACCENT color on focused row, value in SURFACE+bold
- Active provider: `●` in SUCCESS color at end of name row
- Inactive provider: `○` in TEXT_SECONDARY at end of name row

**Fonts/weights:** Same as existing TUI (no changes)

---

## Keyboard Reference

| Key | List View | Detail View | Create View |
|---|---|---|---|
| `↑` / `k` | Move cursor up (wrap) | Cycle field up | Cycle field up |
| `↓` / `j` | Move cursor down (wrap) | Cycle field down | Cycle field down |
| `Enter` | Open provider detail OR create from template | (no action — activate already done) | Save & create provider |
| `Tab` | (no action) | Advance to next field | Advance to next field |
| `Esc` | (no action) | Back to list | Cancel, back to list |
| `d` | Delete (no confirmation in list) | Show delete confirmation | (no action) |
| `Backspace` | (no action) | Delete char from focused field | Delete char from focused field |
| `a-z, etc.` | (no action) | Append character to focused field | Append character to focused field |

**Mouse:**
- Click provider row → enter Detail view
- Click template row → enter Create view
- Click field row in Detail/Create → focus that field
- Hover → highlight row in List view (already fixed off-by-one)

---

## Scope of Changes

**Files modified:**
- `src/tui/app.rs` — state fields, `delete_provider`, etc.
- `src/tui/ui.rs` — `render_providers`, `render_provider_list`, `render_provider_config` consolidated into single `render_providers` function
- `src/tui/events.rs` — `handle_providers_keys`, `handle_mouse_down`, `handle_mouse_moved`

**Files NOT modified:**
- `src/agent/orchestrator.rs` — provider activation logic unchanged
- `src/blob_store.rs` — unchanged
- All other screens — unchanged

**Backwards compatibility:**
- Settings file format for providers unchanged (still `Vec<ProviderEntry>` as JSON)
- `active_provider` persisted the same way

---

## Implementation Order

1. Change `App` state fields in `app.rs` (add `provider_view` enum, remove decoupled fields)
2. Update `handle_providers_keys` in `events.rs` — new navigation logic
3. Update `handle_mouse_down` / `handle_mouse_moved` in `events.rs`
4. Rewrite `render_providers` in `ui.rs` — single-panel three-state rendering
5. Delete `render_provider_list` and `render_provider_config` (merged into `render_providers`)
6. Run tests, fix any failures
7. Update any integration tests that reference old state fields