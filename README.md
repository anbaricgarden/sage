# sage — Token-Efficient Coding Agent

A multi-agent coding system that calls API-based LLMs (Claude, GPT-4o, etc.) to minimize both input context tokens and output generation tokens at every architectural layer.

## Architecture

sage uses **four specialized sub-agents**, each calling an API-based LLM chosen for its task:

| Agent | API Model | Role |
|---|---|---|
| **Planner** | GPT-4o-mini or Claude 3.5 Haiku | Task decomposition, action graph generation |
| **Editor** | GPT-4o or Claude 3.5 Sonnet | Hash-anchored diff generation |
| **Executor** | GPT-4o-mini or Claude 3.5 Haiku | Parallel tool execution, result summarization |
| **Reviewer** | GPT-4o-mini or Claude 3.5 Haiku | Semantic validation, rollback decisions |

```
sage/
├── src/
│   ├── main.rs              # CLI entry point (demo)
│   ├── lib.rs               # Library root — re-exports all modules
│   ├── blob_store.rs        # SHA-256 content-addressed blob storage
│   ├── diff/
│   │   ├── mod.rs           # Diff module root
│   │   ├── format.rs        # EditBlock + anchor hash computation
│   │   ├── parser.rs        # Hash-anchored diff block parser
│   │   └── applicator.rs    # Progressive context expansion matcher
│   ├── ast/
│   │   ├── mod.rs           # Symbol + SymbolKind definitions
│   │   └── parser.rs        # Tree-sitter symbol extraction
│   └── agent/
│       ├── mod.rs           # Agent trait
│       └── editor.rs        # Rule-based diff generation
├── tests/
│   └── integration_tests.rs # End-to-end tests for all modules
└── spec.md                  # Full engineering specification
```

## Modules

### `blob_store` — Content-Addressed Storage

- `BlobStore::put(content)` → SHA-256 hex hash (64 chars)
- `BlobStore::get(hash)` → retrieve bytes by hash
- `BlobStore::contains(hash)` → O(1) existence check
- Automatic deduplication: identical content always yields the same hash

### `diff` — Hash-Anchored Edit Format

**Format:**

```text
<<<<<<< HEAD:{old_anchor}
{unchanged context lines}
=======
{new lines}
>>>>>>> {new_anchor}
```

- **Anchor hash:** first 8 characters of `SHA-256(file_path + "\n" + context_lines)`
- **Parser:** extracts `EditBlock`s from diff text via regex
- **Applicator:** progressive context expansion (3→5→10→20 lines) to resolve ambiguous anchors; falls back to first match if still ambiguous

### `ast` — Tree-Sitter Symbol Extraction

- Supports Python, JavaScript, TypeScript, Go
- Extracts: functions, classes, methods, structs, interfaces, types, constants
- Returns `Symbol` structs with name, kind, line range, signature, docstring

### `agent/editor` — Basic Rule-Based Editor (Phase 1)

- Parses natural-language tasks (e.g. "Change 'X' to 'Y'")
- Generates `EditBlock`s with proper anchor hashes
- Heuristic fallback for quoted-string replacement when literal match fails
- Part of the multi-agent pipeline; will be augmented by Planner / Executor / Reviewer in Phase 3

## Build & Run

```bash
# Build
cargo build --release

# Run the TUI dashboard
cargo run -- --tui

# Run the CLI demo (default)
cargo run

# Run tests
cargo test

# Lint
cargo clippy -- -D warnings
```

### TUI Dashboard

`sage --tui` launches a rich terminal interface with five screens:

| Screen | Content |
|---|---|
| **Dashboard (1)** | Orchestrator state machine, agent status cards, token ledger |
| **Task (2)** | Input coding tasks and watch the multi-agent pipeline execute |
| **Files (3)** | File tree with syntax-highlighted content viewer |
| **Logs (4)** | Scrollable agent/system log history |
| **Graph (5)** | CodeGraph stats + symbol list ranked by pageRank |
| **Settings (6)** | TUI preferences: animation speed, mouse, log filter, theme |

Keyboard shortcuts: `1-6` navigate screens, `Tab`/`BackTab` cycle, `Ctrl+C` quit, `↑↓`/`j`/`k` scroll, `Enter` submit tasks.

## Test Coverage

14 integration tests covering:

- Blob store round-trip & deduplication
- EditBlock anchor hash computation
- Diff parser (single & multiple blocks)
- Diff applicator (single-line change, line insertion, ambiguous anchor expansion, trailing newline preservation)
- AST parser (Python functions/classes, JavaScript functions)
- Editor agent (single-line fix, line addition)

## Roadmap

| Phase | Deliverable | Status |
|---|---|---|
| 1 | Foundation: blob store, diff format/parser/applicator, AST parser, Editor agent | **✅ Done** |
| 2 | Context Intelligence: CodeGraph with pageRank, hybrid retrieval, ContextAssembler, RepoMap | **✅ Done** |
| 3 | Multi-Agent Orchestration: Planner, Executor, Reviewer agents, Orchestrator state machine | **✅ Done** |
| 4 | Polish and Metrics: TUI dashboard, prompt compression, token accounting, benchmarks | **✅ Done** |

## License

See `LICENSE.md`.
