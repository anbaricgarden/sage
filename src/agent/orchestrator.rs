use std::collections::HashMap;

use super::action_graph::{ActionGraph, ActionNode, ActionType};
use super::checkpoint::CheckpointManager;
use super::client::LlmClient;
use super::reviewer::ReviewDecision;
use super::{EditorRole, ExecutorRole, PlannerRole, ReviewerRole};
use crate::blob_store::BlobStore;
use crate::codegraph::graph::CodeGraph;
use crate::codegraph::repo_map::RepoMap;

/// States of the orchestrator state machine.
#[derive(Debug, Clone, PartialEq)]
pub enum OrchestratorState {
    Idle,
    Planning,
    Editing,
    Executing,
    Reviewing,
    Done,
    Rollback,
}

/// Central coordinator that dispatches agents, manages checkpoints,
/// routes context, and drives the action graph to completion.
pub struct Orchestrator {
    pub state: OrchestratorState,
    pub planner: Box<dyn PlannerRole>,
    pub editor: Box<dyn EditorRole>,
    pub executor: Box<dyn ExecutorRole>,
    pub reviewer: Box<dyn ReviewerRole>,
    /// Optional LLM client for agent-based generation.
    pub llm_client: Option<LlmClient>,
    pub checkpoints: CheckpointManager,
    pub blob_store: BlobStore,
    /// In-memory working tree: file_path -> content.
    pub file_contents: HashMap<String, String>,
    /// Current action graph being executed.
    pub action_graph: Option<ActionGraph>,
    /// Conversation history (turn summaries).
    pub history: Vec<String>,
    /// Cumulative token ledger per agent.
    pub token_ledger: HashMap<String, usize>,
}

impl Default for Orchestrator {
    fn default() -> Self {
        Self::new()
    }
}

impl Orchestrator {
    pub fn new() -> Self {
        use super::editor::EditorAgent;
        use super::executor::ExecutorAgent;
        use super::planner::PlannerAgent;
        use super::reviewer::ReviewerAgent;
        Self {
            state: OrchestratorState::Idle,
            planner: Box::new(PlannerAgent::new()),
            editor: Box::new(EditorAgent::new()),
            executor: Box::new(ExecutorAgent::new()),
            reviewer: Box::new(ReviewerAgent::new()),
            llm_client: None,
            checkpoints: CheckpointManager::new(),
            blob_store: BlobStore::new(),
            file_contents: HashMap::new(),
            action_graph: None,
            history: Vec::new(),
            token_ledger: HashMap::new(),
        }
    }

    /// Create an Orchestrator with an LLM client wired into all agents.
    pub fn with_llm_client(client: LlmClient) -> Self {
        use super::editor::EditorAgent;
        use super::executor::ExecutorAgent;
        use super::planner::PlannerAgent;
        use super::reviewer::ReviewerAgent;
        Self {
            state: OrchestratorState::Idle,
            planner: Box::new(PlannerAgent::with_llm(client.clone())),
            editor: Box::new(EditorAgent::with_llm(client.clone())),
            executor: Box::new(ExecutorAgent::new()),
            reviewer: Box::new(ReviewerAgent::with_llm(client.clone())),
            llm_client: Some(client),
            checkpoints: CheckpointManager::new(),
            blob_store: BlobStore::new(),
            file_contents: HashMap::new(),
            action_graph: None,
            history: Vec::new(),
            token_ledger: HashMap::new(),
        }
    }

    /// Ingest a file into the working tree and blob store.
    pub fn ingest_file(&mut self, file_path: &str, content: &str) {
        self.blob_store.put(content.as_bytes().to_vec());
        self.file_contents.insert(file_path.to_string(), content.to_string());
    }

    /// Process a user task end-to-end through the multi-agent pipeline.
    ///
    /// 1. **Planning** — Planner decomposes the task into an action graph.
    /// 2. **Editing** — Editor generates diff blocks for each edit node.
    /// 3. **Executing** — Executor applies diffs and runs tool calls.
    /// 4. **Reviewing** — Reviewer validates results and approves or triggers rollback.
    /// 5. **Done / Rollback** — Final state.
    pub fn run_task(
        &mut self,
        task: &str,
        code_graph: &CodeGraph,
    ) -> Result<OrchestratorState, String> {
        // Pre-edit checkpoint.
        self.create_checkpoint("pre-edit")?;

        // --- Planning ---
        self.transition_to(OrchestratorState::Planning)?;
        let repo_map = RepoMap::generate(code_graph, 3, None);
        let mut graph = self.planner.plan(task, &repo_map, code_graph)?;

        // --- Editing ---
        self.transition_to(OrchestratorState::Editing)?;
        for node in graph.nodes_mut().values_mut() {
            if let ActionType::Edit { file_path, description } = &node.action_type {
                let content = self
                    .file_contents
                    .get(file_path)
                    .cloned()
                    .unwrap_or_default();
                match self.editor.generate_edit(file_path, &content, description) {
                    Ok(diff) => {
                        // Store the diff block in the node result for the executor.
                        node.result = Some(format!(
                            "<<<<<<< HEAD:{}\n{}\n=======\n{}\n>>>>>>> {}",
                            diff.old_anchor,
                            diff.old_lines.join("\n"),
                            diff.new_lines.join("\n"),
                            diff.new_anchor
                        ));
                        node.succeeded = Some(true);
                        *self.token_ledger.entry("Editor".to_string()).or_insert(0) +=
                            node.token_budget;
                    }
                    Err(err) => {
                        node.result = Some(err);
                        node.succeeded = Some(false);
                    }
                }
            }
        }

        // --- Executing ---
        self.transition_to(OrchestratorState::Executing)?;
        let batches = graph.topological_batches();
        for batch in batches {
            let batch_nodes: Vec<&ActionNode> = batch
                .iter()
                .filter_map(|id| graph.get_node(id))
                .collect();
            if batch_nodes.is_empty() {
                continue;
            }

            let results = self.executor.execute_batch(
                &batch_nodes,
                &mut self.file_contents,
                &self.blob_store,
            );

            // Apply diffs for edit nodes that succeeded in editing.
            for (id, result) in &results {
                if let Some(node) = graph.get_node_mut(id) {
                    if let ActionType::Edit { file_path, .. } = &node.action_type
                        && node.succeeded == Some(true)
                        && let Some(diff_text) = &node.result
                        && let Ok(blocks) = crate::diff::parser::parse_diff(diff_text, file_path)
                    {
                        for block in blocks {
                            let _ = self.executor.apply_edit_block(
                                file_path,
                                &block,
                                &mut self.file_contents,
                                &self.blob_store,
                            );
                        }
                    }
                    // Mark as succeeded if the executor returned Ok.
                    if result.is_ok() {
                        node.succeeded = Some(true);
                    } else {
                        node.succeeded = Some(false);
                    }
                    *self.token_ledger.entry("Executor".to_string()).or_insert(0) +=
                        node.token_budget;
                }
            }
        }

        // --- Reviewing ---
        self.transition_to(OrchestratorState::Reviewing)?;
        let mut all_approved = true;
        for node in graph.nodes().values() {
            if node.succeeded != Some(true) {
                continue; // Skip nodes that already failed.
            }
            match self.reviewer.review(node, &self.file_contents) {
                Ok(ReviewDecision::Approve) => {
                    self.history.push(self.reviewer.summarize_turn(node));
                }
                Ok(ReviewDecision::RequestChanges { reason }) => {
                    self.history.push(format!(
                        "{}: review requested changes - {}",
                        node.id, reason
                    ));
                    all_approved = false;
                }
                Err(err) => {
                    self.history.push(format!("{}: critical review failure - {}", node.id, err));
                    all_approved = false;
                }
            }
            *self.token_ledger.entry("Reviewer".to_string()).or_insert(0) += node.token_budget;
        }

        self.action_graph = Some(graph);

        if all_approved {
            self.transition_to(OrchestratorState::Done)
        } else {
            self.transition_to(OrchestratorState::Rollback)
        }
    }

    /// Create a checkpoint of the current working tree.
    pub fn create_checkpoint(&mut self, id: &str) -> Result<String, String> {
        let mut file_hashes = HashMap::new();
        for (path, content) in &self.file_contents {
            let hash = self.blob_store.put(content.as_bytes().to_vec());
            file_hashes.insert(path.clone(), hash);
        }
        let parent = self
            .checkpoints
            .get(id)
            .map(|cp| cp.id.clone());
        let key = self.checkpoints.create(id, file_hashes, parent);
        Ok(key)
    }

    /// Rollback the working tree to a previous checkpoint.
    pub fn rollback_to(&mut self, checkpoint_id: &str) -> Result<(), String> {
        let contents = self
            .checkpoints
            .restore_contents(checkpoint_id, &self.blob_store)
            .ok_or_else(|| format!("Checkpoint {} not found", checkpoint_id))?;
        self.file_contents = contents;
        self.transition_to(OrchestratorState::Rollback)?;
        Ok(())
    }

    fn transition_to(&mut self, new_state: OrchestratorState) -> Result<OrchestratorState, String> {
        // Validate allowed transitions.
        let allowed = match &self.state {
            OrchestratorState::Idle => vec![OrchestratorState::Planning, OrchestratorState::Rollback],
            OrchestratorState::Planning => vec![OrchestratorState::Editing],
            OrchestratorState::Editing => vec![OrchestratorState::Executing],
            OrchestratorState::Executing => vec![OrchestratorState::Reviewing],
            OrchestratorState::Reviewing => {
                vec![OrchestratorState::Done, OrchestratorState::Rollback]
            }
            OrchestratorState::Done => vec![OrchestratorState::Idle],
            OrchestratorState::Rollback => vec![OrchestratorState::Idle],
        };

        if !allowed.contains(&new_state) {
            return Err(format!(
                "Invalid state transition: {:?} -> {:?}",
                self.state, new_state
            ));
        }

        self.state = new_state.clone();
        Ok(new_state)
    }

    /// Generate a high-level summary of the current task execution.
    pub fn task_summary(&self) -> String {
        let phase_summary = self
            .action_graph
            .as_ref()
            .map(|g| self.reviewer.summarize_phase(g))
            .unwrap_or_else(|| "No action graph executed".to_string());

        let tokens: usize = self.token_ledger.values().sum();
        format!(
            "State: {:?}\n{}\nTokens consumed: {}\nHistory:\n{}",
            self.state,
            phase_summary,
            tokens,
            self.history.join("\n")
        )
    }
}
