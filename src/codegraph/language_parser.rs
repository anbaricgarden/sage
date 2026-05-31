use crate::ast::SymbolKind;
use std::collections::HashMap;
use tree_sitter::Node;

/// A language-specific parser that extracts cross-reference edges from source code.
/// Each adapter implements tree-sitter traversal for one language.
pub trait LanguageParser: Send + Sync {
    /// The tree-sitter language this parser handles.
    fn tree_sitter_language(&self) -> tree_sitter::Language;

    /// The canonical name of the language (e.g. "python", "javascript", "go").
    fn name(&self) -> &'static str;

    /// Collect edges from the parsed AST.
    ///
    /// `symbol_map` maps line numbers → symbol names for scope resolution.
    /// Returns `(from_symbol_id, EdgeKind, to_symbol_name)` tuples.
    fn collect_edges(
        &self,
        root: Node,
        code: &str,
        file_path: &str,
        symbol_map: &HashMap<usize, String>,
    ) -> Vec<(String, super::EdgeKind, String)>;
}

// ── Python ──

pub struct PythonParser;

impl LanguageParser for PythonParser {
    fn tree_sitter_language(&self) -> tree_sitter::Language {
        tree_sitter_python::language()
    }

    fn name(&self) -> &'static str {
        "python"
    }

    fn collect_edges(
        &self,
        root: Node,
        code: &str,
        file_path: &str,
        symbol_map: &HashMap<usize, String>,
    ) -> Vec<(String, super::EdgeKind, String)> {
        let mut edges = Vec::new();
        Self::walk(root, code, file_path, symbol_map, &mut edges);
        edges
    }
}

impl PythonParser {
    fn walk(
        node: Node,
        code: &str,
        file_path: &str,
        symbol_map: &HashMap<usize, String>,
        edges: &mut Vec<(String, super::EdgeKind, String)>,
    ) {
        let kind = node.kind();
        let start_line = node.start_position().row;
        let scope_symbol = symbol_map.get(&start_line).cloned();

        match kind {
            "call" => {
                if let (Some(from), Some(callee)) = (scope_symbol.clone(), extract_callee_name(node, code)) {
                    edges.push((
                        format!("{}#{:?}", from, SymbolKind::Function),
                        super::EdgeKind::Calls,
                        callee,
                    ));
                }
            }
            "import_statement" | "import_from_statement" => {
                if let Some(names) = extract_import_names(node, code) {
                    for name in names {
                        edges.push((
                            format!("{}#module", file_path),
                            super::EdgeKind::Imports,
                            name,
                        ));
                    }
                }
            }
            "class_definition" => {
                if let (Some(from), Some(base)) = (scope_symbol.clone(), extract_class_bases_python(node, code)) {
                    for base_name in base {
                        edges.push((
                            format!("{}#class", from),
                            super::EdgeKind::InheritsFrom,
                            base_name,
                        ));
                    }
                }
            }
            _ => {}
        }

        let mut cursor = node.walk();
        if cursor.goto_first_child() {
            loop {
                Self::walk(cursor.node(), code, file_path, symbol_map, edges);
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
        }
    }
}

// ── JavaScript / TypeScript ──

pub struct JavaScriptParser;

impl LanguageParser for JavaScriptParser {
    fn tree_sitter_language(&self) -> tree_sitter::Language {
        tree_sitter_javascript::language()
    }

    fn name(&self) -> &'static str {
        "javascript"
    }

    fn collect_edges(
        &self,
        root: Node,
        code: &str,
        file_path: &str,
        symbol_map: &HashMap<usize, String>,
    ) -> Vec<(String, super::EdgeKind, String)> {
        let mut edges = Vec::new();
        Self::walk(root, code, file_path, symbol_map, &mut edges);
        edges
    }
}

impl JavaScriptParser {
    fn walk(
        node: Node,
        code: &str,
        file_path: &str,
        symbol_map: &HashMap<usize, String>,
        edges: &mut Vec<(String, super::EdgeKind, String)>,
    ) {
        let kind = node.kind();
        let start_line = node.start_position().row;
        let scope_symbol = symbol_map.get(&start_line).cloned();

        match kind {
            "call_expression" => {
                if let (Some(from), Some(callee)) = (scope_symbol.clone(), extract_callee_name_js(node, code)) {
                    edges.push((
                        format!("{}#function", from),
                        super::EdgeKind::Calls,
                        callee,
                    ));
                }
            }
            "import_statement" | "import_declaration" => {
                if let Some(names) = extract_import_names_js(node, code) {
                    for name in names {
                        edges.push((
                            format!("{}#module", file_path),
                            super::EdgeKind::Imports,
                            name,
                        ));
                    }
                }
            }
            "class_declaration" | "class_" => {
                if let (Some(from), Some(bases)) = (scope_symbol.clone(), extract_class_bases_js(node, code)) {
                    for base_name in bases {
                        edges.push((
                            format!("{}#class", from),
                            super::EdgeKind::InheritsFrom,
                            base_name,
                        ));
                    }
                }
            }
            _ => {}
        }

        let mut cursor = node.walk();
        if cursor.goto_first_child() {
            loop {
                Self::walk(cursor.node(), code, file_path, symbol_map, edges);
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
        }
    }
}

pub struct TypeScriptParser;

impl LanguageParser for TypeScriptParser {
    fn tree_sitter_language(&self) -> tree_sitter::Language {
        tree_sitter_typescript::language_typescript()
    }

    fn name(&self) -> &'static str {
        "typescript"
    }

    fn collect_edges(
        &self,
        root: Node,
        code: &str,
        file_path: &str,
        symbol_map: &HashMap<usize, String>,
    ) -> Vec<(String, super::EdgeKind, String)> {
        // TS shares the same AST shapes as JS for the edges we care about.
        JavaScriptParser::walk(root, code, file_path, symbol_map, &mut Vec::new());
        let mut edges = Vec::new();
        JavaScriptParser::walk(root, code, file_path, symbol_map, &mut edges);
        edges
    }
}

// ── Go ──

pub struct GoParser;

impl LanguageParser for GoParser {
    fn tree_sitter_language(&self) -> tree_sitter::Language {
        tree_sitter_go::language()
    }

    fn name(&self) -> &'static str {
        "go"
    }

    fn collect_edges(
        &self,
        root: Node,
        code: &str,
        file_path: &str,
        symbol_map: &HashMap<usize, String>,
    ) -> Vec<(String, super::EdgeKind, String)> {
        let mut edges = Vec::new();
        Self::walk(root, code, file_path, symbol_map, &mut edges);
        edges
    }
}

impl GoParser {
    fn walk(
        node: Node,
        code: &str,
        file_path: &str,
        symbol_map: &HashMap<usize, String>,
        edges: &mut Vec<(String, super::EdgeKind, String)>,
    ) {
        let kind = node.kind();
        let start_line = node.start_position().row;
        let scope_symbol = symbol_map.get(&start_line).cloned();

        match kind {
            "call_expression" => {
                if let (Some(from), Some(callee)) = (scope_symbol.clone(), extract_callee_name_go(node, code)) {
                    edges.push((
                        format!("{}#function", from),
                        super::EdgeKind::Calls,
                        callee,
                    ));
                }
            }
            "import_spec" | "import_declaration" => {
                if let Some(names) = extract_import_names_go(node, code) {
                    for name in names {
                        edges.push((
                            format!("{}#module", file_path),
                            super::EdgeKind::Imports,
                            name,
                        ));
                    }
                }
            }
            "type_spec" => {
                if let Some((name, bases)) = extract_type_spec_go(node, code) {
                    for base in bases {
                        edges.push((
                            format!("{}#type", name),
                            super::EdgeKind::InheritsFrom,
                            base,
                        ));
                    }
                }
            }
            _ => {}
        }

        let mut cursor = node.walk();
        if cursor.goto_first_child() {
            loop {
                Self::walk(cursor.node(), code, file_path, symbol_map, edges);
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
        }
    }
}

// ── Registry ──

/// Registry of language parsers. Adding a new language means registering a new adapter here.
pub struct ParserRegistry {
    parsers: HashMap<String, Box<dyn LanguageParser>>,
}

impl Default for ParserRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ParserRegistry {
    pub fn new() -> Self {
        let mut parsers: HashMap<String, Box<dyn LanguageParser>> = HashMap::new();
        let py: Box<dyn LanguageParser> = Box::new(PythonParser);
        parsers.insert("python".to_string(), py);
        parsers.insert("py".to_string(), Box::new(PythonParser));

        let js: Box<dyn LanguageParser> = Box::new(JavaScriptParser);
        parsers.insert("javascript".to_string(), js);
        parsers.insert("js".to_string(), Box::new(JavaScriptParser));

        let ts: Box<dyn LanguageParser> = Box::new(TypeScriptParser);
        parsers.insert("typescript".to_string(), ts);
        parsers.insert("ts".to_string(), Box::new(TypeScriptParser));

        let go: Box<dyn LanguageParser> = Box::new(GoParser);
        parsers.insert("go".to_string(), go);
        parsers.insert("golang".to_string(), Box::new(GoParser));

        Self { parsers }
    }

    pub fn get(&self, language: &str) -> Option<&dyn LanguageParser> {
        self.parsers.get(&language.to_lowercase()).map(|b| b.as_ref())
    }

    pub fn register(&mut self, name: &str, parser: Box<dyn LanguageParser>) {
        self.parsers.insert(name.to_lowercase(), parser);
    }
}

// ── Extraction helpers (language-agnostic tree-sitter traversal) ──

fn extract_callee_name(node: Node, code: &str) -> Option<String> {
    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            let child = cursor.node();
            if child.kind() == "identifier" || child.kind() == "attribute" {
                return Some(child.utf8_text(code.as_bytes()).ok()?.to_string());
            }
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
    None
}

fn extract_import_names(node: Node, code: &str) -> Option<Vec<String>> {
    let mut names = Vec::new();
    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            let child = cursor.node();
            if child.kind() == "dotted_name" || child.kind() == "identifier" {
                let text = child.utf8_text(code.as_bytes()).ok()?.to_string();
                names.push(text);
            }
            if child.kind() == "import_clause" || child.kind() == "aliased_import" {
                let mut inner = child.walk();
                if inner.goto_first_child() {
                    loop {
                        let inner_child = inner.node();
                        if inner_child.kind() == "identifier" || inner_child.kind() == "dotted_name" {
                            let text = inner_child.utf8_text(code.as_bytes()).ok()?.to_string();
                            names.push(text);
                        }
                        if !inner.goto_next_sibling() {
                            break;
                        }
                    }
                }
            }
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
    if names.is_empty() { None } else { Some(names) }
}

fn extract_class_bases_python(node: Node, code: &str) -> Option<Vec<String>> {
    let mut bases = Vec::new();
    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            let child = cursor.node();
            if child.kind() == "argument_list" || child.kind() == "base_list" {
                let mut inner = child.walk();
                if inner.goto_first_child() {
                    loop {
                        let inner_child = inner.node();
                        if (inner_child.kind() == "identifier" || inner_child.kind() == "attribute")
                            && let Ok(text) = inner_child.utf8_text(code.as_bytes())
                        {
                            bases.push(text.to_string());
                        }
                        if !inner.goto_next_sibling() {
                            break;
                        }
                    }
                }
            }
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
    if bases.is_empty() { None } else { Some(bases) }
}

// JS helpers
fn extract_callee_name_js(node: Node, code: &str) -> Option<String> {
    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            let child = cursor.node();
            if child.kind() == "identifier" || child.kind() == "member_expression" {
                return Some(child.utf8_text(code.as_bytes()).ok()?.to_string());
            }
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
    None
}

fn extract_import_names_js(node: Node, code: &str) -> Option<Vec<String>> {
    let mut names = Vec::new();
    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            let child = cursor.node();
            if child.kind() == "import_clause" || child.kind() == "string" {
                let text = child.utf8_text(code.as_bytes()).ok()?.to_string();
                names.push(text);
            }
            if child.kind() == "identifier" {
                let text = child.utf8_text(code.as_bytes()).ok()?.to_string();
                names.push(text);
            }
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
    if names.is_empty() { None } else { Some(names) }
}

fn extract_class_bases_js(node: Node, code: &str) -> Option<Vec<String>> {
    let mut bases = Vec::new();
    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            let child = cursor.node();
            if child.kind() == "class_heritage" || child.kind() == "extends_clause" {
                let mut inner = child.walk();
                if inner.goto_first_child() {
                    loop {
                        let inner_child = inner.node();
                        if (inner_child.kind() == "identifier" || inner_child.kind() == "member_expression")
                            && let Ok(text) = inner_child.utf8_text(code.as_bytes())
                        {
                            bases.push(text.to_string());
                        }
                        if !inner.goto_next_sibling() {
                            break;
                        }
                    }
                }
            }
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
    if bases.is_empty() { None } else { Some(bases) }
}

// Go helpers
fn extract_callee_name_go(node: Node, code: &str) -> Option<String> {
    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            let child = cursor.node();
            if child.kind() == "identifier" || child.kind() == "selector_expression" {
                return Some(child.utf8_text(code.as_bytes()).ok()?.to_string());
            }
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
    None
}

fn extract_import_names_go(node: Node, code: &str) -> Option<Vec<String>> {
    let mut names = Vec::new();
    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            let child = cursor.node();
            if child.kind() == "import_spec_list" {
                let mut inner = child.walk();
                if inner.goto_first_child() {
                    loop {
                        let spec = inner.node();
                        if spec.kind() == "import_spec" {
                            names.extend(extract_string_from_import_spec(spec, code));
                        }
                        if !inner.goto_next_sibling() {
                            break;
                        }
                    }
                }
            } else if child.kind() == "import_spec" {
                names.extend(extract_string_from_import_spec(child, code));
            }
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
    if names.is_empty() { None } else { Some(names) }
}

fn extract_string_from_import_spec(node: Node, code: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            let child = cursor.node();
            if (child.kind() == "interpreted_string_literal" || child.kind() == "raw_string_literal")
                && let Ok(text) = child.utf8_text(code.as_bytes())
            {
                let clean = text.trim_matches('"').trim_matches('`').to_string();
                if !clean.is_empty() {
                    out.push(clean);
                }
            }
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
    out
}

fn extract_type_spec_go(node: Node, code: &str) -> Option<(String, Vec<String>)> {
    let mut name = None;
    let mut bases = Vec::new();
    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            let child = cursor.node();
            if (child.kind() == "type_identifier" || child.kind() == "identifier") && name.is_none() {
                name = child.utf8_text(code.as_bytes()).ok().map(|s| s.to_string());
            }
            if child.kind() == "type_spec" || child.kind() == "type_identifier" {
                let mut inner = child.walk();
                if inner.goto_first_child() {
                    loop {
                        let inner_child = inner.node();
                        if (inner_child.kind() == "type_identifier" || inner_child.kind() == "identifier")
                            && let Ok(text) = inner_child.utf8_text(code.as_bytes())
                        {
                            bases.push(text.to_string());
                        }
                        if !inner.goto_next_sibling() {
                            break;
                        }
                    }
                }
            }
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
    name.map(|n| (n, bases))
}
