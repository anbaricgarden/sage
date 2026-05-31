use std::collections::HashMap;

use crate::blob_store::BlobStore;

/// A lightweight snapshot of the working tree at a point in time.
/// Stores only blob hash pointers, not full file contents.
#[derive(Debug, Clone, PartialEq)]
pub struct Checkpoint {
    pub id: String,
    pub file_hashes: HashMap<String, String>,
    pub parent: Option<String>,
}

impl Checkpoint {
    pub fn new(id: &str, file_hashes: HashMap<String, String>, parent: Option<String>) -> Self {
        Self {
            id: id.to_string(),
            file_hashes,
            parent,
        }
    }
}

/// Manages checkpoints for rollback and restoration.
pub struct CheckpointManager {
    checkpoints: HashMap<String, Checkpoint>,
}

impl Default for CheckpointManager {
    fn default() -> Self {
        Self::new()
    }
}

impl CheckpointManager {
    pub fn new() -> Self {
        Self {
            checkpoints: HashMap::new(),
        }
    }

    /// Create a checkpoint from the current working tree.
    /// `file_hashes` maps file_path -> blob_hash.
    pub fn create(&mut self, id: &str, file_hashes: HashMap<String, String>, parent: Option<String>) -> String {
        let checkpoint = Checkpoint::new(id, file_hashes, parent);
        let key = checkpoint.id.clone();
        self.checkpoints.insert(key.clone(), checkpoint);
        key
    }

    pub fn get(&self, id: &str) -> Option<&Checkpoint> {
        self.checkpoints.get(id)
    }

    /// Restore file hashes from a checkpoint.
    /// Returns the `file_hashes` map so the caller can write files from the blob store.
    pub fn restore(&self, id: &str) -> Option<HashMap<String, String>> {
        self.checkpoints.get(id).map(|cp| cp.file_hashes.clone())
    }

    /// Reconstruct the actual file contents for a checkpoint using the blob store.
    pub fn restore_contents(
        &self,
        id: &str,
        store: &BlobStore,
    ) -> Option<HashMap<String, String>> {
        let file_hashes = self.restore(id)?;
        let mut contents = HashMap::new();
        for (path, hash) in &file_hashes {
            if let Some(bytes) = store.get(hash)
                && let Ok(text) = String::from_utf8(bytes)
            {
                contents.insert(path.clone(), text);
            }
        }
        Some(contents)
    }

    /// List all checkpoint IDs in chronological order (best-effort via parent chain).
    pub fn lineage(&self, id: &str) -> Vec<String> {
        let mut lineage = Vec::new();
        let mut current = Some(id.to_string());
        while let Some(cid) = current {
            lineage.push(cid.clone());
            current = self
                .checkpoints
                .get(&cid)
                .and_then(|cp| cp.parent.clone());
        }
        lineage
    }
}
