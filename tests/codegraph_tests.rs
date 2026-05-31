use sage::ast::{Symbol, SymbolKind};
use sage::codegraph::{
    reciprocal_rank_fusion, CodeGraph, ContextAssembler, EdgeKind, EmbeddingProvider, KeywordMatcher,
    MockEmbeddingProvider, RepoMap, VectorIndex,
};
use std::collections::HashMap;

// ── CodeGraph basics ──

#[test]
fn test_graph_add_remove_node() {
    let mut graph = CodeGraph::new();
    let sym = Symbol {
        name: "foo".to_string(),
        kind: SymbolKind::Function,
        file_path: "src/lib.rs".to_string(),
        start_line: 10,
        end_line: 20,
        signature: Some("fn foo() -> i32".to_string()),
        docstring: None,
    };
    let id = graph.add_node(sym.clone());
    assert!(graph.get_node(&id).is_some());
    assert_eq!(graph.get_node(&id).unwrap().symbol.name, "foo");

    graph.remove_node(&id);
    assert!(graph.get_node(&id).is_none());
}

#[test]
fn test_graph_edges() {
    let mut graph = CodeGraph::new();
    let a = graph.add_node(Symbol {
        name: "a".to_string(),
        kind: SymbolKind::Function,
        file_path: "a.rs".to_string(),
        start_line: 1,
        end_line: 2,
        signature: None,
        docstring: None,
    });
    let b = graph.add_node(Symbol {
        name: "b".to_string(),
        kind: SymbolKind::Function,
        file_path: "b.rs".to_string(),
        start_line: 1,
        end_line: 2,
        signature: None,
        docstring: None,
    });
    graph.add_edge(&a, &b, EdgeKind::Calls, 1.0);

    assert_eq!(graph.get_outgoing(&a).len(), 1);
    assert_eq!(graph.get_incoming(&b).len(), 1);
}

#[test]
fn test_graph_remove_file_nodes() {
    let mut graph = CodeGraph::new();
    let id1 = graph.add_node(Symbol {
        name: "foo".to_string(),
        kind: SymbolKind::Function,
        file_path: "a.py".to_string(),
        start_line: 1,
        end_line: 2,
        signature: None,
        docstring: None,
    });
    let id2 = graph.add_node(Symbol {
        name: "bar".to_string(),
        kind: SymbolKind::Function,
        file_path: "b.py".to_string(),
        start_line: 1,
        end_line: 2,
        signature: None,
        docstring: None,
    });
    graph.remove_file_nodes("a.py");
    assert!(graph.get_node(&id1).is_none());
    assert!(graph.get_node(&id2).is_some());
}

#[test]
fn test_page_rank_convergence() {
    let mut graph = CodeGraph::new();
    // Build a simple chain: a -> b -> c
    let a = graph.add_node(Symbol {
        name: "a".to_string(),
        kind: SymbolKind::Function,
        file_path: "x.rs".to_string(),
        start_line: 1,
        end_line: 2,
        signature: None,
        docstring: None,
    });
    let b = graph.add_node(Symbol {
        name: "b".to_string(),
        kind: SymbolKind::Function,
        file_path: "x.rs".to_string(),
        start_line: 3,
        end_line: 4,
        signature: None,
        docstring: None,
    });
    let c = graph.add_node(Symbol {
        name: "c".to_string(),
        kind: SymbolKind::Function,
        file_path: "x.rs".to_string(),
        start_line: 5,
        end_line: 6,
        signature: None,
        docstring: None,
    });
    graph.add_edge(&a, &b, EdgeKind::Calls, 1.0);
    graph.add_edge(&b, &c, EdgeKind::Calls, 1.0);

    let ranks = graph.page_rank(&[], 0.85, 1e-6, 100);
    // c has no outgoing edges so it leaks rank; in a chain a->b->c,
    // c typically ends up with the highest rank because it accumulates
    // from b while losing nothing.
    let rank_a = *ranks.get(&a).unwrap();
    let rank_b = *ranks.get(&b).unwrap();
    let rank_c = *ranks.get(&c).unwrap();
    assert!(rank_c > rank_b, "c should rank higher than b");
    assert!(rank_b > rank_a, "b should rank higher than a");
}

#[test]
fn test_page_rank_with_seeds() {
    let mut graph = CodeGraph::new();
    let a = graph.add_node(Symbol {
        name: "a".to_string(),
        kind: SymbolKind::Function,
        file_path: "x.rs".to_string(),
        start_line: 1,
        end_line: 2,
        signature: None,
        docstring: None,
    });
    let b = graph.add_node(Symbol {
        name: "b".to_string(),
        kind: SymbolKind::Function,
        file_path: "x.rs".to_string(),
        start_line: 3,
        end_line: 4,
        signature: None,
        docstring: None,
    });
    let c = graph.add_node(Symbol {
        name: "c".to_string(),
        kind: SymbolKind::Function,
        file_path: "x.rs".to_string(),
        start_line: 5,
        end_line: 6,
        signature: None,
        docstring: None,
    });
    graph.add_edge(&a, &b, EdgeKind::Calls, 1.0);
    graph.add_edge(&b, &c, EdgeKind::Calls, 1.0);

    // Seed "a" should boost "a" and its downstream nodes.
    let ranks = graph.page_rank(&[a.clone()], 0.85, 1e-6, 100);
    let rank_a = *ranks.get(&a).unwrap();
    let rank_b = *ranks.get(&b).unwrap();
    let rank_c = *ranks.get(&c).unwrap();
    // With seed=a, a should get a substantial share of the teleport mass.
    assert!(rank_a > rank_c * 0.5, "seeded node a should have significant rank");
    assert!(rank_b > 0.0);
    assert!(rank_c > 0.0);
}

// ── Edge extraction ──

#[test]
fn test_extract_edges_python() {
    let code = r#"
def foo():
    bar()

class Baz(Qux):
    pass
"#;
    let edges = sage::codegraph::extract_edges("test.py", code, "python").unwrap();
    // Should find a call edge from foo to bar and an inherit edge from Baz to Qux.
    let calls: Vec<_> = edges
        .iter()
        .filter(|(_, kind, _)| matches!(kind, EdgeKind::Calls))
        .collect();
    let inherits: Vec<_> = edges
        .iter()
        .filter(|(_, kind, _)| matches!(kind, EdgeKind::InheritsFrom))
        .collect();
    assert!(
        calls.iter().any(|(_, _, target)| target == "bar"),
        "should find call to bar"
    );
    assert!(
        inherits.iter().any(|(_, _, target)| target == "Qux"),
        "should find inheritance from Qux"
    );
}

#[test]
fn test_extract_edges_javascript() {
    let code = r#"
function greet() {
    console.log("hi");
}
class Dog extends Animal {
    bark() {
        greet();
    }
}
"#;
    let edges = sage::codegraph::extract_edges("test.js", code, "javascript").unwrap();
    let calls: Vec<_> = edges
        .iter()
        .filter(|(_, kind, _)| matches!(kind, EdgeKind::Calls))
        .collect();
    let inherits: Vec<_> = edges
        .iter()
        .filter(|(_, kind, _)| matches!(kind, EdgeKind::InheritsFrom))
        .collect();
    assert!(
        calls.iter().any(|(_, _, target)| target.contains("console")),
        "should find call to console.log"
    );
    assert!(
        inherits.iter().any(|(_, _, target)| target == "Animal"),
        "should find inheritance from Animal"
    );
}

#[test]
fn test_extract_edges_go() {
    let code = r#"
package main
import "fmt"
func hello() {
    fmt.Println("hello")
}
"#;
    let edges = sage::codegraph::extract_edges("test.go", code, "go").unwrap();
    let calls: Vec<_> = edges
        .iter()
        .filter(|(_, kind, _)| matches!(kind, EdgeKind::Calls))
        .collect();
    let imports: Vec<_> = edges
        .iter()
        .filter(|(_, kind, _)| matches!(kind, EdgeKind::Imports))
        .collect();
    assert!(
        calls.iter().any(|(_, _, target)| target.contains("fmt")),
        "should find call to fmt.Println"
    );
    assert!(
        imports.iter().any(|(_, _, target)| target.contains("fmt")),
        "should find import of fmt"
    );
}

// ── Retrieval components ──

#[test]
fn test_vector_index_search() {
    let mut index = VectorIndex::new();
    let provider = MockEmbeddingProvider::new(8);
    let emb_a = provider.embed("fn alpha").unwrap();
    let emb_b = provider.embed("fn beta").unwrap();
    let emb_c = provider.embed("struct Gamma").unwrap();

    index.add("a".to_string(), emb_a, "fn alpha".to_string());
    index.add("b".to_string(), emb_b, "fn beta".to_string());
    index.add("c".to_string(), emb_c, "struct Gamma".to_string());

    let query = provider.embed("fn alpha").unwrap();
    let results = index.search(&query, 2);
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].0, "a");
    // a should score higher than b and c.
    assert!(results[0].1 > results[1].1);
}

#[test]
fn test_keyword_matcher() {
    let symbols = vec![
        Symbol {
            name: "calculate_total".to_string(),
            kind: SymbolKind::Function,
            file_path: "cart.rs".to_string(),
            start_line: 1,
            end_line: 5,
            signature: Some("fn calculate_total(items: &[Item]) -> f64".to_string()),
            docstring: None,
        },
        Symbol {
            name: "process_payment".to_string(),
            kind: SymbolKind::Function,
            file_path: "payment.rs".to_string(),
            start_line: 1,
            end_line: 5,
            signature: None,
            docstring: None,
        },
    ];
    let scores = KeywordMatcher::score(&symbols, "calculate total price");
    assert!(!scores.is_empty());
    assert!(scores.iter().any(|(id, _)| id.contains("calculate_total")));
}

#[test]
fn test_rrf_fusion() {
    let list_a = vec![
        ("a".to_string(), 1.0),
        ("b".to_string(), 0.8),
    ];
    let list_b = vec![
        ("b".to_string(), 0.9),
        ("c".to_string(), 0.7),
    ];
    let fused = reciprocal_rank_fusion(&[list_a, list_b], 60.0);
    // b appears in both lists, so it should have the highest fused score.
    let score_b = fused.get("b").unwrap();
    let score_a = fused.get("a").unwrap();
    let score_c = fused.get("c").unwrap();
    assert!(score_b > score_a);
    assert!(score_b > score_c);
}

// ── Context Assembler ──

#[test]
fn test_context_assembler_query_and_assemble() {
    let mut graph = CodeGraph::new();
    let id1 = graph.add_node(Symbol {
        name: "process_order".to_string(),
        kind: SymbolKind::Function,
        file_path: "src/orders.rs".to_string(),
        start_line: 10,
        end_line: 30,
        signature: Some("fn process_order(order: &Order) -> Result<(), Error>".to_string()),
        docstring: Some("Process a customer order.".to_string()),
    });
    let id2 = graph.add_node(Symbol {
        name: "validate_payment".to_string(),
        kind: SymbolKind::Function,
        file_path: "src/payment.rs".to_string(),
        start_line: 5,
        end_line: 15,
        signature: Some("fn validate_payment(card: &str) -> bool".to_string()),
        docstring: None,
    });
    graph.add_edge(&id1, &id2, EdgeKind::Calls, 1.0);

    let provider = MockEmbeddingProvider::new(16);
    let mut assembler = ContextAssembler::new();
    assembler.index_graph(&graph, &provider);

    let results = assembler
        .query(&graph, "process order payment", &provider, 5)
        .unwrap();
    assert!(!results.is_empty());
    // process_order and validate_payment should both appear.
    assert!(results.iter().any(|(id, _)| id == &id1));
    assert!(results.iter().any(|(id, _)| id == &id2));

    let context = assembler.assemble_context(&graph, &results, &HashMap::new(), 500);
    assert!(context.contains("process_order"));
}

// ── RepoMap ──

#[test]
fn test_repo_map_generation() {
    let mut graph = CodeGraph::new();
    graph.add_node(Symbol {
        name: "main".to_string(),
        kind: SymbolKind::Function,
        file_path: "src/main.rs".to_string(),
        start_line: 1,
        end_line: 3,
        signature: Some("fn main()".to_string()),
        docstring: None,
    });
    graph.add_node(Symbol {
        name: "helper".to_string(),
        kind: SymbolKind::Function,
        file_path: "src/lib.rs".to_string(),
        start_line: 5,
        end_line: 8,
        signature: Some("fn helper() -> i32".to_string()),
        docstring: None,
    });

    let map = RepoMap::generate(&graph, 5, None);
    assert!(map.contains("src/main.rs"));
    assert!(map.contains("src/lib.rs"));
    assert!(map.contains("main"));
    assert!(map.contains("helper"));
}
