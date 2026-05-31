use std::collections::HashSet;

use crate::codegraph::graph::CodeGraph;

/// Formats ranked symbols into a context string, respecting a token budget
/// and using dynamic granularity (full body → signature → file header).
pub struct Formatter {
    /// Approximate tokens per character (0.25 is a rough heuristic for code).
    tokens_per_char: f64,
}

impl Default for Formatter {
    fn default() -> Self {
        Self::new()
    }
}

impl Formatter {
    pub fn new() -> Self {
        Self {
            tokens_per_char: 0.25,
        }
    }

    /// Assemble a context string from the top-ranked symbols, respecting `token_budget`.
    pub fn assemble_context(
        &self,
        graph: &CodeGraph,
        ranked_ids: &[(String, f64)],
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
            let mut seen_files: HashSet<&str> = HashSet::new();
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

fn symbol_to_text(sym: &crate::ast::Symbol) -> String {
    let mut parts = vec![format!("{} {:?}", sym.name, sym.kind)];
    if let Some(ref sig) = sym.signature {
        parts.push(sig.clone());
    }
    if let Some(ref doc) = sym.docstring {
        parts.push(format!("\"{}\"", doc));
    }
    parts.join("\n")
}
