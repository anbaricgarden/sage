use crate::codegraph::formatter::Formatter;
use crate::codegraph::graph::CodeGraph;
use crate::codegraph::ranker::Ranker;
use crate::codegraph::retrieval::EmbeddingProvider;

/// Thin coordinator that delegates ranking to `Ranker` and formatting to `Formatter`.
pub struct ContextAssembler {
    ranker: Ranker,
    formatter: Formatter,
}

impl Default for ContextAssembler {
    fn default() -> Self {
        Self::new()
    }
}

impl ContextAssembler {
    pub fn new() -> Self {
        Self {
            ranker: Ranker::new(),
            formatter: Formatter::new(),
        }
    }

    /// Build or refresh the vector index from all symbols in the graph.
    pub fn index_graph(&mut self, graph: &CodeGraph, provider: &dyn EmbeddingProvider) {
        self.ranker.index_graph(graph, provider);
    }

    /// Query the graph for symbols relevant to `task_description`.
    /// Returns ranked symbol IDs with a fused score.
    pub fn query(
        &self,
        graph: &CodeGraph,
        task_description: &str,
        provider: &dyn EmbeddingProvider,
        top_k: usize,
    ) -> Result<Vec<(String, f64)>, String> {
        self.ranker.query(graph, task_description, provider, top_k)
    }

    /// Assemble a context string from the top-ranked symbols, respecting `token_budget`.
    pub fn assemble_context(
        &self,
        graph: &CodeGraph,
        ranked_ids: &[(String, f64)],
        token_budget: usize,
    ) -> String {
        self.formatter.assemble_context(graph, ranked_ids, token_budget)
    }
}
