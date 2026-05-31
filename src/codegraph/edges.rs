use crate::ast::SymbolKind;
use std::collections::HashMap;
use tree_sitter::{Node, Parser};

/// Extract cross-reference edges from a single file.
/// Returns a list of (from_symbol_id, EdgeKind, to_symbol_name) tuples.
/// The caller (CodeGraph) resolves `to_symbol_name` into actual node IDs.
pub fn extract_edges(
    file_path: &str,
    code: &str,
    language: &str,
) -> Result<Vec<(String, super::EdgeKind, String)>, String> {
    let lang = match language.to_lowercase().as_str() {
        "python" | "py" => tree_sitter_python::language(),
        "javascript" | "js" => tree_sitter_javascript::language(),
        "typescript" | "ts" => tree_sitter_typescript::language_typescript(),
        "go" | "golang" => tree_sitter_go::language(),
        _ => return Err(format!("Unsupported language: {}", language)),
    };

    let mut parser = Parser::new();
    parser.set_language(&lang).map_err(|e| e.to_string())?;
    let tree = parser.parse(code, None).ok_or("Parse error")?;
    let root = tree.root_node();

    // Build a map of line -> symbol name for scope resolution.
    let symbol_map = build_symbol_line_map(file_path, code, language)?;

    let mut edges = Vec::new();
    match language.to_lowercase().as_str() {
        "python" | "py" => collect_edges_python(root, code, file_path, &symbol_map, &mut edges),
        "javascript" | "js" | "typescript" | "ts" => {
            collect_edges_js(root, code, file_path, &symbol_map, &mut edges)
        }
        "go" | "golang" => collect_edges_go(root, code, file_path, &symbol_map, &mut edges),
        _ => {}
    }

    Ok(edges)
}

/// Map from line number to the symbol name whose body contains that line.
fn build_symbol_line_map(
    file_path: &str,
    code: &str,
    language: &str,
) -> Result<HashMap<usize, String>, String> {
    use crate::ast::parser::AstParser;
    let parser = AstParser::for_language(language)?;
    let symbols = parser.extract_symbols(file_path, code)?;
    let mut map = HashMap::new();
    for sym in symbols {
        for line in sym.start_line..=sym.end_line {
            map.insert(line, sym.name.clone());
        }
    }
    Ok(map)
}

fn collect_edges_python(
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
            if let Some(from) = scope_symbol.clone() && let Some(callee) = extract_callee_name(node, code) {
                edges.push((format!("{}#{:?}", from, SymbolKind::Function), super::EdgeKind::Calls, callee));
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
            if let Some(base) = extract_class_bases_python(node, code) && let Some(from) = scope_symbol.clone() {
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
            collect_edges_python(cursor.node(), code, file_path, symbol_map, edges);
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
}

fn collect_edges_js(
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
            if let Some(from) = scope_symbol.clone() && let Some(callee) = extract_callee_name_js(node, code) {
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
            if let Some(bases) = extract_class_bases_js(node, code) && let Some(from) = scope_symbol.clone() {
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
            collect_edges_js(cursor.node(), code, file_path, symbol_map, edges);
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
}

fn collect_edges_go(
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
            if let Some(from) = scope_symbol.clone() && let Some(callee) = extract_callee_name_go(node, code) {
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
            collect_edges_go(cursor.node(), code, file_path, symbol_map, edges);
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
}

// ── Python helpers ──

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

// ── JS/TS helpers ──

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

// ── Go helpers ──

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
