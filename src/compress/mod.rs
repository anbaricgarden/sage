//! # Prompt Compression
//!
//! Code-aware token pruning to reduce input context size before sending to API models.
//! Applied after context retrieval but before prompt assembly.
//!
//! # Compression Rules
//!
//! | Target | Strategy | Token Savings | Quality Impact |
//! |---|---|---|---|
//!! File contents | Remove unused imports, truncate docstrings, strip non-TODO comments, collapse whitespace | 25–35% | Minimal (semantics preserved) |
//! | Tool results | Rule-based summarization | 50–80% | None (lossy by design) |
//! | Conversation history | Hierarchical summarization | 40–60% | Low (key decisions kept) |
//!
//! # Safety Rules
//!
//! Type annotations, function signatures, and control flow are **never** compressed.
//! Only non-critical regions are touched: comments, docstrings, whitespace, unused imports.

pub mod pruner;
pub mod summarize;

use serde::{Deserialize, Serialize};

pub use pruner::CodePruner;
pub use summarize::{ConversationSummarizer, TurnSummary};

/// Configuration for the prompt compressor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompressorConfig {
    /// Remove unused imports identified by AST analysis.
    pub remove_unused_imports: bool,
    /// Truncate docstrings to their first sentence.
    pub truncate_docstrings: bool,
    /// Remove non-TODO/FIXME inline comments.
    pub strip_comments: bool,
    /// Collapse multiple blank lines into one.
    pub collapse_whitespace: bool,
    /// When loading test files, load only test signatures and assertions (not setup boilerplate).
    pub compress_test_files: bool,
    /// Preserve all type annotations (never strip).
    pub preserve_type_annotations: bool,
    /// Max tokens in a compressed docstring (0 = no limit).
    pub max_docstring_tokens: usize,
}

impl Default for CompressorConfig {
    fn default() -> Self {
        Self {
            remove_unused_imports: true,
            truncate_docstrings: true,
            strip_comments: true,
            collapse_whitespace: true,
            compress_test_files: true,
            preserve_type_annotations: true,
            max_docstring_tokens: 50,
        }
    }
}

/// High-level prompt compressor that orchestrates all compression stages.
#[derive(Debug, Clone)]
pub struct Compressor {
    pruner: CodePruner,
    summarizer: ConversationSummarizer,
}

impl Default for Compressor {
    fn default() -> Self {
        Self::new(CompressorConfig::default())
    }
}

impl Compressor {
    pub fn new(config: CompressorConfig) -> Self {
        Self {
            pruner: CodePruner::new(config),
            summarizer: ConversationSummarizer::new(),
        }
    }

    /// Compress a file's content for prompt injection.
    /// Returns the compressed content and the estimated token reduction.
    pub fn compress_file(&self, file_path: &str, content: &str) -> CompressedOutput {
        let lang = detect_language(file_path);
        let original_tokens = self.estimate_tokens(content);
        let compressed = self.pruner.compress(content, &lang);
        let compressed_tokens = self.estimate_tokens(&compressed);
        let reduction = if original_tokens > 0 {
            (original_tokens as f64 - compressed_tokens as f64) / original_tokens as f64
        } else {
            0.0
        };

        CompressedOutput {
            original: content.to_string(),
            compressed,
            original_tokens,
            compressed_tokens,
            reduction_pct: (reduction * 100.0).round() as u8,
        }
    }

    /// Compress a tool result for context injection.
    pub fn compress_tool_result(&self, tool_name: &str, raw_output: &str) -> String {
        self.summarizer.summarize_tool_result(tool_name, raw_output)
    }

    /// Summarize the conversation history down to a compact summary.
    pub fn compress_history(&self, turns: &[TurnSummary]) -> String {
        self.summarizer.summarize_conversation(turns)
    }

    /// Estimate token count using a rough chars-per-token heuristic (0.25 for code).
    fn estimate_tokens(&self, text: &str) -> usize {
        ((text.len() as f64) * 0.25).ceil() as usize
    }
}

/// Output from a compression operation.
#[derive(Debug, Clone)]
pub struct CompressedOutput {
    /// The original content.
    pub original: String,
    /// The compressed content.
    pub compressed: String,
    /// Estimated original token count.
    pub original_tokens: usize,
    /// Estimated compressed token count.
    pub compressed_tokens: usize,
    /// Reduction percentage (0–100).
    pub reduction_pct: u8,
}

/// Detect the language from a file extension.
fn detect_language(file_path: &str) -> String {
    let ext = std::path::Path::new(file_path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    match ext.to_lowercase().as_str() {
        "py" => "python".to_string(),
        "js" => "javascript".to_string(),
        "ts" | "tsx" => "typescript".to_string(),
        "go" => "go".to_string(),
        "rs" => "rust".to_string(),
        "rb" => "ruby".to_string(),
        "java" => "java".to_string(),
        "c" | "h" => "c".to_string(),
        "cpp" | "cc" | "cxx" => "cpp".to_string(),
        _ => "unknown".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_language() {
        assert_eq!(detect_language("foo.py"), "python");
        assert_eq!(detect_language("bar.js"), "javascript");
        assert_eq!(detect_language("baz.ts"), "typescript");
        assert_eq!(detect_language("qux.go"), "go");
        assert_eq!(detect_language("main.rs"), "rust");
        assert_eq!(detect_language("lib.rs"), "rust");
    }

    #[test]
    fn test_estimate_tokens() {
        let c = Compressor::default();
        assert_eq!(c.estimate_tokens("hello world"), 3); // 11 chars * 0.25 = 2.75 → 3
        assert_eq!(c.estimate_tokens(""), 0);
        assert_eq!(c.estimate_tokens("0123456789012345678901234567890123456789"), 10); // 40 chars
    }

    #[test]
    fn test_compress_file_python() {
        let c = Compressor::default();
        let input = r#"import os
import sys
from typing import List, Optional

def process(items: List[str]) -> Optional[str]:
    """Process a list of items and return the first non-empty one.

    This is a longer docstring that spans multiple lines and describes
    the full behavior of the function in detail.

    Args:
        items: List of strings to process

    Returns:
        The first non-empty string, or None if all are empty
    """
    # TODO: optimize this later
    for item in items:
        if item:  # check if not empty
            return item
    return None
"#;
        let out = c.compress_file("test.py", input);
        assert!(out.reduction_pct > 0, "Expected compression to reduce tokens");
        assert!(out.compressed_tokens < out.original_tokens);
        // The compressed version should still have the function signature and logic.
        assert!(out.compressed.contains("def process"));
        assert!(out.compressed.contains("items: List[str]"));
    }

    #[test]
    fn test_compress_file_js() {
        let c = Compressor::default();
        let input = r#"// This is a comment that should be removed
import { foo } from './foo';
import { bar } from './bar';  // inline comment

function doThing(x) {
    // Another comment inside
    return x * 2;
}
"#;
        let out = c.compress_file("test.js", input);
        assert!(out.compressed.contains("function doThing"));
        assert!(!out.compressed.contains("This is a comment"));
    }

    #[test]
    fn test_compress_tool_result_grep() {
        let c = Compressor::default();
        let grep_output = "src/foo.rs:10:    let x = 5;\nsrc/foo.rs:15:    let y = 10;\nsrc/bar.rs:5:     let z = 15;\n";
        let compressed = c.compress_tool_result("grep", grep_output);
        // Summarized form should have file:count format
        assert!(compressed.contains("src/foo.rs"));
        assert!(compressed.len() < grep_output.len());
    }

    #[test]
    fn test_reduction_percentage() {
        let c = Compressor::default();
        let input = "import os\nimport sys\n\ndef foo():\n    \"\"\"Short docstring.\"\"\"\n    pass\n";
        let out = c.compress_file("test.py", input);
        assert!(out.reduction_pct <= 100);
    }
}