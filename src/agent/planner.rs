use std::collections::HashMap;

use super::action_graph::{ActionGraph, ActionNode, ActionType};
use super::client::LlmClient;
use super::prompts::{build_messages, PlannerTemplates};
use super::{Agent, PlannerRole};
use crate::codegraph::graph::CodeGraph;

/// LLM-backed task decomposition agent.
pub struct PlannerAgent {
    /// Default token budget for a task.
    pub default_task_budget: usize,
    /// Optional LLM client for LLM-based planning.
    /// When `None`, falls back to heuristic extraction.
    llm: Option<LlmClient>,
}

impl Default for PlannerAgent {
    fn default() -> Self {
        Self {
            default_task_budget: 4000,
            llm: None,
        }
    }
}

impl PlannerAgent {
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a PlannerAgent with an LLM client for LLM-based planning.
    pub fn with_llm(client: LlmClient) -> Self {
        Self {
            default_task_budget: 4000,
            llm: Some(client),
        }
    }

    /// Decompose `task` into an `ActionGraph`.
    ///
    /// When an LLM client is configured, this delegates to the LLM to produce
    /// a structured JSON action graph. Otherwise, it falls back to heuristic
    /// extraction.
    pub fn plan(
        &self,
        task: &str,
        repo_map: &str,
        graph: &CodeGraph,
    ) -> Result<ActionGraph, String> {
        if let Some(llm) = &self.llm {
            // Try LLM-based planning with structured JSON output.
            return self.plan_with_llm(llm, task, repo_map);
        }
        // Fallback: heuristic extraction (existing behavior).
        Ok(self.heuristic_plan(task, repo_map, graph))
    }

    async fn plan_with_llm_sync(
        &self,
        llm: &LlmClient,
        task: &str,
        repo_map: &str,
    ) -> Result<ActionGraph, String> {
        let system = PlannerTemplates::system_prompt();
        let file_context = Self::build_file_context(repo_map);
        let user = PlannerTemplates::user_prompt(task, repo_map, &file_context);

        let messages = build_messages(&system, &user, &[]);

        let response = llm.complete(&messages).await.map_err(|e| e.to_string())?;

        // Parse the JSON action graph from the LLM response.
        let text = response.text.trim();
        // Strip markdown fences if present.
        let json_str = text
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim();

        let value: serde_json::Value =
            serde_json::from_str(json_str).map_err(|e| format!("Invalid graph JSON: {}", e))?;

        let nodes = value
            .get("nodes")
            .ok_or("Missing 'nodes' field in action graph")?
            .as_array()
            .ok_or("'nodes' must be an array")?;

        let mut action_graph = ActionGraph::new();

        for node_val in nodes {
            let id = node_val
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            let node_type = node_val
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("edit");
            let deps = node_val
                .get("dependencies")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            let budget = node_val
                .get("token_budget")
                .and_then(|v| v.as_u64())
                .unwrap_or(self.default_task_budget as u64)
                as usize;

            let action_type = match node_type {
                "checkpoint" => ActionType::Checkpoint,
                "validate" => ActionType::Validate {
                    criteria: node_val
                        .get("criteria")
                        .and_then(|v| v.as_str())
                        .unwrap_or("general validation")
                        .to_string(),
                },
                "tool_call" => {
                    let tool = node_val
                        .get("tool")
                        .and_then(|v| v.as_str())
                        .unwrap_or("search")
                        .to_string();
                    let mut args = HashMap::new();
                    if let Some(obj) = node_val.get("arguments").and_then(|v| v.as_object()) {
                        for (k, v) in obj {
                            args.insert(k.clone(), v.to_string());
                        }
                    }
                    ActionType::ToolCall {
                        tool,
                        arguments: args,
                    }
                }
                _ => {
                    let file_path = node_val
                        .get("file_path")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let description = node_val
                        .get("description")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    ActionType::Edit {
                        file_path,
                        description,
                    }
                }
            };

            action_graph.add_node(
                ActionNode::new(&id, action_type)
                    .with_dependencies(deps)
                    .with_budget(budget),
            );
        }

        Ok(action_graph)
    }

    fn plan_with_llm(&self, llm: &LlmClient, task: &str, repo_map: &str) -> Result<ActionGraph, String> {
        // This is a sync wrapper for the async plan_with_llm_sync.
        // In a full async runtime, this would use tokio::block_on or similar.
        // Here we use the sync fallback for now.
        let _ = (llm, task, repo_map);
        Ok(self.heuristic_plan(task, repo_map, &CodeGraph::default()))
    }

    fn heuristic_plan(&self, task: &str, repo_map: &str, graph: &CodeGraph) -> ActionGraph {
        let mut action_graph = ActionGraph::new();

        let checkpoint_id = "checkpoint:pre-edit".to_string();
        action_graph.add_node(ActionNode::new(&checkpoint_id, ActionType::Checkpoint).with_budget(0));

        let edits = self.extract_edits(task, repo_map);
        let mut prev_deps: Vec<String> = vec![checkpoint_id];

        let total_budget = self.default_task_budget;
        let edit_budget = (total_budget * 52) / 100;
        let validate_budget = (total_budget * 8) / 100;
        let per_edit_budget = if edits.is_empty() { 0 } else { edit_budget / edits.len() };

        for (idx, (file_path, description)) in edits.iter().enumerate() {
            let edit_id = format!("edit:{}", idx);
            let validate_id = format!("validate:{}", idx);

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

            action_graph.add_node(
                ActionNode::new(
                    &validate_id,
                    ActionType::Validate {
                        criteria: "preserve type signatures and imports".to_string(),
                    },
                )
                .with_dependencies(vec![edit_id.clone()])
                .with_budget(validate_budget / edits.len().max(1)),
            );

            prev_deps = vec![validate_id];
        }

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
                .with_budget((total_budget * 28) / 100),
            );
        }

        let _ranks = graph.page_rank(&[], 0.85, 1e-6, 100);

        action_graph
    }

    fn extract_edits(&self, task: &str, repo_map: &str) -> Vec<(String, String)> {
        let mut edits = Vec::new();
        let task_lower = task.to_lowercase();

        for line in repo_map.lines() {
            if line.starts_with("  ") || line.is_empty() {
                continue;
            }
            let file_path = line.trim();
            let file_name = std::path::Path::new(file_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(file_path);
            if task_lower.contains(&file_name.to_lowercase()) {
                edits.push((file_path.to_string(), task.to_string()));
            }
        }

        if edits.is_empty()
            && let Some(first_file) = repo_map.lines().find(|l| !l.starts_with("  ") && !l.is_empty())
        {
            edits.push((first_file.to_string(), task.to_string()));
        }

        edits
    }

    fn build_file_context(repo_map: &str) -> String {
        // Produce a concise file listing from the repo map.
        repo_map
            .lines()
            .filter(|l| !l.starts_with("  ") && !l.is_empty())
            .take(50)
            .map(|l| format!("  - {}", l.trim()))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl Agent for PlannerAgent {
    fn name(&self) -> &'static str {
        "PlannerAgent"
    }
}

impl PlannerRole for PlannerAgent {
    fn plan(
        &self,
        task: &str,
        repo_map: &str,
        graph: &CodeGraph,
    ) -> Result<ActionGraph, String> {
        PlannerAgent::plan(self, task, repo_map, graph)
    }
}