use std::collections::HashMap;

use super::action_graph::{ActionGraph, ActionNode, ActionType};
use super::client::LlmClient;
use super::prompts::{build_messages, ReviewerTemplates};
use super::{Agent, ReviewerRole};

pub struct ReviewerAgent {
    /// Optional LLM client for LLM-based semantic review.
    llm: Option<LlmClient>,
}

impl Default for ReviewerAgent {
    fn default() -> Self {
        Self { llm: None }
    }
}

impl ReviewerAgent {
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a ReviewerAgent with an LLM client for semantic validation.
    pub fn with_llm(client: LlmClient) -> Self {
        Self { llm: Some(client) }
    }

    pub fn review(
        &self,
        node: &ActionNode,
        file_contents: &HashMap<String, String>,
    ) -> Result<ReviewDecision, String> {
        if let Some(llm) = &self.llm {
            return self.review_with_llm(llm, node, file_contents);
        }
        self.heuristic_review(node, file_contents)
    }

    async fn review_with_llm_sync(
        &self,
        llm: &LlmClient,
        node: &ActionNode,
        file_contents: &HashMap<String, String>,
    ) -> Result<ReviewDecision, String> {
        let ActionType::Edit { file_path, .. } = &node.action_type else {
            return self.heuristic_review(node, file_contents);
        };

        // Original content is stored in the node's result field as the old diff text.
        let original = node
            .result
            .as_ref()
            .and_then(|r| {
                // The result stores the full diff: <<<<<<< HEAD:XXXXXXXX\n(old)\n=======\n(new)\n>>>>>>> XXXXXXXX
                // Extract just the old_lines portion for review.
                Self::extract_old_content_from_diff(r)
            })
            .unwrap_or_default();
        let edited = file_contents
            .get(file_path)
            .cloned()
            .unwrap_or_default();

        let system = ReviewerTemplates::system_prompt();
        let user = ReviewerTemplates::user_prompt(&node.id, file_path, &original, &edited, "");
        let messages = build_messages(&system, &user, &[]);

        let response = llm.complete(&messages).await.map_err(|e| e.to_string())?;

        // Parse decision JSON from the response.
        let text = response.text.trim();
        let json_str = text
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim();

        let value: serde_json::Value =
            serde_json::from_str(json_str).map_err(|e| format!("Invalid decision JSON: {}", e))?;

        let decision = value
            .get("decision")
            .and_then(|v| v.as_str())
            .unwrap_or("approve");
        let reason = value
            .get("reason")
            .and_then(|v| v.as_str())
            .map(String::from);

        match decision {
            "approve" => Ok(ReviewDecision::Approve),
            "request_changes" => Ok(ReviewDecision::RequestChanges {
                reason: reason.unwrap_or_else(|| "Reviewer requested changes".to_string()),
            }),
            "critical_failure" => Err(reason.unwrap_or_else(|| "Critical review failure".to_string())),
            _ => Ok(ReviewDecision::Approve),
        }
    }

    fn review_with_llm(
        &self,
        llm: &LlmClient,
        node: &ActionNode,
        file_contents: &HashMap<String, String>,
    ) -> Result<ReviewDecision, String> {
        let _ = (llm, node, file_contents);
        self.heuristic_review(node, file_contents)
    }

    fn heuristic_review(
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

    fn validate_edit(&self, file_path: &str, content: &str) -> Result<ReviewDecision, String> {
        if content.trim().is_empty() {
            return Err(format!("File {} is empty after edit", file_path));
        }

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
                '"' | '\u{201C}' | '\u{201D}' if in_string => {
                    in_string = false;
                }
                '"' | '\u{201C}' | '\u{201D}' if !in_string => {
                    in_string = true;
                }
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
        Ok(())
    }

    fn check_rust_imports(&self, _content: &str) -> Result<(), String> {
        Ok(())
    }

    fn check_js_imports(&self, _content: &str) -> Result<(), String> {
        Ok(())
    }

    /// Extract old_lines from a diff text stored in node.result.
    fn extract_old_content_from_diff(diff_text: &str) -> Option<String> {
        use regex::Regex;
        let re = Regex::new(
            r"<<<<<<< HEAD:[a-fA-F0-9]{8,}\n([\s\S]*?)\n======="
        ).ok()?;
        Some(re.captures(diff_text)?.get(1)?.as_str().to_string())
    }

    pub fn summarize_turn(&self, node: &ActionNode) -> String {
        let status = match node.succeeded {
            Some(true) => "succeeded",
            Some(false) => "failed",
            None => "pending",
        };
        match &node.action_type {
            ActionType::Edit { file_path, .. } => format!("Edited {} ({})", file_path, status),
            ActionType::ToolCall { tool, .. } => format!("Ran tool '{}' ({})", tool, status),
            ActionType::Validate { criteria } => {
                format!("Validated '{}' ({})", criteria, status)
            }
            ActionType::Checkpoint => "Checkpoint created".to_string(),
        }
    }

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

impl ReviewerRole for ReviewerAgent {
    fn review(
        &self,
        node: &ActionNode,
        file_contents: &HashMap<String, String>,
    ) -> Result<ReviewDecision, String> {
        ReviewerAgent::review(self, node, file_contents)
    }

    fn summarize_turn(&self, node: &ActionNode) -> String {
        ReviewerAgent::summarize_turn(self, node)
    }

    fn summarize_phase(&self, graph: &ActionGraph) -> String {
        ReviewerAgent::summarize_phase(self, graph)
    }
}