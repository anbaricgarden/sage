use std::collections::HashMap;

use crate::codegraph::graph::CodeGraph;

/// Generates a concise file-level overview ("repo map") of the codebase.
/// For each file, it lists the top-N symbols ranked by a global pageRank score.
pub struct RepoMap;

impl RepoMap {
    /// Build a repo-map string.
    ///
    /// * `graph` — the populated `CodeGraph`.
    /// * `top_n_per_file` — how many top-ranked symbols to show per file.
    /// * `max_files` — optional cap on the number of files to include.
    pub fn generate(graph: &CodeGraph, top_n_per_file: usize, max_files: Option<usize>) -> String {
        // Compute global pageRank with no seeds (uniform teleport).
        let ranks = graph.page_rank(&[], 0.85, 1e-6, 100);

        // Group symbols by file.
        let mut by_file: HashMap<String, Vec<(String, f64)>> = HashMap::new();
        for (id, node) in graph.nodes() {
            let rank = ranks.get(id).copied().unwrap_or(0.0);
            by_file
                .entry(node.symbol.file_path.clone())
                .or_default()
                .push((id.clone(), rank));
        }

        // Sort each file's symbols by rank descending.
        for symbols in by_file.values_mut() {
            symbols.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        }

        // Sort files by their highest-ranked symbol's score.
        let mut files: Vec<(String, Vec<(String, f64)>)> = by_file.into_iter().collect();
        files.sort_by(|a, b| {
            let a_best = a.1.first().map(|(_, r)| *r).unwrap_or(0.0);
            let b_best = b.1.first().map(|(_, r)| *r).unwrap_or(0.0);
            b_best.partial_cmp(&a_best).unwrap()
        });

        if let Some(max) = max_files {
            files.truncate(max);
        }

        let mut lines = Vec::new();
        for (file_path, symbols) in files {
            lines.push(file_path);
            for (id, _rank) in symbols.iter().take(top_n_per_file) {
                if let Some(node) = graph.get_node(id) {
                    let sym = &node.symbol;
                    let sig_hint = sym
                        .signature
                        .as_ref()
                        .map(|s| {
                            // Truncate long signatures for the map.
                            if s.len() > 60 {
                                format!("{}...", &s[..57])
                            } else {
                                s.clone()
                            }
                        })
                        .unwrap_or_default();
                    lines.push(format!(
                        "  {} {:?} L{}-{} {}",
                        sym.name,
                        sym.kind,
                        sym.start_line,
                        sym.end_line,
                        sig_hint
                    ));
                }
            }
        }

        lines.join("\n")
    }
}
