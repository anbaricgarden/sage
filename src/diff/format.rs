use sha2::{Digest, Sha256};
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub struct EditBlock {
    pub file_path: String,
    pub old_anchor: String,
    pub new_anchor: String,
    pub old_lines: Vec<String>,
    pub new_lines: Vec<String>,
    pub context_above: usize,
    pub context_below: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DiffError {
    AnchorNotFound { anchor: String, file_path: String },
    AmbiguousAnchor { anchor: String, matches: usize },
    ContextCollision { anchor: String },
    HashMismatch { expected: String, found: String },
}

impl fmt::Display for DiffError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DiffError::AnchorNotFound { anchor, file_path } => {
                write!(f, "Anchor {} not found in {}", anchor, file_path)
            }
            DiffError::AmbiguousAnchor { anchor, matches } => {
                write!(f, "Anchor {} matches {} locations", anchor, matches)
            }
            DiffError::ContextCollision { anchor } => {
                write!(f, "Context collision for anchor {}", anchor)
            }
            DiffError::HashMismatch { expected, found } => {
                write!(f, "Hash mismatch: expected {}, found {}", expected, found)
            }
        }
    }
}

impl std::error::Error for DiffError {}

pub type DiffResult<T> = Result<T, DiffError>;

impl EditBlock {
    pub fn compute_anchor(
        file_path: &str,
        content: &str,
        target_idx: usize,
        above: usize,
        below: usize,
    ) -> Self {
        let lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
        let start = target_idx.saturating_sub(above);
        let end = (target_idx + 1 + below).min(lines.len());
        let context: Vec<String> = lines[start..end].to_vec();
        let old_anchor = compute_context_hash(file_path, &context);
        Self {
            file_path: file_path.to_string(),
            old_anchor: old_anchor.clone(),
            new_anchor: old_anchor,
            old_lines: context.clone(),
            new_lines: context,
            context_above: above,
            context_below: below,
        }
    }

    pub fn recompute_new_anchor(&mut self) {
        self.new_anchor = compute_context_hash(&self.file_path, &self.new_lines);
    }
}

pub fn compute_context_hash(file_path: &str, context_lines: &[String]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(file_path.as_bytes());
    hasher.update(b"\n");
    for line in context_lines {
        hasher.update(line.as_bytes());
        hasher.update(b"\n");
    }
    let full = hex::encode(hasher.finalize());
    full[..8.min(full.len())].to_string()
}
