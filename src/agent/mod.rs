pub mod action_graph;
pub mod checkpoint;
pub mod client;
pub mod editor;
pub mod executor;
pub mod orchestrator;
pub mod planner;
pub mod prompts;
pub mod reviewer;
pub mod tools;

pub use action_graph::{ActionGraph, ActionNode, ActionType};
pub use checkpoint::{Checkpoint, CheckpointManager, SnapshotStore};
pub use client::{ClientConfig, LlmClient, Message};
pub use executor::ExecutorAgent;
pub use orchestrator::{Orchestrator, OrchestratorState};
pub use planner::PlannerAgent;
pub use prompts::PlannerTemplates;
pub use reviewer::{ReviewDecision, ReviewerAgent};
pub use tools::{ListDirTool, MockTool, ReadFileTool, RunTestsTool, SearchTool, Tool, WriteFileTool};

use std::collections::HashMap;

/// Base trait all agents implement.
pub trait Agent {
    fn name(&self) -> &'static str;
}

/// Role-specific trait for task decomposition and action-graph generation.
pub trait PlannerRole: Agent {
    fn plan(
        &self,
        task: &str,
        repo_map: &str,
        graph: &crate::codegraph::graph::CodeGraph,
    ) -> Result<ActionGraph, String>;
}

/// Role-specific trait for generating hash-anchored diff blocks.
pub trait EditorRole: Agent {
    fn generate_edit(
        &self,
        file_path: &str,
        content: &str,
        task: &str,
    ) -> Result<crate::diff::format::EditBlock, String>;
}

/// Role-specific trait for tool execution, diff application, and batching.
pub trait ExecutorRole: Agent {
    fn execute(
        &self,
        node: &ActionNode,
        file_contents: &mut HashMap<String, String>,
        store: &dyn SnapshotStore,
    ) -> Result<String, String>;

    fn execute_batch(
        &self,
        nodes: &[&ActionNode],
        file_contents: &mut HashMap<String, String>,
        store: &dyn SnapshotStore,
    ) -> HashMap<String, Result<String, String>>;

    fn apply_edit_block(
        &self,
        file_path: &str,
        block: &crate::diff::format::EditBlock,
        file_contents: &mut HashMap<String, String>,
        store: &dyn SnapshotStore,
    ) -> Result<String, String>;

    fn summarize_results(&self, results: &HashMap<String, Result<String, String>>) -> String;
}

/// Role-specific trait for semantic validation and summarization.
pub trait ReviewerRole: Agent {
    fn review(
        &self,
        node: &ActionNode,
        file_contents: &HashMap<String, String>,
    ) -> Result<ReviewDecision, String>;

    fn summarize_turn(&self, node: &ActionNode) -> String;

    fn summarize_phase(&self, graph: &ActionGraph) -> String;
}
