use std::collections::HashMap;
use tree_sitter::Parser;

use crate::ast::parser::AstParser;
use crate::codegraph::language_parser::ParserRegistry;

/// Extract cross-reference edges from a single file.
/// Returns a list of (from_symbol_id, EdgeKind, to_symbol_name) tuples.
/// The caller (CodeGraph) resolves `to_symbol_name` into actual node IDs.
pub fn extract_edges(
    file_path: &str,
    code: &str,
    language: &str,
) -> Result<Vec<(String, super::EdgeKind, String)>, String> {
    let registry = ParserRegistry::new();
    let parser_adapter = registry
        .get(language)
        .ok_or_else(|| format!("Unsupported language: {}", language))?;

    let mut parser = Parser::new();
    parser
        .set_language(&parser_adapter.tree_sitter_language())
        .map_err(|e| e.to_string())?;
    let tree = parser.parse(code, None).ok_or("Parse error")?;
    let root = tree.root_node();

    // Build a map of line -> symbol name for scope resolution.
    let symbol_map = build_symbol_line_map(file_path, code, language)?;

    Ok(parser_adapter.collect_edges(root, code, file_path, &symbol_map))
}

/// Map from line number to the symbol name whose body contains that line.
fn build_symbol_line_map(
    file_path: &str,
    code: &str,
    language: &str,
) -> Result<HashMap<usize, String>, String> {
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
