use sage::blob_store::BlobStore;
use sage::diff::format::EditBlock;
use sage::diff::applicator::apply_diff;
use sage::diff::parser::parse_diff;
use sage::ast::parser::AstParser;
use sage::ast::SymbolKind;
use sage::agent::editor::EditorAgent;

// ---------------------------------------------------------------------------
// Task 2: Blob Store
// ---------------------------------------------------------------------------

#[test]
fn test_blob_store_round_trip() {
    let store = BlobStore::new();
    let content = b"fn main() { println!(\"hello\"); }";
    let hash = store.put(content.to_vec());
    assert_eq!(hash.len(), 64); // SHA-256 hex = 64 chars
    let retrieved = store.get(&hash).unwrap();
    assert_eq!(retrieved, content);
}

#[test]
fn test_blob_store_deduplication() {
    let store = BlobStore::new();
    let content = b"duplicate content";
    let hash1 = store.put(content.to_vec());
    let hash2 = store.put(content.to_vec());
    assert_eq!(hash1, hash2);
}

// ---------------------------------------------------------------------------
// Task 3: Hash-Anchored Diff Format
// ---------------------------------------------------------------------------

#[test]
fn test_edit_block_anchor_hash_computation() {
    let file_content = "line1\nline2\nline3\nline4\nline5\nline6\nline7\n";
    let block = EditBlock::compute_anchor(
        "src/main.rs",
        file_content,
        3, // target line index (0-based, so line4)
        3, // lines above
        3, // lines below
    );
    assert_eq!(block.old_anchor.len(), 8);
    assert_eq!(block.new_anchor.len(), 8);
    assert!(block.old_anchor != block.new_anchor || block.old_lines == block.new_lines);
}

// ---------------------------------------------------------------------------
// Task 4: Diff Parser
// ---------------------------------------------------------------------------

#[test]
fn test_parse_single_diff_block() {
    let diff_text = r#"<<<<<<< HEAD:abc12345
fn old_func() {
    println!("old");
}
=======
fn new_func() {
    println!("new");
}
>>>>>>> def67890"#;

    let blocks = parse_diff(diff_text, "src/main.rs").unwrap();
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].old_anchor, "abc12345");
    assert_eq!(blocks[0].new_anchor, "def67890");
    assert_eq!(blocks[0].old_lines.len(), 3);
    assert_eq!(blocks[0].new_lines.len(), 3);
    assert_eq!(blocks[0].old_lines[0], "fn old_func() {");
    assert_eq!(blocks[0].new_lines[0], "fn new_func() {");
}

#[test]
fn test_parse_multiple_diff_blocks() {
    let diff_text = r#"<<<<<<< HEAD:abc12345
line1
=======
line1_modified
>>>>>>> def67890
<<<<<<< HEAD:aabbccdd
line2
=======
line2_modified
>>>>>>> 11223344"#;

    let blocks = parse_diff(diff_text, "src/main.rs").unwrap();
    assert_eq!(blocks.len(), 2);
    assert_eq!(blocks[0].old_anchor, "abc12345");
    assert_eq!(blocks[1].old_anchor, "aabbccdd");
}

// ---------------------------------------------------------------------------
// Task 5: Diff Applicator
// ---------------------------------------------------------------------------

#[test]
fn test_apply_single_line_change() {
    let content = "line1\nline2\nline3\nline4\nline5\n";
    let mut block = EditBlock::compute_anchor("src/main.rs", content, 2, 3, 3);
    // Replace the middle line (line3) with "line3_modified"
    block.new_lines = vec![
        "line1".to_string(),
        "line2".to_string(),
        "line3_modified".to_string(),
        "line4".to_string(),
        "line5".to_string(),
    ];
    block.recompute_new_anchor();

    let result = apply_diff(content, &block).unwrap();
    assert_eq!(result, "line1\nline2\nline3_modified\nline4\nline5\n");
}

#[test]
fn test_apply_adds_new_lines() {
    let content = "line1\nline2\nline3\n";
    let mut block = EditBlock::compute_anchor("src/main.rs", content, 1, 1, 1);
    block.new_lines = vec![
        "line1".to_string(),
        "line1.5".to_string(),
        "line2".to_string(),
        "line3".to_string(),
    ];
    block.recompute_new_anchor();

    let result = apply_diff(content, &block).unwrap();
    assert_eq!(result, "line1\nline1.5\nline2\nline3\n");
}

#[test]
fn test_apply_ambiguous_anchor_expands_context() {
    // Two identical line2 occurrences
    let content = "line1\nline2\nline3\nline1\nline2\nline3\n";
    let mut block = EditBlock::compute_anchor("src/main.rs", content, 1, 1, 1);
    block.new_lines = vec![
        "line1".to_string(),
        "line2_modified".to_string(),
        "line3".to_string(),
    ];
    block.recompute_new_anchor();

    // With only 1-line context, this should be ambiguous (two matches).
    // The applicator should expand context and succeed.
    let result = apply_diff(content, &block).unwrap();
    // Should match the first occurrence because context expansion makes it unique
    assert!(result.contains("line2_modified"));
}

#[test]
fn test_apply_preserves_no_trailing_newline() {
    let content = "line1\nline2\nline3"; // no trailing newline
    let mut block = EditBlock::compute_anchor("src/main.rs", content, 1, 1, 1);
    block.new_lines = vec![
        "line1".to_string(),
        "line2_modified".to_string(),
        "line3".to_string(),
    ];
    block.recompute_new_anchor();

    let result = apply_diff(content, &block).unwrap();
    assert!(!result.ends_with('\n'));
    assert_eq!(result, "line1\nline2_modified\nline3");
}

// ---------------------------------------------------------------------------
// Task 6: AST Parser
// ---------------------------------------------------------------------------

#[test]
fn test_parse_python_function() {
    let parser = AstParser::for_language("python").unwrap();
    let code = r#"def calculate_sum(a, b):
    return a + b
"#;
    let symbols = parser.extract_symbols("test.py", code).unwrap();
    assert_eq!(symbols.len(), 1);
    assert_eq!(symbols[0].name, "calculate_sum");
    assert_eq!(symbols[0].kind, SymbolKind::Function);
    assert_eq!(symbols[0].file_path, "test.py");
}

#[test]
fn test_parse_python_class_and_method() {
    let parser = AstParser::for_language("python").unwrap();
    let code = r#"class Calculator:
    def add(self, a, b):
        return a + b
"#;
    let symbols = parser.extract_symbols("test.py", code).unwrap();
    let names: Vec<_> = symbols.iter().map(|s| s.name.clone()).collect();
    assert!(names.contains(&"Calculator".to_string()));
    assert!(names.contains(&"add".to_string()));
}

#[test]
fn test_parse_javascript_function() {
    let parser = AstParser::for_language("javascript").unwrap();
    let code = r#"function greet(name) {
    return "Hello, " + name;
}"#;
    let symbols = parser.extract_symbols("test.js", code).unwrap();
    let names: Vec<_> = symbols.iter().map(|s| s.name.clone()).collect();
    assert!(names.contains(&"greet".to_string()));
}

// ---------------------------------------------------------------------------
// Task 7: Editor Agent
// ---------------------------------------------------------------------------

#[test]
fn test_editor_agent_generates_diff_for_single_line_fix() {
    let agent = EditorAgent::new();
    let file_path = "src/main.rs";
    let content = "fn main() {\n    println!(\"Hello, world!\");\n}\n";
    let task = "Change the greeting to 'Hello, Sage!'";

    let diff = agent.generate_edit(file_path, content, task).unwrap();
    let applied = sage::diff::applicator::apply_diff(content, &diff).unwrap();
    assert!(applied.contains("Hello, Sage!"));
    assert!(!applied.contains("Hello, world!"));
}

#[test]
fn test_editor_agent_generates_diff_for_adding_a_line() {
    let agent = EditorAgent::new();
    let file_path = "src/main.rs";
    let content = "fn main() {\n    println!(\"Hello\");\n}\n";
    let task = "Add a second println for 'Goodbye' after the first one";

    let diff = agent.generate_edit(file_path, content, task).unwrap();
    let applied = sage::diff::applicator::apply_diff(content, &diff).unwrap();
    assert!(applied.contains("Goodbye"));
}
