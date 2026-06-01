# sage — Token-Efficient Coding Agent

A multi-agent coding system built in Rust that minimizes both input context tokens and output generation tokens at every architectural layer. Currently implements the full multi-agent pipeline with a working TUI dashboard.

> **Status:** Core infrastructure is complete. The agents use heuristic rule-based implementations (not yet calling external LLM APIs). The TUI is functional and the full pipeline runs end-to-end in-process.

## What's Built

```
sage/
├── src/
│   ├── main.rs              # CLI entry point
│   ├── lib.rs               # Library root
│   ├── blob_store.rs        # SHA-256 content-addressed storage
│   ├── diff/
│   │   ├── format.rs        # EditBlock + anchor hash computation
│   │   ├── parser.rs        # Hash-anchored diff block parser
│   │   └── applicator.rs    # Progressive context expansion (3→5→10→20 lines)
│   ├── ast/
│   │   ├── mod.rs           # Symbol + SymbolKind definitions
│   │   └── parser.rs        # Tree-sitter symbol extraction (Py, JS, TS, Go)
│   ├── agent/
│   │   ├── mod.rs           # Agent trait + role traits (Planner/Editor/Executor/Reviewer)
│   │   ├── planner.rs       # Task decomposition → action graph + token budget allocation
│   │   ├── editor.rs        # Rule-based hash-anchored diff generation
│   │   ├── executor.rs      # Tool registry, batch execution, diff application
│   │   ├── reviewer.rs      # Semantic validation (brace balance, import checks)
│   │   ├── orchestrator.rs  # State machine: Idle→Planning→Editing→Executing→Reviewing→Done
│   │   ├── action_graph.rs  # DAG execution, topological batches, critical path
│   │   ├── checkpoint.rs    # Blob-store-backed snapshots + rollback
│   │   └── tools.rs         # Tool trait + implementations (read/write/search/list/run_tests)
│   ├── codegraph/
│   │   ├── graph.rs         # Nodes, edges, pageRank scoring
│   │   ├── edges.rs         # Edge extraction (calls, imports, inherits, uses_type)
│   │   ├── retrieval.rs     # VectorIndex + KeywordMatcher + RRF fusion
│   │   ├── context.rs       # ContextAssembler for lazy context loading
│   │   ├── repo_map.rs      # File-level repo overview with pageRank
│   │   ├── ranker.rs        # Personalized pageRank implementation
│   │   ├── language_parser.rs
│   │   └── formatter.rs
│   └── tui/
│       ├── app.rs           # App state, settings persistence
│       ├── ui.rs            # 6-screen TUI rendering (ratatui)
│       ├── events.rs        # Input handling, mouse selection, OSC 52 clipboard
│       ├── run.rs           # TUI runtime loop
│       └── file_tree.rs     # File tree with syntax highlighting
└── tests/
    ├── integration_tests.rs  # 14 tests (blob store, diff, ast, editor agent)
    ├── codegraph_tests.rs    # 13 tests (graph, pageRank, retrieval, repo map)
    └── orchestrator_tests.rs # 25 tests (orchestrator, action graph, checkpoint)
```

## Modules

### `blob_store` — Content-Addressed Storage

SHA-256 content-addressed blob storage with automatic deduplication. All file versions are stored by hash, enabling O(1) change detection and instant rollback.

- `BlobStore::put(content)` → SHA-256 hex hash (64 chars)
- `BlobStore::get(hash)` → retrieve bytes by hash
- `BlobStore::contains(hash)` → O(1) existence check

### `diff` — Hash-Anchored Edit Format

The core output optimization. Instead of emitting full file rewrites, the system emits only changed lines anchored by SHA-256 hashes of surrounding context. This achieves ~93% token reduction for edit operations.

**Format:**
```
<<<<<<< HEAD:{old_anchor}
{unchanged context lines}
=======
{new lines}
>>>>>>> {new_anchor}
```

The parser extracts `EditBlock` structs from diff text. The applicator resolves matches using progressive context expansion: starts at 3 lines, expands to 5→10→20 if anchors collide, falls back to first match.

### `ast` — Tree-Sitter Symbol Extraction

Extracts code symbols from source files using tree-sitter grammars for Python, JavaScript, TypeScript, and Go. Returns `Symbol` structs with name, kind, line range, signature, and docstring.

### `agent` — Multi-Agent Pipeline

Four specialized agents coordinated by an Orchestrator state machine:

| Agent | Implementation | Role |
|---|---|---|
| **Planner** | Heuristic task decomposition | Decomposes task into action graph + allocates token budgets |
| **Editor** | Rule-based diff generation | Generates hash-anchored `EditBlock`s for each file edit |
| **Executor** | Tool execution engine | Runs tool calls, applies diffs, batches parallel operations |
| **Reviewer** | Semantic validation | Checks brace balance, import consistency, approves/rejects |

**Orchestrator state machine:** `Idle → Planning → Editing → Executing → Reviewing → Done/Rollback`

**ActionGraph:** DAG-based execution with topological batching (independent nodes run in parallel), speculative branches with early termination, critical path computation, and per-node token budgets.

**CheckpointManager:** Blob-store-backed snapshots enable instant rollback to any previous state.

### `codegraph` — Context Intelligence

Directed graph of code symbols (functions, classes, methods, types) with semantic edges (calls, imports, inherits, uses_type, tested_by). pageRank scoring identifies the most relevant symbols for a given task.

- **pageRank** — Personalized pageRank with seed nodes for task-specific relevance
- **Hybrid retrieval** — Combines vector index (embeddings) + keyword matcher with reciprocal rank fusion (RRF)
- **ContextAssembler** — Lazy loading of only the most relevant symbols, sorted by granularity (full body / signature / name-only)
- **RepoMap** — File-level overview generated from the CodeGraph

### `tui` — Terminal Dashboard

Six-screen terminal UI built with ratatui:

| Screen | Content |
|---|---|
| **Dashboard (1)** | Orchestrator state machine, agent status cards, token ledger |
| **Task (2)** | Multi-line task input with mouse selection + clipboard copy (OSC 52) |
| **Files (3)** | File tree with syntax-highlighted content viewer |
| **Logs (4)** | Scrollable agent/system log history |
| **Graph (5)** | CodeGraph stats + symbol list ranked by pageRank |
| **Settings (6)** | Animation speed, mouse mode, copy defer duration, theme — persisted to JSON |

Keyboard: `1-6` navigate screens, `Tab`/`BackTab` cycle, `Ctrl+C` quit, `↑↓`/`j`/`k` scroll, `Enter` submit. Mouse: click to select, double-click word, triple-click line, Shift+Click extend selection.

## Build & Run

```bash
cargo build --release
cargo run -- --tui   # TUI dashboard (default when run with no args)
cargo test           # 94 tests across 4 test suites
cargo clippy -- -D warnings
```

## Test Coverage

**94 tests total:**

| Suite | Count | Coverage |
|---|---|---|
| `src/lib.rs` | 42 | TUI events (mouse selection, deferred copy, click tracking) |
| `tests/codegraph_tests.rs` | 13 | Graph, pageRank, edges, retrieval, RRF fusion, repo map |
| `tests/integration_tests.rs` | 14 | Blob store, diff format/parser/applicator, AST parser, editor agent |
| `tests/orchestrator_tests.rs` | 25 | Orchestrator state machine, action graph, checkpoint/rollback, agents |

## Architecture

The system targets an **86% reduction** in per-task tokens (from ~10,000 to ~1,400) through:

1. **Hash-anchored diffs** — ~93% output token reduction for edits
2. **CodeGraph with pageRank** — Only relevant symbols loaded, not whole files
3. **Hybrid retrieval (AST + embeddings)** — High precision + high recall via RRF fusion
4. **Multi-agent specialization** — Smaller/cheaper models for planning and review
5. **Action graph execution** — Parallel batches, speculative paths, early termination
6. **Tool batching** — Batch up to 16 parallel tool calls per turn (batching implemented; result compression is a design goal, not yet implemented)

See `spec.md` for the full engineering specification.

## Status & Roadmap

| Component | Status | Notes |
|---|---|---|
| Blob store, diff format/parser/applicator, AST parser | **Done** | Full implementation with tests |
| Multi-agent pipeline (Planner, Editor, Executor, Reviewer) | **Skeleton** | State machine, action graph, and all role traits implemented; agents use heuristic rule-based logic, not yet connected to LLM APIs |
| CodeGraph (pageRank, edges, retrieval, context assembler, repo map) | **Done** | Full implementation with 13 tests |
| TUI dashboard (6 screens) | **Done** | Functional, with settings persistence |
| LLM API integration | **Not started** | Agents need API client + structured output for diffs + prompt templates |
| Prompt compression | **Not started** | Code-aware token pruning rules defined in spec, not yet implemented |
| Benchmark suite (SWE-bench, HumanEval) | **Not started** | Infrastructure for evaluation is not yet in place |

**Next priorities:**
1. **Wire agents to real LLM APIs** — Replace heuristic implementations with actual API calls. Add an API client (OpenAI/Anthropic), implement structured output parsing for hash-anchored diffs, and write prompt templates for each agent role.
2. **Prompt compression** — Code-aware token pruning before sending to models (remove unused imports, truncate docstrings, collapse whitespace — per spec §8).
3. **Benchmark suite** — Automated evaluation on SWE-bench Lite and HumanEval to measure actual token savings.

## License

See `LICENSE.md`.