use std::collections::{HashMap, HashSet};

/// A single visible entry in the flattened file tree.
#[derive(Debug, Clone)]
pub struct TreeEntry {
    /// Full path (e.g. "src/agent/mod.rs" or "src/agent").
    pub path: String,
    /// Display name (last path segment).
    pub name: String,
    /// Nesting depth (0 = root).
    pub depth: usize,
    /// True if this is a directory.
    pub is_dir: bool,
    /// True if the directory is currently expanded.
    pub is_expanded: bool,
}

/// Build a flat list of visible tree entries from a set of file paths.
pub fn build_visible_tree(
    file_paths: &[String],
    expanded_dirs: &HashSet<String>,
    filter: &str,
) -> Vec<TreeEntry> {
    // Build a prefix tree (trie) from the file paths.
    let mut root = DirNode::default();
    for path in file_paths {
        root.insert(path);
    }

    let filter_lower = filter.to_lowercase();
    let mut result = Vec::new();

    if filter.is_empty() {
        // Normal tree mode: respect expanded_dirs.
        root.walk("", expanded_dirs, &mut result, 0);
    } else {
        // Filter mode: show all matching entries and their ancestors.
        root.walk_filtered("", &filter_lower, &mut result, 0);
    }

    result
}

/// Internal node in the prefix tree.
#[derive(Default)]
struct DirNode {
    /// name -> child directory
    dirs: HashMap<String, DirNode>,
    /// Files directly in this directory.
    files: Vec<String>,
}

impl DirNode {
    fn insert(&mut self, path: &str) {
        let parts: Vec<&str> = path.split('/').collect();
        if parts.is_empty() {
            return;
        }
        let mut current = self;
        for (i, part) in parts.iter().enumerate() {
            if i + 1 == parts.len() {
                // Last part is a file.
                current.files.push(part.to_string());
            } else {
                current = current
                    .dirs
                    .entry(part.to_string())
                    .or_default();
            }
        }
    }

    fn walk(
        &self,
        prefix: &str,
        expanded: &HashSet<String>,
        out: &mut Vec<TreeEntry>,
        depth: usize,
    ) {
        // Directories first, sorted.
        let mut dir_names: Vec<&String> = self.dirs.keys().collect();
        dir_names.sort();
        for name in dir_names {
            let path = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{}/{}", prefix, name)
            };
            let is_expanded = expanded.contains(&path);
            out.push(TreeEntry {
                path: path.clone(),
                name: name.clone(),
                depth,
                is_dir: true,
                is_expanded,
            });
            if is_expanded {
                let child = self.dirs.get(name).unwrap();
                child.walk(&path, expanded, out, depth + 1);
            }
        }

        // Then files, sorted.
        let mut files = self.files.clone();
        files.sort();
        for name in files {
            let path = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{}/{}", prefix, name)
            };
            out.push(TreeEntry {
                path,
                name,
                depth,
                is_dir: false,
                is_expanded: false,
            });
        }
    }

    fn walk_filtered(
        &self,
        prefix: &str,
        filter: &str,
        out: &mut Vec<TreeEntry>,
        depth: usize,
    ) {
        let mut dir_names: Vec<&String> = self.dirs.keys().collect();
        dir_names.sort();
        for name in dir_names {
            let path = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{}/{}", prefix, name)
            };
            let child = self.dirs.get(name).unwrap();
            let matches = path.to_lowercase().contains(filter);
            let child_matches = child.matches(filter);
            if matches || child_matches {
                out.push(TreeEntry {
                    path: path.clone(),
                    name: name.clone(),
                    depth,
                    is_dir: true,
                    is_expanded: true, // Always expanded in filter mode.
                });
                child.walk_filtered(&path, filter, out, depth + 1);
            }
        }

        let mut files = self.files.clone();
        files.sort();
        for name in files {
            let path = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{}/{}", prefix, name)
            };
            if path.to_lowercase().contains(filter) {
                out.push(TreeEntry {
                    path,
                    name,
                    depth,
                    is_dir: false,
                    is_expanded: false,
                });
            }
        }
    }

    fn matches(&self, filter: &str) -> bool {
        for name in &self.files {
            if name.to_lowercase().contains(filter) {
                return true;
            }
        }
        for (name, child) in &self.dirs {
            if name.to_lowercase().contains(filter) || child.matches(filter) {
                return true;
            }
        }
        false
    }
}
