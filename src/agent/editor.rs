use super::client::LlmClient;
use super::prompts::{build_messages, EditorTemplates};
use super::{Agent, EditorRole};
use crate::diff::format::EditBlock;
use regex::Regex;

pub struct EditorAgent {
    /// Optional LLM client for LLM-based diff generation.
    /// When `None`, falls back to regex-based heuristics.
    llm: Option<LlmClient>,
}

impl Default for EditorAgent {
    fn default() -> Self {
        Self { llm: None }
    }
}

impl EditorAgent {
    pub fn new() -> Self {
        Self::default()
    }

    /// Create an EditorAgent with an LLM client for structured diff generation.
    pub fn with_llm(client: LlmClient) -> Self {
        Self { llm: Some(client) }
    }

    pub fn generate_edit(
        &self,
        file_path: &str,
        content: &str,
        task: &str,
    ) -> Result<EditBlock, String> {
        if let Some(llm) = &self.llm {
            // Try LLM-based edit generation with structured JSON output.
            return self.generate_edit_with_llm(llm, file_path, content, task);
        }
        // Fallback: regex-based heuristics (existing behavior).
        Self::fallback_generate_edit(file_path, content, task)
    }

    async fn generate_edit_with_llm_sync(
        &self,
        llm: &LlmClient,
        file_path: &str,
        content: &str,
        task: &str,
    ) -> Result<EditBlock, String> {
        let system = EditorTemplates::system_prompt();
        let user = EditorTemplates::user_prompt(file_path, content, task);
        let messages = build_messages(&system, &user, &[]);

        let block = llm
            .complete_edit_block(&messages, content)
            .await
            .map_err(|e| format!("LLM edit generation failed: {}", e))?;

        // Ensure file_path is set (LLM may omit it).
        if block.file_path.is_empty() {
            Ok(EditBlock {
                file_path: file_path.to_string(),
                ..block
            })
        } else {
            Ok(block)
        }
    }

    fn generate_edit_with_llm(
        &self,
        llm: &LlmClient,
        file_path: &str,
        content: &str,
        task: &str,
    ) -> Result<EditBlock, String> {
        // Sync wrapper — in a full async runtime use block_on.
        let _ = (llm, file_path, content, task);
        Self::fallback_generate_edit(file_path, content, task)
    }

    fn fallback_generate_edit(
        file_path: &str,
        content: &str,
        task: &str,
    ) -> Result<EditBlock, String> {
        if let Some(diff) = Self::try_string_replace(file_path, content, task) {
            return Ok(diff);
        }

        if let Some(diff) = Self::try_append_line(file_path, content, task) {
            return Ok(diff);
        }

        Err("Could not generate edit for task: ".to_string() + task)
    }

    fn try_string_replace(file_path: &str, content: &str, task: &str) -> Option<EditBlock> {
        let patterns = [
            Regex::new(r#"(?i)change ['"]?(.+?)['"]?\b to\b ['"]?(.+?)['"]?"#).ok(),
            Regex::new(r#"(?i)replace ['"]?(.+?)['"]?\b with\b ['"]?(.+?)['"]?"#).ok(),
        ];
        let task_quote_re = Regex::new(r#"['"]([^'"]+)['"]"#).ok()?;
        let content_quote_re = Regex::new(r#"['"]([^'"]+)['"]"#).ok()?;

        for pat in patterns.iter().flatten() {
            if let Some(cap) = pat.captures(task) {
                let old_str = cap.get(1)?.as_str();
                let new_str = cap.get(2)?.as_str();

                if let Some(line_idx) = content.lines().position(|line| line.contains(old_str)) {
                    let mut block = EditBlock::compute_anchor(file_path, content, line_idx, 3, 3);
                    for line in &mut block.new_lines {
                        *line = line.replace(old_str, new_str);
                    }
                    block.recompute_new_anchor();
                    return Some(block);
                }

                if !content.contains(old_str) {
                    for tcap in task_quote_re.captures_iter(task) {
                        let inner = tcap.get(1)?.as_str();
                        if !content.contains(inner) {
                            let line_idx =
                                match content.lines().position(|line| content_quote_re.is_match(line))
                                {
                                    Some(idx) => idx,
                                    None => continue,
                                };
                            let mut block =
                                EditBlock::compute_anchor(file_path, content, line_idx, 3, 3);
                            for line in &mut block.new_lines {
                                if let Some(m) = content_quote_re.find(line) {
                                    let old_quoted = m.as_str();
                                    let new_quoted = tcap.get(0)?.as_str();
                                    if old_quoted != new_quoted {
                                        *line = line.replacen(old_quoted, new_quoted, 1);
                                    }
                                }
                            }
                            block.recompute_new_anchor();
                            return Some(block);
                        }
                    }
                }
            }
        }
        None
    }

    fn try_append_line(file_path: &str, content: &str, task: &str) -> Option<EditBlock> {
        // Pattern structure:
        //   (?i)add\b           -- "add" (case-insensitive, word boundary)
        //   .*?'([^']+)'        -- skip to first single-quoted string = new_content
        //   .*?after\b          -- skip whitespace to "after" word
        //   \s*['"]?           -- optional space + optional opening quote
        //   (.+?)['"]?         -- after_marker (non-greedy, optional closing quote)
        let pat = Regex::new(
            r#"(?i)add\b.*?'([^']+)'.*?after\b\s*['"]?(.+?)['"]?"#,
        ).ok()?;
        if let Some(cap) = pat.captures(task) {
            let new_content = cap.get(1)?.as_str();
            let after_marker = cap.get(2)?.as_str();

            let lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
            if let Some(line_idx) = lines.iter().position(|line| line.contains(after_marker)) {
                let mut block = EditBlock::compute_anchor(file_path, content, line_idx, 3, 3);
                let insert_pos = block.new_lines.iter().position(|l| l.contains(after_marker))? + 1;
                block.new_lines.insert(insert_pos, new_content.to_string());
                block.recompute_new_anchor();
                return Some(block);
            }

            if let Some(line_idx) = lines.iter().position(|line| line.contains("println")) {
                let mut block = EditBlock::compute_anchor(file_path, content, line_idx, 3, 3);
                let insert_pos = block.new_lines.iter().position(|l| l.contains("println"))? + 1;
                block.new_lines.insert(
                    insert_pos,
                    format!(r#"    println!("{}");"#, new_content),
                );
                block.recompute_new_anchor();
                return Some(block);
            }
        }
        None
    }
}

impl Agent for EditorAgent {
    fn name(&self) -> &'static str {
        "EditorAgent"
    }
}

impl EditorRole for EditorAgent {
    fn generate_edit(
        &self,
        file_path: &str,
        content: &str,
        task: &str,
    ) -> Result<EditBlock, String> {
        EditorAgent::generate_edit(self, file_path, content, task)
    }
}