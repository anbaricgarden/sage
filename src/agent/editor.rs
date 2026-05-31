use super::{Agent, EditorRole};
use crate::diff::format::EditBlock;
use regex::Regex;

pub struct EditorAgent;

impl Default for EditorAgent {
    fn default() -> Self {
        Self
    }
}

impl EditorAgent {
    pub fn new() -> Self {
        Self
    }

    pub fn generate_edit(
        &self,
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
            Regex::new(r#"(?i)change\s+['\"]?(.+?)['\"]?\s+to\s+['\"]?(.+?)['\"]?\s*$"#).ok()?,
            Regex::new(r#"(?i)replace\s+['\"]?(.+?)['\"]?\s+with\s+['\"]?(.+?)['\"]?\s*$"#).ok()?,
        ];
        let task_quote_re = Regex::new(r#"['"]([^'"]+)['"]"#).ok()?;
        let content_quote_re = Regex::new(r#"['"]([^'"]+)['"]"#).ok()?;

        for pat in &patterns {
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

                // Fallback: if old_str is not literally in the file, it may be a description.
                // Look for a quoted string in the task that is not in the content, and replace
                // the first quoted string in the content with it.
                if !content.contains(old_str) {
                    for tcap in task_quote_re.captures_iter(task) {
                        let inner = tcap.get(1)?.as_str();
                        if !content.contains(inner) {
                            let line_idx = match content.lines().position(|line| content_quote_re.is_match(line)) {
                                Some(idx) => idx,
                                None => continue,
                            };
                            let mut block = EditBlock::compute_anchor(file_path, content, line_idx, 3, 3);
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
        let pat = Regex::new(r#"(?i)add\s+(?:a\s+)?(?:second\s+)?.*?['\"]?(.+?)['\"]?\s+after\s+(?:the\s+)?.*?['\"]?(.+?)['\"]?\s*$"#).ok()?;
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

            // Fallback: if after_marker not found, look for a println line.
            if let Some(line_idx) = lines.iter().position(|line| line.contains("println")) {
                let mut block = EditBlock::compute_anchor(file_path, content, line_idx, 3, 3);
                let insert_pos = block.new_lines.iter().position(|l| l.contains("println"))? + 1;
                block.new_lines.insert(insert_pos, format!(r#"    println!("{}");"#, new_content));
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
