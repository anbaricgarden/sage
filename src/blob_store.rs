use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct BlobStore {
    blobs: Arc<Mutex<HashMap<String, Vec<u8>>>>,
}

impl Default for BlobStore {
    fn default() -> Self {
        Self {
            blobs: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl BlobStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Store content and return its SHA-256 hex hash.
    pub fn put(&self, content: Vec<u8>) -> String {
        let hash = compute_sha256(&content);
        let mut blobs = self.blobs.lock().unwrap();
        blobs.entry(hash.clone()).or_insert(content);
        hash
    }

    /// Retrieve content by its SHA-256 hex hash.
    pub fn get(&self, hash: &str) -> Option<Vec<u8>> {
        let blobs = self.blobs.lock().unwrap();
        blobs.get(hash).cloned()
    }

    /// Check if a hash exists in the store.
    pub fn contains(&self, hash: &str) -> bool {
        let blobs = self.blobs.lock().unwrap();
        blobs.contains_key(hash)
    }
}

fn compute_sha256(content: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content);
    hex::encode(hasher.finalize())
}
