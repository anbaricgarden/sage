use std::collections::HashMap;

use crate::ast::Symbol;

#[derive(Debug, Clone, PartialEq)]
pub enum EdgeKind {
    Calls,
    Imports,
    InheritsFrom,
    UsesType,
    Raises,
    TestedBy,
    DefinedIn,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub kind: EdgeKind,
    pub weight: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SymbolNode {
    pub symbol: Symbol,
    pub id: String,
}

pub struct CodeGraph {
    nodes: HashMap<String, SymbolNode>,
    outgoing: HashMap<String, Vec<Edge>>,
    incoming: HashMap<String, Vec<Edge>>,
}

impl Default for CodeGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl CodeGraph {
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            outgoing: HashMap::new(),
            incoming: HashMap::new(),
        }
    }

    fn symbol_id(symbol: &Symbol) -> String {
        format!("{}#{}#{:?}", symbol.file_path, symbol.name, symbol.kind)
    }

    pub fn add_node(&mut self, symbol: Symbol) -> String {
        let id = Self::symbol_id(&symbol);
        let node = SymbolNode {
            symbol,
            id: id.clone(),
        };
        self.nodes.insert(id.clone(), node);
        self.outgoing.entry(id.clone()).or_default();
        self.incoming.entry(id.clone()).or_default();
        id
    }

    pub fn add_edge(&mut self, from: &str, to: &str, kind: EdgeKind, weight: f64) {
        if !self.nodes.contains_key(from) || !self.nodes.contains_key(to) {
            return;
        }
        let edge = Edge {
            from: from.to_string(),
            to: to.to_string(),
            kind,
            weight,
        };
        self.outgoing
            .entry(from.to_string())
            .or_default()
            .push(edge.clone());
        self.incoming
            .entry(to.to_string())
            .or_default()
            .push(edge);
    }

    pub fn remove_node(&mut self, id: &str) {
        if let Some(edges) = self.outgoing.remove(id) {
            for edge in edges {
                if let Some(in_edges) = self.incoming.get_mut(&edge.to) {
                    in_edges.retain(|e| e.from != id);
                }
            }
        }
        if let Some(edges) = self.incoming.remove(id) {
            for edge in edges {
                if let Some(out_edges) = self.outgoing.get_mut(&edge.from) {
                    out_edges.retain(|e| e.to != id);
                }
            }
        }
        self.nodes.remove(id);
    }

    pub fn remove_file_nodes(&mut self, file_path: &str) {
        let to_remove: Vec<String> = self
            .nodes
            .values()
            .filter(|n| n.symbol.file_path == file_path)
            .map(|n| n.id.clone())
            .collect();
        for id in to_remove {
            self.remove_node(&id);
        }
    }

    /// Personalized pageRank with seed nodes as teleportation targets.
    ///
    /// `alpha` is the damping factor (typically 0.85).
    /// `seeds` are the starting node IDs that define the personalization vector.
    /// `tolerance` is the convergence threshold for L1 norm of rank changes.
    pub fn page_rank(
        &self,
        seeds: &[String],
        alpha: f64,
        tolerance: f64,
        max_iters: usize,
    ) -> HashMap<String, f64> {
        let n = self.nodes.len();
        if n == 0 {
            return HashMap::new();
        }

        let ids: Vec<String> = self.nodes.keys().cloned().collect();
        let mut ranks: HashMap<String, f64> =
            ids.iter().map(|id| (id.clone(), 1.0 / n as f64)).collect();

        let seed_set: std::collections::HashSet<&str> = seeds.iter().map(|s| s.as_str()).collect();
        let teleport_prob = if !seeds.is_empty() {
            ids.iter()
                .map(|id| {
                    if seed_set.contains(id.as_str()) {
                        (id.clone(), 1.0 / seeds.len() as f64)
                    } else {
                        (id.clone(), 0.0)
                    }
                })
                .collect::<HashMap<String, f64>>()
        } else {
            ids.iter().map(|id| (id.clone(), 1.0 / n as f64)).collect()
        };

        for _ in 0..max_iters {
            let mut new_ranks: HashMap<String, f64> = HashMap::with_capacity(n);
            let mut delta = 0.0;

            for id in &ids {
                let incoming_edges = self.incoming.get(id).map(|v| v.as_slice()).unwrap_or(&[]);
                let mut sum = 0.0;
                for edge in incoming_edges {
                    let out_degree = self
                        .outgoing
                        .get(&edge.from)
                        .map(|v| v.len() as f64)
                        .unwrap_or(1.0)
                        .max(1.0);
                    let from_rank = ranks.get(&edge.from).unwrap_or(&0.0);
                    sum += from_rank * edge.weight / out_degree;
                }

                let teleport = teleport_prob.get(id).unwrap_or(&0.0);
                let new_rank = (1.0 - alpha) * teleport + alpha * sum;
                let old_rank = ranks.get(id).unwrap_or(&0.0);
                delta += (new_rank - old_rank).abs();
                new_ranks.insert(id.clone(), new_rank);
            }

            ranks = new_ranks;
            if delta < tolerance {
                break;
            }
        }

        ranks
    }

    pub fn nodes(&self) -> &HashMap<String, SymbolNode> {
        &self.nodes
    }

    pub fn get_node(&self, id: &str) -> Option<&SymbolNode> {
        self.nodes.get(id)
    }

    pub fn get_outgoing(&self, id: &str) -> &[Edge] {
        self.outgoing.get(id).map(|v| v.as_slice()).unwrap_or(&[])
    }

    pub fn get_incoming(&self, id: &str) -> &[Edge] {
        self.incoming.get(id).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Rebuild the subgraph for a single file by removing old symbols and adding new ones.
    pub fn update_file(&mut self, file_path: &str, symbols: Vec<Symbol>) {
        self.remove_file_nodes(file_path);
        for symbol in symbols {
            self.add_node(symbol);
        }
    }
}
