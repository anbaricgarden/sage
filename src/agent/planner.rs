use super::action_graph::{ActionGraph, ActionNode, ActionType};
use super::Agent;
use crate::codegraph::graph::CodeGraph;

/// Decomposes a user task into an action graph and allocates token budgets per subtask.
pub struct PlannerAgent {
    /// Default token budget for a task.
    pub default_task_budget: usize,
}

impl Default for PlannerAgent {
    fn default() -> Self {
        Self {
            default_task_budget: 4000,
        }
    }
}

impl PlannerAgent {
    pub fn new() -> Self {
        Self::default()
    }

    /// Decompose `task` into an `ActionGraph` based on repository context.
    ///
    /// * `task` — natural language description of what to do.
    /// * `repo_map` — concise file-level overview of the codebase.
    /// * `graph` — the CodeGraph (used to estimate scope).
    ///
    /// Returns an `ActionGraph` with nodes for edits, validations, and checkpoints.
    pub fn plan(
        &self,
        task: &str,
        repo_map: &str,
        graph: &CodeGraph,
    ) -> Result<ActionGraph, String> {
        let mut action_graph = ActionGraph::new();

        // Phase 0: checkpoint before any changes.
        let checkpoint_id = "checkpoint:pre-edit".to_string();
        action_graph.add_node(
            ActionNode::new(&checkpoint_id, ActionType::Checkpoint)
                .with_budget(0),
        );

        // Naive decomposition: try to identify file-level edits from the task.
        let edits = self.extract_edits(task, repo_map);
        let mut prev_deps: Vec<String> = vec![checkpoint_id];

        let total_budget = self.default_task_budget;
        let edit_budget = (total_budget * 52) / 100; // Editor gets ~52% per spec
        let validate_budget = (total_budget * 8) / 100; // Reviewer gets ~8%
        let per_edit_budget = if edits.is_empty() {
            0
        } else {
            edit_budget / edits.len()
        };

        for (idx, (file_path, description)) in edits.iter().enumerate() {
            let edit_id = format!("edit:{}", idx);
            let validate_id = format!("validate:{}", idx);

            // Edit node depends on the checkpoint (and optionally previous edits).
            action_graph.add_node(
                ActionNode::new(
                    &edit_id,
                    ActionType::Edit {
                        file_path: file_path.clone(),
                        description: description.clone(),
                    },
                )
                .with_dependencies(prev_deps.clone())
                .with_budget(per_edit_budget),
            );

            // Validation node depends on its edit.
            action_graph.add_node(
                ActionNode::new(&validate_id, ActionType::Validate {
                    criteria: "preserve type signatures and imports".to_string(),
                })
                .with_dependencies(vec![edit_id.clone()])
                .with_budget(validate_budget / edits.len().max(1)),
            );

            prev_deps = vec![validate_id];
        }

        // If no edits were extracted, add a single tool-call node for exploration.
        if edits.is_empty() {
            action_graph.add_node(
                ActionNode::new(
                    "explore:0",
                    ActionType::ToolCall {
                        tool: "search".to_string(),
                        arguments: [("query".to_string(), task.to_string())]
                            .into_iter()
                            .collect(),
                    },
                )
                .with_dependencies(prev_deps)
                .with_budget((total_budget * 28) / 100), // Executor budget per spec
            );
        }

        // Compute pageRank for the graph to inform any later ranking.
        let _ranks = graph.page_rank(&[], 0.85, 1e-6, 100);

        Ok(action_graph)
    }

    /// Naive heuristic extraction of (file_path, description) pairs from a task string.
    fn extract_edits(&self, task: &str, repo_map: &str) -> Vec<(String, String)> {
        let mut edits = Vec::new();
        let task_lower = task.to_lowercase();

        // Heuristic 1: look for "in <filename>" or "<filename>" references.
        for line in repo_map.lines() {
            // repo_map lines that don't start with spaces are file paths.
            if line.starts_with("  ") || line.is_empty() {
                continue;
            }
            let file_path = line.trim();
            // Check if the task mentions this file (by name or extension).
            let file_name = std::path::Path::new(file_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(file_path);
            if task_lower.contains(&file_name.to_lowercase()) {
                edits.push((file_path.to_string(), task.to_string()));
            }
        }

        // Heuristic 2: if task mentions "change", "fix", "add", "remove" but no files matched,
        // pick the first file in the repo_map as a fallback.
        if edits.is_empty()
            && let Some(first_file) = repo_map.lines().find(|l| !l.starts_with("  ") && !l.is_empty())
        {
            edits.push((first_file.to_string(), task.to_string()));
        }

        edits
    }
}

impl Agent for PlannerAgent {
    fn name(&self) -> &'static str {
        "PlannerAgent"
    }
}
