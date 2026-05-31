use std::collections::HashMap;
use std::fs;

/// A tool that the Executor can dispatch to.
pub trait Tool: Send + Sync {
    fn execute(
        &self,
        arguments: &HashMap<String, String>,
        file_contents: &mut HashMap<String, String>,
    ) -> Result<String, String>;
}

/// Mock tool for tests that returns a canned response.
pub struct MockTool {
    pub canned_response: String,
}

impl Tool for MockTool {
    fn execute(
        &self,
        _arguments: &HashMap<String, String>,
        _file_contents: &mut HashMap<String, String>,
    ) -> Result<String, String> {
        Ok(self.canned_response.clone())
    }
}

pub struct ReadFileTool;

impl Tool for ReadFileTool {
    fn execute(
        &self,
        arguments: &HashMap<String, String>,
        file_contents: &mut HashMap<String, String>,
    ) -> Result<String, String> {
        let path = arguments.get("path").ok_or("Missing 'path' argument")?;
        let content = fs::read_to_string(path)
            .or_else(|_| {
                file_contents.get(path).cloned().ok_or_else(|| {
                    format!("File not found: {}", path)
                })
            })?;
        let lines: Vec<&str> = content.lines().collect();
        let preview: Vec<String> = lines.iter().take(20).map(|s| s.to_string()).collect();
        Ok(format!(
            "{} ({} lines)\n{}",
            path,
            lines.len(),
            preview.join("\n")
        ))
    }
}

pub struct WriteFileTool;

impl Tool for WriteFileTool {
    fn execute(
        &self,
        arguments: &HashMap<String, String>,
        file_contents: &mut HashMap<String, String>,
    ) -> Result<String, String> {
        let path = arguments.get("path").ok_or("Missing 'path' argument")?;
        let content = arguments.get("content").ok_or("Missing 'content' argument")?;
        file_contents.insert(path.to_string(), content.to_string());
        Ok(format!("Wrote {} ({} bytes)", path, content.len()))
    }
}

pub struct ListDirTool;

impl Tool for ListDirTool {
    fn execute(
        &self,
        arguments: &HashMap<String, String>,
        _file_contents: &mut HashMap<String, String>,
    ) -> Result<String, String> {
        let path = arguments.get("path").ok_or("Missing 'path' argument")?;
        let entries = fs::read_dir(path)
            .map_err(|e| format!("Failed to list directory: {}", e))?
            .filter_map(|entry| entry.ok().and_then(|e| e.file_name().into_string().ok()))
            .collect::<Vec<String>>();
        Ok(format!(
            "{} entries in {}: {}",
            entries.len(),
            path,
            entries.join(", ")
        ))
    }
}

pub struct SearchTool;

impl Tool for SearchTool {
    fn execute(
        &self,
        arguments: &HashMap<String, String>,
        file_contents: &mut HashMap<String, String>,
    ) -> Result<String, String> {
        let query = arguments.get("query").ok_or("Missing 'query' argument")?;
        let mut matches = Vec::new();
        for (path, content) in file_contents.iter() {
            for (line_num, line) in content.lines().enumerate() {
                if line.contains(query) {
                    matches.push(format!("{}:{} {}", path, line_num + 1, line.trim()));
                    if matches.len() >= 20 {
                        break;
                    }
                }
            }
            if matches.len() >= 20 {
                break;
            }
        }
        if matches.len() >= 20 {
            matches.push("... (truncated after 20 matches)".to_string());
        }
        Ok(format!(
            "Found {} matches for '{}'\n{}",
            matches.len().saturating_sub(1),
            query,
            matches.join("\n")
        ))
    }
}

pub struct RunTestsTool;

impl Tool for RunTestsTool {
    fn execute(
        &self,
        _arguments: &HashMap<String, String>,
        _file_contents: &mut HashMap<String, String>,
    ) -> Result<String, String> {
        Ok("Tests: pass/fall summary not available in stub".to_string())
    }
}
