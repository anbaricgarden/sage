use std::collections::HashMap;

use sage::agent::action_graph::{ActionGraph, ActionNode, ActionType};
use sage::agent::checkpoint::CheckpointManager;
use sage::agent::editor::EditorAgent;
use sage::agent::executor::ExecutorAgent;
use sage::agent::orchestrator::{Orchestrator, OrchestratorState};
use sage::agent::planner::PlannerAgent;
use sage::agent::reviewer::{ReviewDecision, ReviewerAgent};
use sage::agent::Agent;
use sage::blob_store::BlobStore;
use sage::codegraph::graph::CodeGraph;
use sage::diff::format::EditBlock;

// ---------------------------------------------------------------------------
// ActionGraph tests
// ---------------------------------------------------------------------------

#[test]
fn action_graph_topological_batches_linear() {
    let mut graph = ActionGraph::new();
    let a = ActionNode::new("a", ActionType::Checkpoint);
    let b = ActionNode::new("b", ActionType::Validate {
        criteria: "c".to_string(),
    })
    .with_dependencies(vec!["a".to_string()]);
    let c = ActionNode::new("c", ActionType::Checkpoint).with_dependencies(vec!["b".to_string()]);

    graph.add_node(a);
    graph.add_node(b);
    graph.add_node(c);

    let batches = graph.topological_batches();
    assert_eq!(batches.len(), 3);
    assert!(batches[0].contains("a"));
    assert!(batches[1].contains("b"));
    assert!(batches[2].contains("c"));
}

#[test]
fn action_graph_topological_batches_parallel() {
    let mut graph = ActionGraph::new();
    let a = ActionNode::new("a", ActionType::Checkpoint);
    let b = ActionNode::new("b", ActionType::Checkpoint);
    let c = ActionNode::new("c", ActionType::Checkpoint).with_dependencies(vec![
        "a".to_string(),
        "b".to_string(),
    ]);

    graph.add_node(a);
    graph.add_node(b);
    graph.add_node(c);

    let batches = graph.topological_batches();
    assert_eq!(batches.len(), 2);
    assert!(batches[0].contains("a"));
    assert!(batches[0].contains("b"));
    assert!(batches[1].contains("c"));
}

#[test]
fn action_graph_critical_path() {
    let mut graph = ActionGraph::new();
    let a = ActionNode::new("a", ActionType::Checkpoint);
    let b = ActionNode::new("b", ActionType::Checkpoint).with_dependencies(vec!["a".to_string()]);
    let c = ActionNode::new("c", ActionType::Checkpoint).with_dependencies(vec!["b".to_string()]);

    graph.add_node(a);
    graph.add_node(b);
    graph.add_node(c);

    let path = graph.critical_path();
    assert_eq!(path.len(), 3);
    assert_eq!(path[0], "a");
    assert_eq!(path[1], "b");
    assert_eq!(path[2], "c");
}

#[test]
fn action_graph_cancel_branch() {
    let mut graph = ActionGraph::new();
    let a = ActionNode::new("a", ActionType::Checkpoint);
    let b = ActionNode::new("b", ActionType::Checkpoint).with_dependencies(vec!["a".to_string()]);
    let c = ActionNode::new("c", ActionType::Checkpoint).with_dependencies(vec!["b".to_string()]);

    graph.add_node(a);
    graph.add_node(b);
    graph.add_node(c);

    graph.cancel_branch("b");
    assert_eq!(graph.get_node("b").unwrap().succeeded, Some(false));
    assert_eq!(graph.get_node("c").unwrap().succeeded, Some(false));
    assert!(graph.get_node("a").unwrap().succeeded.is_none());
}

#[test]
fn action_graph_ready_nodes() {
    let mut graph = ActionGraph::new();
    let a = ActionNode::new("a", ActionType::Checkpoint);
    let b = ActionNode::new("b", ActionType::Checkpoint).with_dependencies(vec!["a".to_string()]);

    graph.add_node(a.clone());
    graph.add_node(b);

    // Node 'a' has no dependencies and is not yet executed, so it is ready.
    let ready = graph.ready_nodes();
    assert_eq!(ready, vec!["a"]);

    // Mark 'a' as succeeded.
    if let Some(n) = graph.get_node_mut("a") {
        n.succeeded = Some(true);
    }
    let ready = graph.ready_nodes();
    assert_eq!(ready, vec!["b"]);
}

#[test]
fn action_graph_budget_and_counts() {
    let mut graph = ActionGraph::new();
    graph.add_node(ActionNode::new("a", ActionType::Checkpoint).with_budget(100));
    graph.add_node(ActionNode::new("b", ActionType::Checkpoint).with_budget(200));

    assert_eq!(graph.total_budget(), 300);
    assert_eq!(graph.succeeded_count(), 0);
    assert_eq!(graph.failed_count(), 0);

    graph.get_node_mut("a").unwrap().succeeded = Some(true);
    graph.get_node_mut("b").unwrap().succeeded = Some(false);

    assert_eq!(graph.succeeded_count(), 1);
    assert_eq!(graph.failed_count(), 1);
    assert!(graph.is_complete());
}

// ---------------------------------------------------------------------------
// Checkpoint tests
// ---------------------------------------------------------------------------

#[test]
fn checkpoint_create_and_restore() {
    let mut mgr = CheckpointManager::new();
    let mut hashes = HashMap::new();
    hashes.insert("src/main.rs".to_string(), "abc123".to_string());

    let id = mgr.create("pre-edit", hashes, None);
    let cp = mgr.get(&id).unwrap();
    assert_eq!(cp.id, "pre-edit");
    assert_eq!(cp.file_hashes.get("src/main.rs"), Some(&"abc123".to_string()));
    assert!(cp.parent.is_none());
}

#[test]
fn checkpoint_restore_with_blob_store() {
    let store = BlobStore::new();
    let hash = store.put("fn main() {}".as_bytes().to_vec());

    let mut mgr = CheckpointManager::new();
    let mut hashes = HashMap::new();
    hashes.insert("src/main.rs".to_string(), hash.clone());
    mgr.create("snap", hashes, None);

    let contents = mgr.restore_contents("snap", &store).unwrap();
    assert_eq!(contents.get("src/main.rs"), Some(&"fn main() {}".to_string()));
}

#[test]
fn checkpoint_lineage() {
    let mut mgr = CheckpointManager::new();
    mgr.create("a", HashMap::new(), None);
    mgr.create("b", HashMap::new(), Some("a".to_string()));
    mgr.create("c", HashMap::new(), Some("b".to_string()));

    let lineage = mgr.lineage("c");
    assert_eq!(lineage, vec!["c", "b", "a"]);
}

// ---------------------------------------------------------------------------
// Planner tests
// ---------------------------------------------------------------------------

#[test]
fn planner_generates_non_empty_graph() {
    let planner = PlannerAgent::new();
    let graph = CodeGraph::new();
    let repo_map = "";

    let action_graph = planner.plan("Change 'Hello' to 'Hi'", repo_map, &graph).unwrap();
    assert!(!action_graph.nodes().is_empty());
}

#[test]
fn planner_budget_allocation() {
    let planner = PlannerAgent::new();
    let graph = CodeGraph::new();
    let repo_map = "";

    let action_graph = planner.plan("Fix bug in parser", repo_map, &graph).unwrap();
    let total_budget = action_graph.total_budget();
    assert!(total_budget > 0);
    assert!(total_budget <= planner.default_task_budget);
}

// ---------------------------------------------------------------------------
// Executor tests
// ---------------------------------------------------------------------------

#[test]
fn executor_tool_read_file() {
    let executor = ExecutorAgent::new();
    let mut files = HashMap::new();
    files.insert("demo.rs".to_string(), "fn main() {}\n".to_string());

    let node = ActionNode::new(
        "read",
        ActionType::ToolCall {
            tool: "read_file".to_string(),
            arguments: [("path".to_string(), "demo.rs".to_string())]
                .into_iter()
                .collect(),
        },
    );

    let store = BlobStore::new();
    let result = executor.execute(&node, &mut files, &store).unwrap();
    assert!(result.contains("fn main()"));
}

#[test]
fn executor_tool_write_file() {
    let executor = ExecutorAgent::new();
    let mut files = HashMap::new();

    let node = ActionNode::new(
        "write",
        ActionType::ToolCall {
            tool: "write_file".to_string(),
            arguments: [
                ("path".to_string(), "out.rs".to_string()),
                ("content".to_string(), "let x = 1;".to_string()),
            ]
            .into_iter()
            .collect(),
        },
    );

    let store = BlobStore::new();
    let result = executor.execute(&node, &mut files, &store).unwrap();
    assert!(result.contains("Wrote out.rs"));
    assert_eq!(files.get("out.rs"), Some(&"let x = 1;".to_string()));
}

#[test]
fn executor_apply_edit_block() {
    let executor = ExecutorAgent::new();
    let mut files = HashMap::new();
    files.insert("demo.rs".to_string(), "fn main() {\n    println!(\"old\");\n}\n".to_string());

    let block = EditBlock::compute_anchor("demo.rs", files.get("demo.rs").unwrap(), 1, 3, 3);

    let store = BlobStore::new();
    let result = executor.apply_edit_block("demo.rs", &block, &mut files, &store);
    assert!(result.is_ok());
}

#[test]
fn executor_batch_execution() {
    let executor = ExecutorAgent::new();
    let mut files = HashMap::new();
    files.insert("a.rs".to_string(), "// a".to_string());
    files.insert("b.rs".to_string(), "// b".to_string());

    let node1 = ActionNode::new(
        "r1",
        ActionType::ToolCall {
            tool: "read_file".to_string(),
            arguments: [("path".to_string(), "a.rs".to_string())]
                .into_iter()
                .collect(),
        },
    );
    let node2 = ActionNode::new(
        "r2",
        ActionType::ToolCall {
            tool: "read_file".to_string(),
            arguments: [("path".to_string(), "b.rs".to_string())]
                .into_iter()
                .collect(),
        },
    );

    let store = BlobStore::new();
    let results = executor.execute_batch(&[&node1, &node2], &mut files, &store);
    assert_eq!(results.len(), 2);
    assert!(results.get("r1").unwrap().is_ok());
    assert!(results.get("r2").unwrap().is_ok());
}

// ---------------------------------------------------------------------------
// Reviewer tests
// ---------------------------------------------------------------------------

#[test]
fn reviewer_approves_balanced_file() {
    let reviewer = ReviewerAgent::new();
    let mut files = HashMap::new();
    files.insert("ok.rs".to_string(), "fn main() {\n    println!(\"hi\");\n}\n".to_string());

    let node = ActionNode::new(
        "edit",
        ActionType::Edit {
            file_path: "ok.rs".to_string(),
            description: "print hi".to_string(),
        },
    );

    let decision = reviewer.review(&node, &files).unwrap();
    assert_eq!(decision, ReviewDecision::Approve);
}

#[test]
fn reviewer_rejects_unbalanced_braces() {
    let reviewer = ReviewerAgent::new();
    let mut files = HashMap::new();
    files.insert("bad.rs".to_string(), "fn main() {\n    println!(\"hi\");\n".to_string());

    let node = ActionNode::new(
        "edit",
        ActionType::Edit {
            file_path: "bad.rs".to_string(),
            description: "broken".to_string(),
        },
    );

    let result = reviewer.review(&node, &files);
    assert!(result.is_err());
}

#[test]
fn reviewer_rejects_empty_file() {
    let reviewer = ReviewerAgent::new();
    let mut files = HashMap::new();
    files.insert("empty.rs".to_string(), "".to_string());

    let node = ActionNode::new(
        "edit",
        ActionType::Edit {
            file_path: "empty.rs".to_string(),
            description: "deleted everything".to_string(),
        },
    );

    let result = reviewer.review(&node, &files);
    assert!(result.is_err());
}

#[test]
fn reviewer_summarize_turn() {
    let reviewer = ReviewerAgent::new();
    let mut node = ActionNode::new(
        "e1",
        ActionType::Edit {
            file_path: "main.rs".to_string(),
            description: "fix".to_string(),
        },
    );
    node.succeeded = Some(true);

    let summary = reviewer.summarize_turn(&node);
    assert!(summary.contains("Edited main.rs"));
    assert!(summary.contains("succeeded"));
}

#[test]
fn reviewer_summarize_phase() {
    let reviewer = ReviewerAgent::new();
    let mut graph = ActionGraph::new();
    let mut a = ActionNode::new("a", ActionType::Checkpoint);
    a.succeeded = Some(true);
    let mut b = ActionNode::new("b", ActionType::Checkpoint);
    b.succeeded = Some(false);
    graph.add_node(a);
    graph.add_node(b);

    let summary = reviewer.summarize_phase(&graph);
    assert!(summary.contains("1/2 actions succeeded"));
    assert!(summary.contains("1 failed"));
}

// ---------------------------------------------------------------------------
// Orchestrator end-to-end tests
// ---------------------------------------------------------------------------

#[test]
fn orchestrator_state_transitions() {
    let mut orch = Orchestrator::new();
    assert_eq!(orch.state, OrchestratorState::Idle);

    orch.ingest_file("main.rs", "fn main() {\n    println!(\"Hello\");\n}\n");

    // Build a minimal code graph so planner has something to work with.
    let mut graph = CodeGraph::new();
    let sym = sage::ast::Symbol {
        name: "main".to_string(),
        kind: sage::ast::SymbolKind::Function,
        file_path: "main.rs".to_string(),
        start_line: 1,
        end_line: 3,
        signature: Some("fn main()".to_string()),
        docstring: None,
    };
    graph.add_node(sym);

    let result = orch.run_task("Change 'Hello' to 'Sage'", &graph);
    // Should either reach Done or Rollback depending on review.
    assert!(
        result == Ok(OrchestratorState::Done) || result == Ok(OrchestratorState::Rollback),
        "Unexpected final state: {:?}",
        result
    );
}

#[test]
fn orchestrator_checkpoint_and_rollback() {
    let mut orch = Orchestrator::new();
    orch.ingest_file("demo.rs", "fn main() {}");

    let cp_id = orch.create_checkpoint("snap").unwrap();
    assert_eq!(cp_id, "snap");

    // Mutate file.
    orch.file_contents.insert("demo.rs".to_string(), "fn changed() {}".to_string());

    // Rollback.
    orch.rollback_to(&cp_id).unwrap();
    assert_eq!(orch.file_contents.get("demo.rs"), Some(&"fn main() {}".to_string()));
    assert_eq!(orch.state, OrchestratorState::Rollback);
}

#[test]
fn orchestrator_ingest_and_blob_store() {
    let mut orch = Orchestrator::new();
    orch.ingest_file("test.rs", "let x = 1;");

    assert_eq!(orch.file_contents.get("test.rs"), Some(&"let x = 1;".to_string()));
    // The blob store should contain the content.
    // We can't directly query it, but checkpoint restore proves it works.
    let cp = orch.create_checkpoint("ingest-check").unwrap();
    let restored = orch.checkpoints.restore_contents(&cp, &orch.blob_store).unwrap();
    assert_eq!(restored.get("test.rs"), Some(&"let x = 1;".to_string()));
}

#[test]
fn orchestrator_task_summary() {
    let mut orch = Orchestrator::new();
    orch.ingest_file("main.rs", "fn main() {}");

    let mut graph = CodeGraph::new();
    let sym = sage::ast::Symbol {
        name: "main".to_string(),
        kind: sage::ast::SymbolKind::Function,
        file_path: "main.rs".to_string(),
        start_line: 1,
        end_line: 1,
        signature: Some("fn main()".to_string()),
        docstring: None,
    };
    graph.add_node(sym);

    let _ = orch.run_task("Change greeting", &graph);
    let summary = orch.task_summary();
    assert!(summary.contains("State:"));
    assert!(summary.contains("Tokens consumed:"));
}

// ---------------------------------------------------------------------------
// Agent trait tests
// ---------------------------------------------------------------------------

#[test]
fn agent_names() {
    use sage::agent::editor::EditorAgent;
    assert_eq!(PlannerAgent::new().name(), "PlannerAgent");
    assert_eq!(EditorAgent::new().name(), "EditorAgent");
    assert_eq!(ExecutorAgent::new().name(), "ExecutorAgent");
    assert_eq!(ReviewerAgent::new().name(), "ReviewerAgent");
}
