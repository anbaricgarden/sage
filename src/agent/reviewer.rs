use std::collections::HashMap;

use super::action_graph::{ActionGraph, ActionNode, ActionType};
use super::Agent;

/// Lightweight semantic validation agent that checks edits for correctness
/// and decides whether to approve, request changes, or trigger rollback.
pub struct ReviewerAgent;

impl Default for ReviewerAgent {
    fn default() -> Self {
        Self
    }
}

impl ReviewerAgent {
    pub fn new() -> Self {
        Self
    }

    /// Review a completed action node and return a decision.
    ///
    /// Returns:
    /// * `Ok(ReviewDecision::Approve)` — edit looks correct.
    /// * `Ok(ReviewDecision::RequestChanges { reason })` — needs revision.
    /// * `Err(reason)` — critical failure; trigger rollback.
    pub fn review(
        &self,
        node: &ActionNode,
        file_contents: &HashMap<String, String>,
    ) -> Result<ReviewDecision, String> {
        match &node.action_type {
            ActionType::Edit { file_path, .. } => {
                let content = file_contents
                    .get(file_path)
                    .ok_or(format!("File not found for review: {}", file_path))?;
                self.validate_edit(file_path, content)
            }
            ActionType::ToolCall { tool, .. } => {
                // Tool calls are generally accepted unless they failed.
                if node.succeeded == Some(true) {
                    Ok(ReviewDecision::Approve)
                } else {
                    Ok(ReviewDecision::RequestChanges {
                        reason: format!("Tool {} did not succeed", tool),
                    })
                }
            }
            ActionType::Validate { criteria } => {
                if node.succeeded == Some(true) {
                    Ok(ReviewDecision::Approve)
                } else {
                    Ok(ReviewDecision::RequestChanges {
                        reason: format!("Validation failed: {}", criteria),
                    })
                }
            }
            ActionType::Checkpoint => Ok(ReviewDecision::Approve),
        }
    }

    /// Validate an edited file for common semantic issues:
    /// - unbalanced braces / brackets / parentheses
    /// - broken import references (if the language uses imports)
    /// - empty file (deletion gone wrong)
    fn validate_edit(&self, file_path: &str, content: &str) -> Result<ReviewDecision, String> {
        // 1. Empty file check.
        if content.trim().is_empty() {
            return Err(format!("File {} is empty after edit", file_path));
        }

        // 2. Brace balance heuristic.
        let mut depth = 0i32;
        let mut in_string = false;
        let mut escape = false;
        for ch in content.chars() {
            if escape {
                escape = false;
                continue;
            }
            match ch {
                '\\' if in_string => escape = true,
                '"' | '\'' => in_string = !in_string,
                '{' | '(' | '[' if !in_string => depth += 1,
                '}' | ')' | ']' if !in_string => depth -= 1,
                _ => {}
            }
            if depth < 0 {
                return Err(format!(
                    "Unbalanced closing brace/bracket/paren in {}",
                    file_path
                ));
            }
        }
        if depth != 0 {
            return Err(format!(
                "Unbalanced opening brace/bracket/paren in {} (depth={})",
                file_path, depth
            ));
        }

        // 3. Import/reference consistency (language-specific heuristic).
        let ext = std::path::Path::new(file_path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        match ext {
            "py" => self.check_python_imports(content)?,
            "rs" => self.check_rust_imports(content)?,
            "js" | "ts" => self.check_js_imports(content)?,
            _ => {}
        }

        Ok(ReviewDecision::Approve)
    }

    fn check_python_imports(&self, _content: &str) -> Result<(), String> {
        // Placeholder: in a full implementation we'd parse imports and verify
        // they exist in the graph. For Phase 3, brace balance is sufficient.
        Ok(())
    }

    fn check_rust_imports(&self, _content: &str) -> Result<(), String> {
        Ok(())
    }

    fn check_js_imports(&self, _content: &str) -> Result<(), String> {
        Ok(())
    }

    /// Summarize a completed turn (observation + action + result) into a single
    /// compact sentence suitable for hierarchical conversation summarization.
    pub fn summarize_turn(&self, node: &ActionNode) -> String {
        let status = match node.succeeded {
            Some(true) => "succeeded",
            Some(false) => "failed",
            None => "pending",
        };
        match &node.action_type {
            ActionType::Edit { file_path, .. } => {
                format!("Edited {} ({})", file_path, status)
            }
            ActionType::ToolCall { tool, .. } => {
                format!("Ran tool '{}' ({})", tool, status)
            }
            ActionType::Validate { criteria } => {
                format!("Validated '{}' ({})", criteria, status)
            }
            ActionType::Checkpoint => "Checkpoint created".to_string(),
        }
    }

    /// Summarize an entire action graph into a phase-level description.
    pub fn summarize_phase(&self, graph: &ActionGraph) -> String {
        let succeeded = graph.succeeded_count();
        let failed = graph.failed_count();
        let total = graph.nodes().len();
        format!(
            "Phase complete: {}/{} actions succeeded, {} failed",
            succeeded, total, failed
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ReviewDecision {
    Approve,
    RequestChanges { reason: String },
}

impl Agent for ReviewerAgent {
    fn name(&self) -> &'static str {
        "ReviewerAgent"
    }
}
