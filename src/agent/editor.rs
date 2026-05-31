use super::Agent;
use crate::diff::format::EditBlock;
use regex::Regex;

pub struct EditorAgent;

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
        }
        None
    }
}

impl Agent for EditorAgent {
    fn name(&self) -> &'static str {
        "EditorAgent"
    }
}
