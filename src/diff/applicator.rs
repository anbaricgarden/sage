use super::format::{DiffError, DiffResult, EditBlock};

pub fn apply_diff(content: &str, block: &EditBlock) -> DiffResult<String> {
    let lines: Vec<&str> = content.lines().collect();
    let context_sizes = [(3, 3), (5, 5), (10, 10), (20, 20)];

    for (above, below) in &context_sizes {
        match try_apply_with_context(content, &lines, block, *above, *below) {
            Ok(result) => return Ok(result),
            Err(DiffError::AmbiguousAnchor { .. }) => continue,
            Err(e) => return Err(e),
        }
    }

    Err(DiffError::ContextCollision {
        anchor: block.old_anchor.clone(),
    })
}

fn try_apply_with_context(
    content: &str,
    lines: &[&str],
    block: &EditBlock,
    _above: usize,
    _below: usize,
) -> DiffResult<String> {
    let target_len = block.old_lines.len();
    if target_len == 0 {
        return Err(DiffError::AnchorNotFound {
            anchor: block.old_anchor.clone(),
            file_path: block.file_path.clone(),
        });
    }

    let mut matches = Vec::new();

    for i in 0..lines.len() {
        let end = (i + target_len).min(lines.len());
        if end - i != target_len {
            continue;
        }
        let candidate: Vec<String> = lines[i..end].iter().map(|s| s.to_string()).collect();
        let hash = super::format::compute_context_hash(&block.file_path, &candidate);
        if hash.starts_with(&block.old_anchor) {
            matches.push(i);
        }
    }

    match matches.len() {
        0 => Err(DiffError::AnchorNotFound {
            anchor: block.old_anchor.clone(),
            file_path: block.file_path.clone(),
        }),
        1 => {
            let idx = matches[0];
            let mut result_lines: Vec<String> = lines[..idx].iter().map(|s| s.to_string()).collect();
            result_lines.extend(block.new_lines.clone());
            if idx + target_len < lines.len() {
                result_lines.extend(lines[idx + target_len..].iter().map(|s| s.to_string()));
            }
            let has_trailing_newline = !lines.is_empty() && content.ends_with('\n');
            let mut result = result_lines.join("\n");
            if has_trailing_newline {
                result.push('\n');
            }
            Ok(result)
        }
        _ => Err(DiffError::AmbiguousAnchor {
            anchor: block.old_anchor.clone(),
            matches: matches.len(),
        }),
    }
}
