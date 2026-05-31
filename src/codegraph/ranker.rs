use crate::ast::Symbol;
use crate::codegraph::graph::CodeGraph;
use crate::codegraph::retrieval::{reciprocal_rank_fusion, EmbeddingProvider, KeywordMatcher, VectorIndex};

/// Combines multiple retrieval signals into a single ranked list.
pub struct Ranker {
    vector_index: VectorIndex,
}

impl Default for Ranker {
    fn default() -> Self {
        Self::new()
    }
}

impl Ranker {
    pub fn new() -> Self {
        Self {
            vector_index: VectorIndex::new(),
        }
    }

    /// Build or refresh the vector index from all symbols in the graph.
    pub fn index_graph(&mut self, graph: &CodeGraph, provider: &dyn EmbeddingProvider) {
        self.vector_index.clear();
        for (id, node) in graph.nodes() {
            let text = symbol_to_text(&node.symbol);
            if let Ok(embedding) = provider.embed(&text) {
                self.vector_index.add(id.clone(), embedding, text);
            }
        }
    }

    /// Query the graph for symbols relevant to `task_description`.
    /// Returns ranked symbol IDs with a fused score combining:
    ///   1. Personalized pageRank with keyword-seeded nodes.
    ///   2. Embedding cosine similarity.
    ///   3. Keyword/regex exact-match scores.
    pub fn query(
        &self,
        graph: &CodeGraph,
        task_description: &str,
        provider: &dyn EmbeddingProvider,
        top_k: usize,
    ) -> Result<Vec<(String, f64)>, String> {
        // 1. Keyword seeds.
        let all_symbols: Vec<Symbol> = graph
            .nodes()
            .values()
            .map(|n| n.symbol.clone())
            .collect();
        let keyword_scores = KeywordMatcher::score(&all_symbols, task_description);

        // Extract seed node IDs from top keyword matches.
        let seeds: Vec<String> = keyword_scores
            .iter()
            .take(5)
            .map(|(id, _)| id.clone())
            .collect();

        // 2. Graph pageRank with seeds.
        let pr_scores = graph.page_rank(&seeds, 0.85, 1e-6, 100);
        let mut pr_ranked: Vec<(String, f64)> = pr_scores.into_iter().collect();
        pr_ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        let pr_ranked: Vec<(String, f64)> = pr_ranked.into_iter().take(top_k).collect();

        // 3. Embedding similarity.
        let query_emb = provider.embed(task_description)?;
        let emb_ranked = self.vector_index.search(&query_emb, top_k);

        // 4. Keyword ranked list.
        let kw_ranked: Vec<(String, f64)> = keyword_scores.into_iter().take(top_k).collect();

        // 5. RRF fusion.
        let fused = reciprocal_rank_fusion(&[pr_ranked, emb_ranked, kw_ranked], 60.0);
        let mut results: Vec<(String, f64)> = fused.into_iter().collect();
        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        results.truncate(top_k);
        Ok(results)
    }
}

fn symbol_to_text(sym: &Symbol) -> String {
    let mut parts = vec![format!("{} {:?}", sym.name, sym.kind)];
    if let Some(ref sig) = sym.signature {
        parts.push(sig.clone());
    }
    if let Some(ref doc) = sym.docstring {
        parts.push(format!("\"{}\"", doc));
    }
    parts.join("\n")
}
