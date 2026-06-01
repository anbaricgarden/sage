# Providers Screen — Design Spec

## Overview

A standalone **Providers** screen (`Screen::Providers`) replaces the LLM rows that were bolted onto the Settings screen. It manages a list of named, configured LLM providers with immediate activation. Provider configuration is a first-class concern, not an afterthought.

---

## Data Model

### `ProviderType` Enum

Non-exhaustive for extensibility:

```rust
pub enum ProviderType {
    GenericOpenAI,   // templates → "Generic OpenAI"
    GenericAnthropic,// templates → "Generic Anthropic"
    LMStudio,
    Ollama,
    LlamaCpp,
}
```

Display names:
- `GenericOpenAI` → `"Generic OpenAI"` (template type — user names their own)
- `GenericAnthropic` → `"Generic Anthropic"`
- `LMStudio` → `"LM Studio"`
- `Ollama` → `"Ollama"`
- `LlamaCpp` → `"Llama.cpp"`

Default base URLs (pre-filled on provider creation):
- `GenericOpenAI`: empty (user must set `https://api.openai.com/v1` or custom)
- `GenericAnthropic`: empty (user must set `https://api.anthropic.com/v1` or custom)
- `LMStudio`: `"http://localhost:1234/v1"`
- `Ollama`: `"http://localhost:11434/v1"`
- `LlamaCpp`: `"http://localhost:8080/v1"`

### `ProviderEntry` Struct

```rust
pub struct ProviderEntry {
    pub id: u64,           // monotonic ID for stable identity across renames
    pub name: String,      // user-facing label, e.g. "Production OpenAI"
    pub provider_type: ProviderType,
    pub model: String,     // empty = user must fill in
    pub base_url: String,  // pre-filled defaults per type, user-editable
    pub api_key: String,   // empty = use env var at runtime
}
```

IDs are assigned by incrementing a counter. On load from JSON (which stores entries without IDs), assign IDs sequentially.

### `App` Providers State

```rust
// ── Providers ──────────────────────────────────────
pub providers: Vec<ProviderEntry>,
pub active_provider: Option<u64>,   // provider ID (not index — stable across edits)
pub provider_selected: Option<u64>, // provider ID selected in the list
pub provider_creating: Option<ProviderType>, // type being created (in "add" section)
pub provider_config_field: ProviderConfigField,
pub provider_edit_text: HashMap<usize, String>,
pub provider_edit_cursor: usize,
pub provider_confirm_delete: Option<u64>, // provider ID pending deletion confirm
pub provider_list_cursor: usize,          // cursor into the full list (incl. add section)
pub provider_list_hover: Option<usize>,
pub next_provider_id: u64,
```

The full list shown in the UI has two logical sections sharing a flat `Vec`:
- **Section 1 (indices 0..N)**: the user-created providers (sorted by name or creation order)
- **Section 2 (separator)**: the 5 add-provider template entries

`provider_list_cursor` ranges 0..(N+4) where the last 5 are always the template entries.

### `ProviderConfigField` Enum

```rust
pub enum ProviderConfigField {
    Name,
    Model,
    BaseUrl,
    ApiKey,
}
```

---

## Screen Layout

```
┌─ Providers ───────────────────────────────────────────────────────────┐
│                                                                          │
│  ┌─ Provider List ────────┐  ┌─ Configure ─────────────────────────┐   │
│  │                         │  │                                      │   │
│  │ ▸ Production OpenAI  ●  │  │  Name     [My Production GPT    ]   │   │
│  │   My Ollama             │  │  Type     OpenAI                    │   │
│  │   Local Llama.cpp       │  │  Model    [gpt-4o               ]   │   │
│  │                         │  │  Base URL[http://localhost/v1  ]   │   │
│  │ ─────────────────────── │  │  API Key [••••••••            ]   │   │
│  │                         │  │                                      │   │
│  │ [+ Generic OpenAI]      │  │  [Enter] Activate   [Tab] next field│   │
│  │ [+ Generic Anthropic]   │  │  [d] Delete                            │   │
│  │ [+ LM Studio]           │  └──────────────────────────────────────┘   │
│  │ [+ Ollama]              │                                            │
│  │ [+ Llama.cpp]           │                                            │
│  └─────────────────────────┘                                            │
│                                                                          │
│  ● = active (being used)   ↑↓ navigate   Enter configure   d delete     │
└──────────────────────────────────────────────────────────────────────────┘
```

- **Left panel** (~35% width): scrollable list. Section 1 shows user providers with `▸` on selected and `●` active indicator. Section 2 (after a visual separator/different background) shows the 5 add-provider template entries. Hover state on all entries.
- **Right panel** (~65% width): config form for the selected provider. When no provider is selected (or in creation mode), shows appropriate placeholder or the creation form.
- **Bottom hint bar**: context-sensitive shortcuts.

### Config Panel States

1. **No provider selected**: shows "Select a provider from the list to configure it"
2. **Provider selected, not in creation mode**: shows editable form with current values
3. **Creation mode** (provider just created, name is empty and focused): shows the same form, all fields editable, name field is initially empty and focused

---

## Interactions

### List Navigation (`handle_providers_keys`)

| Key | Action |
|-----|--------|
| `↑` / `k` | Move `provider_list_cursor` up |
| `↓` / `j` | Move `provider_list_cursor` down |
| `Enter` / `Space` | **On user provider**: enter config mode (select it + open config panel) |
| `Enter` / `Space` | **On add-provider entry** (`provider_list_cursor` in section 2): create entry of that type, insert into list, switch to config mode with name field focused |
| `d` | **On user provider**: show deletion confirmation (status bar / inline prompt) |
| `Enter` (in confirm mode) | Confirm deletion, remove provider |
| `Esc` (in confirm mode or config mode) | Cancel confirm or cancel config, return to list view |
| `Tab` / `Shift+Tab` | Cycle through fields in config panel (Name → Model → Base URL → API Key → Name) |
| `←` / `→` (in config, Type field) | Cycle through provider types |
| `←` / `→` (in config, Model field) | Cycle through preset models for current type (openai: gpt-4o ↔ gpt-4o-mini; anthropic: claude-3-5-sonnet ↔ claude-3-5-haiku; others: no presets — free-text) |
| `Enter` (in config mode, on any field) | Activate provider (save + set as active + return to list view) |

### Config Panel Field Behavior

- **Name**: free-text, no presets
- **Type**: rendered as text (e.g. `"OpenAI"`), `←/→` cycles through types (shows type name, not enum variant)
- **Model**: free-text input with `←/→` cycling presets for openai/anthropic
- **Base URL**: free-text input, pre-filled with defaults on creation
- **API Key**: free-text input, masked with `•` in display, actual value stored

### Deletion Confirmation

When `d` is pressed on a user provider:
- The config panel (or a prominent inline area) shows: `"Delete '{name}'? [Enter] Confirm  [Esc] Cancel"`
- Only `Enter` (confirm) or `Esc` (cancel) is accepted
- If it was the active provider, `active_provider` is cleared and orchestrator LLM client is rebuilt without an LLM

### Activation

Pressing `Enter` in config mode (or `Enter` on a list item that already has valid config) immediately:
1. Saves the provider entry
2. Sets it as `active_provider`
3. Rebuilds the orchestrator's LLM client from the entry
4. Returns to list view
5. Shows status: `"Activated: {name}"`

### Mouse Interactions

- Click on a user provider: select + enter config mode
- Click on an add-provider entry: create + enter config mode
- Click on a field in config panel: focus that field
- Hover: highlight

---

## Persistence

`SettingsData` gets a `providers: Vec<ProviderEntry>` field (`#[serde(default)]`). `active_provider` is stored as the provider ID (u64).

On load:
1. Load `providers` from JSON, assign sequential IDs to each
2. Set `next_provider_id = providers.len() as u64`
3. Find the active provider by ID and set `active_provider`
4. Rebuild orchestrator LLM client from active provider

`LlmProviderSettings` and the old inline LLM settings rows are **removed** from `SettingsData`.

---

## Removal from Settings Screen

The 9 LLM-related rows are removed from the Settings screen. Settings retains only:
- Animation Speed (row 0)
- Mouse Support (row 1)
- Log Filter (row 2)
- Theme (row 3)
- Copy Defer Duration (row 4)

`SETTINGS_COUNT` becomes 5. Tests that referenced LLM settings are updated.

---

## Initial State

On first launch (no `providers` in settings JSON), initialize with an empty `providers` list. The user creates providers from scratch using the add section.

---

## Future: Model Query (deferred)

For LM Studio / Ollama / LlamaCpp, a "Query Models" button/shortcut would `GET {base_url}/models` (or `/v1/models`) and populate the model field. This is out of scope for v1 but the `base_url` field is stored and the infrastructure is ready.