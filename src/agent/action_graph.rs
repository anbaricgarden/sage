use std::collections::{HashMap, HashSet, VecDeque};

/// Type of action that a node in the action graph can represent.
#[derive(Debug, Clone, PartialEq)]
pub enum ActionType {
    Edit {
        file_path: String,
        description: String,
    },
    ToolCall {
        tool: String,
        arguments: HashMap<String, String>,
    },
    Validate {
        criteria: String,
    },
    Checkpoint,
}

/// A single node in the action graph.
#[derive(Debug, Clone, PartialEq)]
pub struct ActionNode {
    pub id: String,
    pub action_type: ActionType,
    pub dependencies: Vec<String>,
    pub speculative: bool,
    /// Maximum tokens this action may consume.
    pub token_budget: usize,
    /// Optional result payload populated after execution.
    pub result: Option<String>,
    /// Whether the action succeeded (None = not yet executed).
    pub succeeded: Option<bool>,
}

impl ActionNode {
    pub fn new(id: &str, action_type: ActionType) -> Self {
        Self {
            id: id.to_string(),
            action_type,
            dependencies: Vec::new(),
            speculative: false,
            token_budget: 0,
            result: None,
            succeeded: None,
        }
    }

    pub fn with_dependencies(mut self, deps: Vec<String>) -> Self {
        self.dependencies = deps;
        self
    }

    pub fn with_speculative(mut self, speculative: bool) -> Self {
        self.speculative = speculative;
        self
    }

    pub fn with_budget(mut self, budget: usize) -> Self {
        self.token_budget = budget;
        self
    }
}

/// Directed acyclic graph of actions for non-linear agent execution.
#[derive(Debug, Default)]
pub struct ActionGraph {
    nodes: HashMap<String, ActionNode>,
    /// Edges as (from_id, to_id) representing dependency direction.
    edges: Vec<(String, String)>,
}

impl ActionGraph {
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            edges: Vec::new(),
        }
    }

    pub fn add_node(&mut self, node: ActionNode) {
        for dep in &node.dependencies {
            self.edges.push((dep.clone(), node.id.clone()));
        }
        self.nodes.insert(node.id.clone(), node);
    }

    pub fn get_node(&self, id: &str) -> Option<&ActionNode> {
        self.nodes.get(id)
    }

    pub fn get_node_mut(&mut self, id: &str) -> Option<&mut ActionNode> {
        self.nodes.get_mut(id)
    }

    pub fn nodes(&self) -> &HashMap<String, ActionNode> {
        &self.nodes
    }

    pub fn nodes_mut(&mut self) -> &mut HashMap<String, ActionNode> {
        &mut self.nodes
    }

    pub fn edges(&self) -> &[(String, String)] {
        &self.edges
    }

    /// Remove a node and all edges connected to it.
    pub fn remove_node(&mut self, id: &str) {
        self.nodes.remove(id);
        self.edges.retain(|(from, to)| from != id && to != id);
    }

    /// Group nodes into parallel-executable batches using topological sort.
    /// Each batch contains nodes whose dependencies are all satisfied by previous batches.
    pub fn topological_batches(&self) -> Vec<HashSet<String>> {
        let mut in_degree: HashMap<String, usize> = HashMap::new();
        let mut adj: HashMap<String, Vec<String>> = HashMap::new();

        for id in self.nodes.keys() {
            in_degree.entry(id.clone()).or_insert(0);
        }

        for (from, to) in &self.edges {
            adj.entry(from.clone()).or_default().push(to.clone());
            *in_degree.entry(to.clone()).or_insert(0) += 1;
        }

        let mut queue: VecDeque<String> = VecDeque::new();
        for (id, degree) in &in_degree {
            if *degree == 0 {
                queue.push_back(id.clone());
            }
        }

        let mut batches: Vec<HashSet<String>> = Vec::new();
        while !queue.is_empty() {
            let batch_size = queue.len();
            let mut batch: HashSet<String> = HashSet::new();
            for _ in 0..batch_size {
                let id = queue.pop_front().unwrap();
                batch.insert(id.clone());
                if let Some(neighbors) = adj.get(&id) {
                    for neighbor in neighbors {
                        let deg = in_degree.get_mut(neighbor).unwrap();
                        *deg -= 1;
                        if *deg == 0 {
                            queue.push_back(neighbor.clone());
                        }
                    }
                }
            }
            batches.push(batch);
        }

        batches
    }

    /// Return the longest dependency chain (critical path) as a list of node IDs.
    pub fn critical_path(&self) -> Vec<String> {
        let mut longest_path_to: HashMap<String, Vec<String>> = HashMap::new();

        // Initialize with self-only paths.
        for id in self.nodes.keys() {
            longest_path_to.insert(id.clone(), vec![id.clone()]);
        }

        let batches = self.topological_batches();
        for batch in &batches {
            for id in batch {
                if let Some(node) = self.nodes.get(id) {
                    let current_path = longest_path_to.get(id).cloned().unwrap_or_default();
                    for dep in &node.dependencies {
                        let mut candidate = longest_path_to.get(dep).cloned().unwrap_or_default();
                        candidate.extend(current_path.clone());
                        let existing = longest_path_to.entry(id.clone()).or_default();
                        if candidate.len() > existing.len() {
                            *existing = candidate;
                        }
                    }
                }
            }
        }

        longest_path_to
            .values()
            .max_by_key(|path| path.len())
            .cloned()
            .unwrap_or_default()
    }

    /// Mark a node and all nodes that transitively depend on it as cancelled.
    pub fn cancel_branch(&mut self, root_id: &str) {
        let mut to_cancel: HashSet<String> = HashSet::new();
        to_cancel.insert(root_id.to_string());

        let mut changed = true;
        while changed {
            changed = false;
            for (from, to) in &self.edges {
                if to_cancel.contains(from) && !to_cancel.contains(to) {
                    to_cancel.insert(to.clone());
                    changed = true;
                }
            }
        }

        for id in &to_cancel {
            if let Some(node) = self.nodes.get_mut(id) {
                node.succeeded = Some(false);
            }
        }
    }

    /// Return all nodes that are ready to execute (all dependencies satisfied
    /// and the node itself has not yet been executed).
    pub fn ready_nodes(&self) -> Vec<String> {
        self.nodes
            .values()
            .filter(|node| {
                node.succeeded.is_none()
                    && node.dependencies.iter().all(|dep| {
                        self.nodes
                            .get(dep)
                            .and_then(|n| n.succeeded)
                            .unwrap_or(false)
                    })
            })
            .map(|node| node.id.clone())
            .collect()
    }

    /// Total token budget allocated across all nodes.
    pub fn total_budget(&self) -> usize {
        self.nodes.values().map(|n| n.token_budget).sum()
    }

    /// Count of nodes that have succeeded.
    pub fn succeeded_count(&self) -> usize {
        self.nodes
            .values()
            .filter(|n| n.succeeded == Some(true))
            .count()
    }

    /// Count of nodes that have failed.
    pub fn failed_count(&self) -> usize {
        self.nodes
            .values()
            .filter(|n| n.succeeded == Some(false))
            .count()
    }

    /// True if all non-speculative nodes have been executed (succeeded or failed).
    pub fn is_complete(&self) -> bool {
        self.nodes.values().all(|n| {
            n.succeeded.is_some() || n.speculative
        })
    }
}
