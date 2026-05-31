use std::env;

fn main() {
    let args: Vec<String> = env::args().collect();

    // If --tui flag is passed, run the terminal UI.
    if args.iter().any(|a| a == "--tui" || a == "-t") {
        if let Err(e) = sage::tui::run::run_tui() {
            eprintln!("TUI error: {}", e);
            std::process::exit(1);
        }
        return;
    }

    // Otherwise run the CLI demo.
    cli_demo();
}

fn cli_demo() {
    use sage::agent::editor::EditorAgent;
    use sage::blob_store::BlobStore;
    use sage::diff::applicator::apply_diff;

    let store = BlobStore::new();
    let agent = EditorAgent::new();

    let file_path = "demo.rs";
    let content = "fn main() {\n    println!(\"Hello, world!\");\n}\n";

    // Store original in blob store
    let original_hash = store.put(content.as_bytes().to_vec());
    println!("Original blob hash: {}", original_hash);

    // Generate edit
    let task = "Change 'Hello, world!' to 'Hello, Sage!'";
    let diff = agent
        .generate_edit(file_path, content, task)
        .expect("Failed to generate edit");

    println!("Generated diff block:");
    println!("  old_anchor: {}", diff.old_anchor);
    println!("  new_anchor: {}", diff.new_anchor);

    // Apply diff
    let new_content = apply_diff(content, &diff).expect("Failed to apply diff");

    println!("New content:\n{}", new_content);

    // Store new version
    let new_hash = store.put(new_content.as_bytes().to_vec());
    println!("New blob hash: {}", new_hash);
}
