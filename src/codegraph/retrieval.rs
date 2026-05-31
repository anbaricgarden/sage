use std::collections::HashMap;

use crate::ast::Symbol;

/// Abstract interface for API-based text embeddings.
/// Concrete implementations call OpenAI, Cohere, or a local embedding service.
pub trait EmbeddingProvider: Send + Sync {
    /// Embed a single text string. Returns a normalized embedding vector.
    fn embed(&self, text: &str) -> Result<Vec<f32>, String>;
    /// Batch embed multiple texts.
    fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
        texts.iter().map(|t| self.embed(t)).collect()
    }
}

/// A mock embedding provider for tests and offline operation.
/// Uses a simple hash-based pseudo-embedding so the same text always produces
/// the same vector, and different texts produce uncorrelated vectors.
pub struct MockEmbeddingProvider {
    pub dim: usize,
}

impl MockEmbeddingProvider {
    pub fn new(dim: usize) -> Self {
        Self { dim }
    }
}

impl EmbeddingProvider for MockEmbeddingProvider {
    fn embed(&self, text: &str) -> Result<Vec<f32>, String> {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        text.hash(&mut hasher);
        let seed = hasher.finish();
        let mut vec = Vec::with_capacity(self.dim);
        let mut state = seed;
        for _ in 0..self.dim {
            state = state.wrapping_mul(1103515245).wrapping_add(12345);
            let val = ((state & 0x7FFF) as f32) / 32768.0; // [0, 1)
            vec.push(val);
        }
        // Normalize.
        let norm_sq: f32 = vec.iter().map(|v| v * v).sum();
        let norm = norm_sq.sqrt().max(1e-8);
        for v in &mut vec {
            *v /= norm;
        }
        Ok(vec)
    }
}

/// In-memory vector store with brute-force cosine similarity search.
pub struct VectorIndex {
    entries: Vec<VectorEntry>,
}

struct VectorEntry {
    symbol_id: String,
    embedding: Vec<f32>,
    #[allow(dead_code)]
    text: String,
}

impl Default for VectorIndex {
    fn default() -> Self {
        Self::new()
    }
}

impl VectorIndex {
    pub fn new() -> Self {
        Self { entries: Vec::new() }
    }

    pub fn add(&mut self, symbol_id: String, embedding: Vec<f32>, text: String) {
        self.entries.push(VectorEntry {
            symbol_id,
            embedding,
            text,
        });
    }

    pub fn search(&self, query_embedding: &[f32], top_k: usize) -> Vec<(String, f64)> {
        let mut scored: Vec<(String, f64)> = self
            .entries
            .iter()
            .map(|e| {
                let sim = cosine_similarity(query_embedding, &e.embedding);
                (e.symbol_id.clone(), sim as f64)
            })
            .collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        scored.truncate(top_k);
        scored
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    dot / (norm_a * norm_b).max(1e-8)
}

/// Simple keyword/regex fallback matcher.
pub struct KeywordMatcher;

impl KeywordMatcher {
    pub fn score(symbols: &[Symbol], query: &str) -> Vec<(String, f64)> {
        let query_lower = query.to_lowercase();
        let keywords: Vec<&str> = query_lower.split_whitespace().collect();
        let mut scored = Vec::new();
        for sym in symbols {
            let text = format!(
                "{} {} {:?} {}",
                sym.name,
                sym.file_path,
                sym.kind,
                sym.docstring.as_deref().unwrap_or("")
            )
            .to_lowercase();
            let mut score = 0.0;
            for kw in &keywords {
                if text.contains(kw) {
                    score += 1.0;
                    // Bonus for exact name match.
                    if sym.name.to_lowercase() == *kw {
                        score += 2.0;
                    }
                }
            }
            if score > 0.0 {
                let id = format!("{}#{}#{:?}", sym.file_path, sym.name, sym.kind);
                scored.push((id, score));
            }
        }
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        scored
    }
}

/// Reciprocal Rank Fusion (RRF) combines multiple ranked lists into a single
/// score. Each list contributes `1 / (k + rank)` where `k` is a constant
/// (typically 60) and rank is 1-based.
pub fn reciprocal_rank_fusion(
    ranked_lists: &[Vec<(String, f64)>],
    k: f64,
) -> HashMap<String, f64> {
    let mut fused: HashMap<String, f64> = HashMap::new();
    for list in ranked_lists {
        for (rank, (id, _score)) in list.iter().enumerate() {
            let rrf_score = 1.0 / (k + (rank + 1) as f64);
            *fused.entry(id.clone()).or_insert(0.0) += rrf_score;
        }
    }
    fused
}
