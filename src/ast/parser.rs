use super::{Symbol, SymbolKind};
use tree_sitter::{Language, Node, Parser};

pub struct AstParser {
    language: Language,
    #[allow(dead_code)]
    language_name: String,
}

impl AstParser {
    pub fn for_language(name: &str) -> Result<Self, String> {
        let (lang, lang_name): (Language, String) = match name.to_lowercase().as_str() {
            "python" | "py" => (
                tree_sitter_python::language(),
                "python".to_string(),
            ),
            "javascript" | "js" => (
                tree_sitter_javascript::language(),
                "javascript".to_string(),
            ),
            "typescript" | "ts" => (
                tree_sitter_typescript::language_typescript(),
                "typescript".to_string(),
            ),
            "go" | "golang" => (
                tree_sitter_go::language(),
                "go".to_string(),
            ),
            _ => return Err(format!("Unsupported language: {}", name)),
        };

        Ok(Self {
            language: lang,
            language_name: lang_name,
        })
    }

    pub fn extract_symbols(&self, file_path: &str, code: &str) -> Result<Vec<Symbol>, String> {
        let mut parser = Parser::new();
        parser.set_language(&self.language).map_err(|e| e.to_string())?;

        let tree = parser.parse(code, None).ok_or("Parse error")?;
        let root = tree.root_node();
        let mut symbols = Vec::new();
        let mut cursor = root.walk();

        Self::collect_nodes(&mut cursor, code, file_path, &mut symbols);
        Ok(symbols)
    }

    fn collect_nodes(
        cursor: &mut tree_sitter::TreeCursor,
        code: &str,
        file_path: &str,
        symbols: &mut Vec<Symbol>,
    ) {
        let node = cursor.node();
        if let Some(symbol) = Self::node_to_symbol(node, code, file_path) {
            symbols.push(symbol);
        }

        if cursor.goto_first_child() {
            loop {
                Self::collect_nodes(cursor, code, file_path, symbols);
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
            cursor.goto_parent();
        }
    }

    fn node_to_symbol(node: Node, code: &str, file_path: &str) -> Option<Symbol> {
        let kind = match node.kind() {
            "function_definition" | "function_declaration" | "func_declaration" => SymbolKind::Function,
            "class_definition" | "class_declaration" => SymbolKind::Class,
            "method_definition" => SymbolKind::Method,
            "struct_item" => SymbolKind::Struct,
            "interface_declaration" => SymbolKind::Interface,
            "type_alias" | "type_declaration" => SymbolKind::Type,
            "const_declaration" | "constant_declaration" => SymbolKind::Constant,
            _ => return None,
        };

        let name = Self::extract_name(node, code)?;
        let start_line = node.start_position().row;
        let end_line = node.end_position().row;
        let signature = Self::extract_signature(node, code);
        let docstring = Self::extract_docstring(node, code);

        Some(Symbol {
            name,
            kind,
            file_path: file_path.to_string(),
            start_line,
            end_line,
            signature,
            docstring,
        })
    }

    fn extract_name(node: Node, code: &str) -> Option<String> {
        let mut cursor = node.walk();
        if cursor.goto_first_child() {
            loop {
                let child = cursor.node();
                if child.kind() == "identifier" || child.kind() == "type_identifier" {
                    return Some(child.utf8_text(code.as_bytes()).ok()?.to_string());
                }
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
        }
        None
    }

    fn extract_signature(_node: Node, _code: &str) -> Option<String> {
        None
    }

    fn extract_docstring(node: Node, code: &str) -> Option<String> {
        let mut cursor = node.walk();
        if cursor.goto_first_child() {
            loop {
                let child = cursor.node();
                if child.kind().contains("string") || child.kind() == "expression_statement" {
                    let text = child.utf8_text(code.as_bytes()).ok()?;
                    if text.trim().starts_with('"') || text.trim().starts_with('\'') {
                        return Some(text.trim().to_string());
                    }
                }
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
        }
        None
    }
}
