use std::collections::HashMap;

use crate::ast::Symbol;
use crate::codegraph::graph::CodeGraph;
use crate::codegraph::retrieval::{reciprocal_rank_fusion, EmbeddingProvider, KeywordMatcher, VectorIndex};

/// Assembles a context prompt from the CodeGraph based on a task description,
/// respecting a token budget and using dynamic granularity.
pub struct ContextAssembler {
    vector_index: VectorIndex,
    /// Approximate tokens per character (0.25 is a rough heuristic for code).
    tokens_per_char: f64,
}

impl Default for ContextAssembler {
    fn default() -> Self {
        Self::new()
    }
}

impl ContextAssembler {
    pub fn new() -> Self {
        Self {
            vector_index: VectorIndex::new(),
            tokens_per_char: 0.25,
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

    /// Assemble a context string from the top-ranked symbols, respecting `token_budget`.
    /// Uses dynamic granularity: starts with symbol bodies, expands to enclosing class/file
    /// if budget allows.
    pub fn assemble_context(
        &self,
        graph: &CodeGraph,
        ranked_ids: &[(String, f64)],
        _file_contents: &HashMap<String, String>,
        token_budget: usize,
    ) -> String {
        let mut parts: Vec<String> = Vec::new();
        let mut used_tokens: usize = 0;
        let budget = token_budget;

        for (id, _score) in ranked_ids {
            let Some(node) = graph.get_node(id) else { continue };
            let sym = &node.symbol;

            // Try symbol-level granularity first.
            let text = symbol_to_text(sym);
            let sym_tokens = self.estimate_tokens(&text);
            if used_tokens + sym_tokens < budget {
                parts.push(format!(
                    "// {}:{} {}\n{}",
                    sym.file_path,
                    sym.start_line,
                    sym.name,
                    text
                ));
                used_tokens += sym_tokens;
                continue;
            }

            // If no room for full symbol, try signature-only.
            if let Some(ref sig) = sym.signature {
                let sig_tokens = self.estimate_tokens(sig);
                if used_tokens + sig_tokens < budget {
                    parts.push(format!(
                        "// {}:{} {} (signature only)\n{}",
                        sym.file_path, sym.start_line, sym.name, sig
                    ));
                    used_tokens += sig_tokens;
                }
            }
        }

        // If we still have budget, add file-level headers for the files represented.
        if used_tokens < budget {
            let mut seen_files: std::collections::HashSet<&str> = std::collections::HashSet::new();
            for (id, _score) in ranked_ids {
                let Some(node) = graph.get_node(id) else { continue };
                let file = node.symbol.file_path.as_str();
                if seen_files.insert(file) {
                    let header = format!("// File: {}\n", file);
                    let header_tokens = self.estimate_tokens(&header);
                    if used_tokens + header_tokens < budget {
                        parts.insert(0, header);
                        used_tokens += header_tokens;
                    }
                }
            }
        }

        parts.join("\n\n")
    }

    fn estimate_tokens(&self, text: &str) -> usize {
        ((text.len() as f64) * self.tokens_per_char).ceil() as usize
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
