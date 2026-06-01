//! # Conversation Summarization
//!
//! Hierarchical summarization of multi-agent conversation history to keep
//! prompt size bounded as tasks progress through many turns.
//!
//! ## Levels
//!
//! 1. **Turn-level** — Each completed turn (observation + action + result) → ~50 tokens
//! 2. **Phase-level** — Every 5 turns → ~1 sentence summarizing what happened
//! 3. **Task-level** — Running summary updated after each phase completion
//!
//! The full history is **only** loaded on explicit user request ("why did you make that choice?").
//! In normal operation, the prompt receives: current task summary + last 3 turns + full context.

use serde::{Deserialize, Serialize};

/// A compressed representation of a single turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnSummary {
    /// Which agent produced this turn.
    pub agent: String,
    /// Concise description of the action taken.
    pub action: String,
    /// Whether the action succeeded.
    pub succeeded: bool,
    /// Key tokens consumed by this turn (approximate).
    pub tokens: usize,
}

impl TurnSummary {
    /// Format this turn as a single compact sentence (~50 tokens).
    pub fn to_sentence(&self) -> String {
        let status = if self.succeeded { "✓" } else { "✗" };
        format!("[{}] {} ({}{})", self.agent, self.action, status, self.tokens)
    }
}

/// Summarizes conversation history across all levels.
#[derive(Debug, Clone)]
pub struct ConversationSummarizer {
    /// Max recent turns to keep verbatim in context.
    recent_turns_limit: usize,
}

impl Default for ConversationSummarizer {
    fn default() -> Self {
        Self::new()
    }
}

impl ConversationSummarizer {
    pub fn new() -> Self {
        Self {
            recent_turns_limit: 3,
        }
    }

    /// Summarize a full conversation history into a compact string.
    /// Keeps the most recent `recent_turns_limit` turns verbatim, summarizes the rest.
    pub fn summarize_conversation(&self, turns: &[TurnSummary]) -> String {
        if turns.is_empty() {
            return "No actions taken yet.".to_string();
        }

        let recent = turns.len().saturating_sub(self.recent_turns_limit);
        let recent_slice = &turns[turns.len().saturating_sub(self.recent_turns_limit)..];

        let mut parts = Vec::new();

        // Summarized older turns (phase-level)
        if recent > 0 {
            let older = &turns[..turns.len().saturating_sub(self.recent_turns_limit)];
            let summarized = self.summarize_phase(older);
            if !summarized.is_empty() {
                parts.push(format!("Earlier ({} turns): {}", recent, summarized));
            }
        }

        // Recent turns verbatim
        if !recent_slice.is_empty() {
            parts.push("Recent turns:".to_string());
            for turn in recent_slice {
                parts.push(format!("  • {}", turn.to_sentence()));
            }
        }

        // Task-level summary at the top
        let task_summary = self.task_level_summary(turns);
        if !task_summary.is_empty() {
            parts.insert(0, format!("Task progress: {}\n", task_summary));
        }

        parts.join("\n")
    }

    /// Summarize a batch of turns into a single phase-level sentence.
    /// Groups by outcome (all succeeded, mixed, all failed) and action type.
    pub fn summarize_phase(&self, turns: &[TurnSummary]) -> String {
        if turns.is_empty() {
            return String::new();
        }

        let succeeded = turns.iter().filter(|t| t.succeeded).count();
        let total = turns.len();

        let actions: std::collections::HashSet<_> = turns.iter().map(|t| t.action.clone()).collect();

        let outcome = if succeeded == total {
            "all succeeded"
        } else if succeeded == 0 {
            "all failed"
        } else {
            "partially succeeded"
        };

        let action_summary = if actions.len() == 1 {
            format!("{} {}", outcome, actions.iter().next().unwrap())
        } else {
            format!(
                "{} ({} different action types)",
                outcome,
                actions.len()
            )
        };

        format!(
            "{} in {} turn{} — {} total tokens",
            action_summary,
            total,
            if total == 1 { "" } else { "s" },
            turns.iter().map(|t| t.tokens).sum::<usize>()
        )
    }

    /// Generate a one-line task-level summary describing what's been done so far.
    pub fn task_level_summary(&self, turns: &[TurnSummary]) -> String {
        let succeeded = turns.iter().filter(|t| t.succeeded).count();
        let total = turns.len();
        let total_tokens: usize = turns.iter().map(|t| t.tokens).sum();

        // Group by agent
        let mut agent_counts = std::collections::HashMap::new();
        for turn in turns {
            *agent_counts.entry(&turn.agent).or_insert(0) += 1;
        }

        let agent_summary: String = agent_counts
            .iter()
            .map(|(k, v)| format!("{}:{}", k, v))
            .collect::<Vec<_>>()
            .join(", ");

        format!(
            "{}→{}/{} turns succeeded, {} tokens (agents: {})",
            succeeded,
            total,
            total,
            total_tokens,
            agent_summary
        )
    }

    /// Summarize a raw tool result for context injection.
    /// Returns a compact representation appropriate for the tool type.
    pub fn summarize_tool_result(&self, tool_name: &str, raw_output: &str) -> String {
        match tool_name {
            "read_file" => Self::summarize_file_read(raw_output),
            "grep" | "search" => Self::summarize_grep(raw_output),
            "run_tests" => Self::summarize_test_output(raw_output),
            "list_dir" => Self::summarize_list_dir(raw_output),
            "git_diff" => Self::summarize_git_diff(raw_output),
            "linter" => Self::summarize_linter_output(raw_output),
            _ => Self::summarize_generic(raw_output),
        }
    }

    fn summarize_file_read(output: &str) -> String {
        let lines: Vec<_> = output.lines().collect();
        if lines.len() <= 30 {
            return output.to_string();
        }
        // Return first and last 5 lines + total line count
        let total = lines.len();
        let head = lines.iter().take(5).copied().collect::<Vec<_>>().join("\n");
        let tail = lines.iter().rev().take(5).copied().collect::<Vec<_>>().join("\n");
        format!("{}...\n[{} more lines, last 5:]\n{}", head, total, tail)
    }

    fn summarize_grep(output: &str) -> String {
        let matches: Vec<_> = output.lines().filter(|l| !l.is_empty()).collect();
        let count = matches.len();
        if count == 0 {
            return "No matches found.".to_string();
        }

        // Group by file (store owned Strings to avoid borrow lifetimes)
        let mut by_file: std::collections::HashMap<&str, Vec<String>> = std::collections::HashMap::new();
        for line in &matches {
            let mut parts = line.splitn(3, ':');
            let file = parts.next().unwrap_or("<unknown>");
            let line_part = parts.next().unwrap_or("");
            let content = parts.next().unwrap_or("");
            let rest = format!("{}:{}", line_part, content); // "10: let x = 1;"
            by_file.entry(file).or_default().push(rest);
        }

        // Pick the file with the most matches as the example (stable across runs)
        let mut example_file = "<unknown>";
        let mut example_match = "";
        let mut max_count = 0;
        for (file, matches) in &by_file {
            if matches.len() > max_count {
                max_count = matches.len();
                example_file = file;
                example_match = matches.first().map(|s| s.as_str()).unwrap_or("");
            }
        }

        if by_file.len() == 1 {
            format!("{} matches in 1 file: {}:{}", count, example_file, example_match)
        } else {
            format!("{} matches in {} file(s): {}:{}", count, by_file.len(), example_file, example_match)
        }
    }

    fn summarize_test_output(output: &str) -> String {
        // Look for summary patterns like "X passed" or "X failed"
        let lines: Vec<_> = output.lines().collect();

        // Find summary line
        let summary = lines.iter()
            .find(|l| l.contains("passed") || l.contains("failed") || l.contains("error"))
            .map(|l| l.trim())
            .unwrap_or("Test run complete");

        let mut result = summary.to_string();

        // Add first few failure traces if present
        let failures: Vec<_> = lines.iter()
            .filter(|l| l.contains("FAILED") || l.contains("AssertionError") || l.contains("Error:"))
            .take(5)
            .map(|l| l.trim())
            .collect();

        if !failures.is_empty() {
            result.push_str("\nFailures:");
            for f in failures {
                result.push_str(&format!("\n  • {}", f));
            }
        }

        result
    }

    fn summarize_list_dir(output: &str) -> String {
        let lines: Vec<_> = output.lines().filter(|l| !l.is_empty()).collect();
        if lines.len() <= 20 {
            return output.to_string();
        }
        format!("{} entries (showing first 20):\n{}\n...",
            lines.len(),
            lines.iter().take(20).copied().collect::<Vec<_>>().join("\n"))
    }

    fn summarize_git_diff(output: &str) -> String {
        let lines: Vec<_> = output.lines().collect();
        let added = lines.iter().filter(|l| l.starts_with("+")).count();
        let removed = lines.iter().filter(|l| l.starts_with("-")).count();
        if lines.is_empty() {
            return "No changes.".to_string();
        }
        if lines.len() <= 10 {
            return output.to_string();
        }
        format!(
            "±{} lines added, ∓{} removed ({} total diff lines)\n{}",
            added, removed, lines.len(),
            lines.iter().take(5).copied().collect::<Vec<_>>().join("\n")
        )
    }

    fn summarize_linter_output(output: &str) -> String {
        let lines: Vec<_> = output.lines().collect();
        let errors: Vec<_> = lines.iter()
            .filter(|l| l.contains("error") || l.contains("Error") || l.contains("E:"))
            .take(20)
            .map(|l| l.trim())
            .collect();

        if errors.is_empty() {
            return "No errors found.".to_string();
        }
        let mut result = format!("{} error(s):\n", errors.len());
        for e in errors {
            result.push_str(&format!("  • {}\n", e));
        }
        result
    }

    fn summarize_generic(output: &str) -> String {
        let trimmed = output.trim();
        if trimmed.len() <= 200 {
            return trimmed.to_string();
        }
        format!("{}...[{} chars total]", &trimmed[..200], trimmed.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_turn(agent: &str, action: &str, succeeded: bool, tokens: usize) -> TurnSummary {
        TurnSummary {
            agent: agent.to_string(),
            action: action.to_string(),
            succeeded,
            tokens,
        }
    }

    #[test]
    fn test_turn_summary_sentence() {
        let t = make_turn("Editor", "Generated diff for foo.py", true, 320);
        let s = t.to_sentence();
        assert!(s.contains("Editor"));
        assert!(s.contains("foo.py"));
        assert!(s.contains("✓"));
    }

    #[test]
    fn test_summarize_phase_all_success() {
        let sum = ConversationSummarizer::new();
        let turns = vec![
            make_turn("Editor", "edit foo.py", true, 100),
            make_turn("Editor", "edit bar.py", true, 100),
        ];
        let out = sum.summarize_phase(&turns);
        assert!(out.contains("all succeeded"));
        assert!(out.contains("2 turns"));
    }

    #[test]
    fn test_summarize_phase_mixed() {
        let sum = ConversationSummarizer::new();
        let turns = vec![
            make_turn("Editor", "edit foo.py", true, 100),
            make_turn("Editor", "edit bar.py", false, 80),
        ];
        let out = sum.summarize_phase(&turns);
        assert!(out.contains("partially succeeded"));
    }

    #[test]
    fn test_task_level_summary() {
        let sum = ConversationSummarizer::new();
        let turns = vec![
            make_turn("Planner", "planned 3 edits", true, 200),
            make_turn("Editor", "edit foo.py", true, 400),
            make_turn("Reviewer", "approved foo.py", true, 100),
        ];
        let out = sum.task_level_summary(&turns);
        assert!(out.contains("3→3/3"));
        assert!(out.contains("700"));
    }

    #[test]
    fn test_summarize_conversation_recent_turns() {
        let sum = ConversationSummarizer::new();
        let turns = vec![
            make_turn("Planner", "planned edits", true, 200),
            make_turn("Editor", "edited foo.py", true, 400),
            make_turn("Editor", "edited bar.py", true, 350),
            make_turn("Reviewer", "approved all", true, 100),
        ];
        let out = sum.summarize_conversation(&turns);
        // Recent 3 turns should be verbatim
        assert!(out.contains("edited bar.py"));
        assert!(out.contains("approved all"));
        // Older turn summarized
        assert!(out.contains("Earlier"));
    }

    #[test]
    fn test_summarize_tool_result_grep() {
        let sum = ConversationSummarizer::new();
        let output = "src/foo.rs:10: let x = 1;\nsrc/foo.rs:15: let y = 2;\nsrc/bar.rs:5: let z = 3;\n";
        let out = sum.summarize_tool_result("grep", output);
        assert!(out.contains("3 matches"));
        assert!(out.contains("src/foo.rs"));
    }

    #[test]
    fn test_summarize_tool_result_read_file() {
        let sum = ConversationSummarizer::new();
        let lines: Vec<_> = (0..100).map(|i| format!("line {}", i)).collect();
        let output = lines.join("\n");
        let out = sum.summarize_tool_result("read_file", &output);
        assert!(out.contains("100"));
        assert!(out.contains("more lines"));
    }

    #[test]
    fn test_summarize_tool_result_tests_pass() {
        let sum = ConversationSummarizer::new();
        let output = "test_foo PASSED\ntest_bar PASSED\ntest_baz PASSED\n3 passed, 0 failed";
        let out = sum.summarize_tool_result("run_tests", output);
        assert!(out.contains("passed"));
    }

    #[test]
    fn test_summarize_tool_result_tests_fail() {
        let sum = ConversationSummarizer::new();
        let output = "test_foo PASSED\ntest_bar FAILED\n  AssertionError: expected 1, got 2\ntest_baz PASSED\n2 passed, 1 failed";
        let out = sum.summarize_tool_result("run_tests", output);
        assert!(out.contains("1 failed"));
        assert!(out.contains("AssertionError"));
    }

    #[test]
    fn test_summarize_git_diff() {
        let sum = ConversationSummarizer::new();
        let output = "+ line added\n- line removed\n  context\n+ another added";
        let out = sum.summarize_tool_result("git_diff", output);
        assert!(out.contains("added"));
        assert!(out.contains("removed"));
    }

    #[test]
    fn test_recent_turns_limit() {
        let sum = ConversationSummarizer::new();
        // Only 2 turns, both should be verbatim
        let turns = vec![
            make_turn("Editor", "edit A", true, 100),
            make_turn("Editor", "edit B", true, 100),
        ];
        let out = sum.summarize_conversation(&turns);
        assert!(out.contains("edit A"));
        assert!(out.contains("edit B"));
        assert!(!out.contains("Earlier"));
    }
}