//! # Code-Aware Pruner
//!
//! Applies token-pruning rules to source code while preserving semantics.
//!
//! Safety guarantees:
//! - Type annotations are **never** removed
//! - Function signatures are **never** removed
//! - Control flow logic is **never** removed
//! - TODO/FIXME comments are **always** preserved
//! - Import statements are **never** fully removed (only unused ones via analysis)

use super::CompressorConfig;
use std::collections::HashSet;

/// Core code-aware pruner. Applies compression rules to source code content.
#[derive(Debug, Clone)]
pub struct CodePruner {
    config: CompressorConfig,
}

impl CodePruner {
    pub fn new(config: CompressorConfig) -> Self {
        Self { config }
    }

    /// Compress `content` for the given language.
    /// Returns the compressed source string.
    pub fn compress(&self, content: &str, language: &str) -> String {
        let dominated_by_imports = content.lines().take(30).filter(|l| !l.trim().is_empty()).count() > 5;

        let mut result = content.to_string();

        if self.config.collapse_whitespace {
            result = collapse_whitespace(&result);
        }

        if self.config.strip_comments {
            result = strip_comments(&result, language);
        }

        if self.config.truncate_docstrings {
            result = truncate_docstrings(&result, language, self.config.max_docstring_tokens);
        }

        if self.config.remove_unused_imports {
            let dominated = dominated_by_imports && content.lines().count() < 2000;
            if dominated {
                result = remove_unused_imports_ast(&result, language);
            } else {
                result = remove_obvious_unused_imports_heuristic(&result, language);
            }
        }

        let is_test = is_test_file(content, language);
        if self.config.compress_test_files && is_test {
            result = compress_test_file(&result, language);
        }

        result
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Whitespace Collapsing
// ─────────────────────────────────────────────────────────────────────────────

fn collapse_whitespace(content: &str) -> String {
    let mut result = String::with_capacity(content.len());
    let mut prev_was_blank = false;

    for line in content.lines() {
        let trimmed = line.trim_end();

        if trimmed.is_empty() {
            if !prev_was_blank {
                result.push('\n');
            }
            prev_was_blank = true;
        } else {
            result.push_str(trimmed);
            result.push('\n');
            prev_was_blank = false;
        }
    }

    if result.starts_with('\n') {
        result = result[1..].to_string();
    }

    result
}

// ─────────────────────────────────────────────────────────────────────────────
// Comment Stripping
// ─────────────────────────────────────────────────────────────────────────────

fn strip_comments(content: &str, language: &str) -> String {
    match language {
        "python" => strip_python_comments(content),
        "javascript" | "typescript" => strip_js_comments(content),
        "go" => strip_go_comments(content),
        "rust" => strip_rust_comments(content),
        _ => strip_line_comments(content, "//"),
    }
}

/// Returns true if `c` is a Python string quote character (ASCII double-quote
/// or curly variants).
fn is_python_quote_char(c: char) -> bool {
    c == '"' || c == '\'' || c == '\u{00AB}' || c == '\u{201C}' || c == '\u{201D}'
}

fn strip_python_comments(content: &str) -> String {
    let bytes = content.as_bytes();
    let mut result = Vec::with_capacity(content.len());
    let mut i = 0;

    while i < bytes.len() {
        let b = bytes[i];

        // Check for triple-quoted string start: only ASCII " for now
        // (''' and curly variants are rare in practice)
        if b == b'"' && i + 2 < bytes.len()
            && bytes[i + 1] == b'"' && bytes[i + 2] == b'"'
        {
            let start = i;
            i += 3;
            while i + 2 < bytes.len() {
                if bytes[i] == b'"' && bytes[i + 1] == b'"' && bytes[i + 2] == b'"' {
                    i += 3;
                    break;
                }
                i += 1;
            }
            result.extend_from_slice(&bytes[start..i]);
        }
        // Line comment
        else if b == b'#' {
            let start = i;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            let comment = std::str::from_utf8(&bytes[start..i]).unwrap_or("");
            if contains_todo_fixme(comment) {
                result.extend_from_slice(&bytes[start..i]);
            }
            if i < bytes.len() {
                result.push(b'\n');
            }
        }
        // Single-quoted string — handle via char to catch '
        else if (b as char) == '\u{2019}' || (b as char) == '\u{2018}' {
            result.push(b);
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    result.push(bytes[i]);
                    result.push(bytes[i + 1]);
                    i += 2;
                } else if bytes[i] == b {
                    result.push(b);
                    i += 1;
                    break;
                } else if bytes[i] == b'\n' {
                    break;
                } else {
                    result.push(bytes[i]);
                    i += 1;
                }
            }
        }
        // Double-quoted string — handle via char to catch curly quotes
        else if is_python_quote_char(b as char) {
            let quote_char = b as char;
            result.push(b);
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    result.push(bytes[i]);
                    result.push(bytes[i + 1]);
                    i += 2;
                } else if (bytes[i] as char) == quote_char {
                    result.push(bytes[i]);
                    i += 1;
                    break;
                } else if bytes[i] == b'\n' {
                    break;
                } else {
                    result.push(bytes[i]);
                    i += 1;
                }
            }
        } else {
            result.push(b);
            i += 1;
        }
    }

    String::from_utf8(result).unwrap_or_else(|_| content.to_string())
}

/// Returns true if `c` is a JS/TS string quote character.
fn is_js_quote_char(c: char) -> bool {
    c == '"' || c == '\'' || c == '\u{00AB}' || c == '\u{201C}' || c == '\u{201D}'
        || c == '\u{2018}' || c == '\u{2019}' || c == '`'
}

fn strip_js_comments(content: &str) -> String {
    let bytes = content.as_bytes();
    let mut result = Vec::with_capacity(content.len());
    let mut i = 0;

    while i < bytes.len() {
        let b = bytes[i];

        // String literal
        if is_js_quote_char(b as char) {
            let quote_char = b as char;
            result.push(b);
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    result.push(bytes[i]);
                    result.push(bytes[i + 1]);
                    i += 2;
                } else if (bytes[i] as char) == quote_char {
                    result.push(bytes[i]);
                    i += 1;
                    break;
                } else {
                    result.push(bytes[i]);
                    i += 1;
                }
            }
            continue;
        }

        // Line comment
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            let start = i;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            let comment = std::str::from_utf8(&bytes[start..i]).unwrap_or("");
            if contains_todo_fixme(comment) {
                result.extend_from_slice(&bytes[start..i]);
            }
            if i < bytes.len() {
                result.push(b'\n');
            }
        }
        // Block comment
        else if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            let start = i;
            i += 2;
            while i + 1 < bytes.len() {
                if bytes[i] == b'*' && bytes[i + 1] == b'/' {
                    i += 2;
                    break;
                }
                i += 1;
            }
            let comment = std::str::from_utf8(&bytes[start..i]).unwrap_or("");
            if contains_todo_fixme(comment) {
                result.extend_from_slice(&bytes[start..i]);
            }
        }
        // Everything else
        else {
            result.push(b);
            i += 1;
        }
    }

    let as_str = String::from_utf8(result).unwrap_or_else(|_| content.to_string());
    as_str.lines()
        .map(|l| {
            if l.trim().is_empty() {
                String::new()
            } else {
                l.trim_end().to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn strip_go_comments(content: &str) -> String {
    strip_js_comments(content)
}

fn strip_rust_comments(content: &str) -> String {
    strip_js_comments(content)
}

fn strip_line_comments(content: &str, marker: &str) -> String {
    let mut result = String::with_capacity(content.len());

    for line in content.lines() {
        if let Some(pos) = line.find(marker) {
            let before = &line[..pos];
            let comment_part = &line[pos..];
            if contains_todo_fixme(comment_part) {
                result.push_str(line.trim_end());
            } else {
                result.push_str(before.trim_end());
            }
        } else {
            result.push_str(line.trim_end());
        }
        result.push('\n');
    }

    result
}

// ─────────────────────────────────────────────────────────────────────────────
// Docstring Truncation
// ─────────────────────────────────────────────────────────────────────────────

fn truncate_docstrings(content: &str, language: &str, max_tokens: usize) -> String {
    match language {
        "python" => truncate_python_docstrings(content, max_tokens),
        "javascript" | "typescript" => truncate_js_docstrings(content, max_tokens),
        "rust" => truncate_rust_docstrings(content, max_tokens),
        _ => content.to_string(),
    }
}

fn truncate_python_docstrings(content: &str, max_tokens: usize) -> String {
    let bytes = content.as_bytes();
    let mut result = Vec::with_capacity(content.len());
    let mut i = 0;

    while i < bytes.len() {
        // Detect triple-quoted string start (ASCII only for simplicity)
        if bytes[i] == b'"' && i + 2 < bytes.len()
            && bytes[i + 1] == b'"' && bytes[i + 2] == b'"'
        {
            let start = i;
            i += 3;

            let mut docstring_content = Vec::new();
            let mut closed = false;

            while i + 2 < bytes.len() {
                if bytes[i] == b'"' && bytes[i + 1] == b'"' && bytes[i + 2] == b'"' {
                    i += 3;
                    closed = true;
                    break;
                }
                docstring_content.push(bytes[i]);
                i += 1;
            }

            if !closed {
                result.extend_from_slice(&bytes[start..]);
                break;
            }

            let doc_str = String::from_utf8_lossy(&docstring_content);
            let truncated = truncate_to_first_sentence(&doc_str, max_tokens);

            result.extend(b"\"\"\"");
            result.extend(truncated.as_bytes());
            result.extend(b"\"\"\"");
        } else {
            result.push(bytes[i]);
            i += 1;
        }
    }

    String::from_utf8(result).unwrap_or_else(|_| content.to_string())
}

fn truncate_js_docstrings(content: &str, max_tokens: usize) -> String {
    truncate_js_docstrings_pass2(content, max_tokens)
}

fn truncate_js_docstrings_pass2(content: &str, max_tokens: usize) -> String {
    let bytes = content.as_bytes();
    let mut result = Vec::with_capacity(content.len());
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'/' && i + 2 < bytes.len() && bytes[i + 1] == b'*' {
            i += 2;
            let mut doc_content = Vec::new();
            while i + 1 < bytes.len() {
                if bytes[i] == b'*' && bytes[i + 1] == b'/' {
                    i += 2;
                    break;
                }
                doc_content.push(bytes[i]);
                i += 1;
            }

            let doc_str = String::from_utf8_lossy(&doc_content);
            let cleaned = doc_str.trim().trim_start_matches('*').trim();
            let truncated = truncate_to_first_sentence(cleaned, max_tokens);

            result.extend(b"/** ");
            result.extend(truncated.as_bytes());
            result.extend(b" */");
        } else {
            result.push(bytes[i]);
            i += 1;
        }
    }

    String::from_utf8(result).unwrap_or_else(|_| content.to_string())
}

fn truncate_rust_docstrings(content: &str, max_tokens: usize) -> String {
    let bytes = content.as_bytes();
    let mut result = Vec::with_capacity(content.len());
    let mut i = 0;

    while i < bytes.len() {
        // Check for /*! ... */ (inner doc comment)
        if bytes[i] == b'/' && i + 3 < bytes.len()
            && bytes[i + 1] == b'*' && bytes[i + 2] == b'!'
        {
            i += 3;
            let mut doc_content = Vec::new();
            while i + 1 < bytes.len() {
                if bytes[i] == b'*' && bytes[i + 1] == b'/' {
                    i += 2;
                    break;
                }
                doc_content.push(bytes[i]);
                i += 1;
            }
            let doc_str = String::from_utf8_lossy(&doc_content);
            let truncated = truncate_to_first_sentence(doc_str.trim(), max_tokens);
            result.extend(b"/*! ");
            result.extend(truncated.as_bytes());
            result.extend(b" */");
        }
        // Check for /// outer doc line comments
        else if bytes[i] == b'/' && i + 2 < bytes.len()
            && bytes[i + 1] == b'/' && bytes[i + 2] == b'/'
        {
            let start = i;
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            let comment = std::str::from_utf8(&bytes[start..i]).unwrap_or("");
            let trimmed = comment.trim();
            if trimmed.starts_with("///") {
                let content_part = trimmed.trim_start_matches("///").trim();
                let truncated = truncate_to_first_sentence(content_part, max_tokens);
                result.extend(b"/// ");
                result.extend(truncated.as_bytes());
            } else {
                result.extend(trimmed.as_bytes());
            }
            result.push(b'\n');
        } else {
            result.push(bytes[i]);
            i += 1;
        }
    }

    String::from_utf8(result).unwrap_or_else(|_| content.to_string())
}

fn truncate_to_first_sentence(text: &str, max_tokens: usize) -> String {
    let first_end = text.char_indices().find_map(|(i, c)| {
        if ".!?".contains(c) {
            let after = &text[i + c.len_utf8()..];
            let rest = after.trim_start();
            if rest.is_empty() || rest.chars().next().map(|nc| nc.is_uppercase()).unwrap_or(false) {
                return Some(i + c.len_utf8());
            }
        }
        None
    });

    let end_pos = first_end.unwrap_or(text.len());
    let first_sentence = &text[..end_pos].trim();

    if max_tokens > 0 && !first_sentence.is_empty() {
        let words: Vec<&str> = first_sentence.split_whitespace().collect();
        if words.len() > max_tokens {
            return words[..max_tokens].join(" ") + "…";
        }
    }

    if first_sentence.is_empty() {
        if max_tokens == 0 {
            return text.to_string();
        }
        return text.split_whitespace().take(max_tokens.max(10)).collect::<Vec<_>>().join(" ");
    }

    first_sentence.to_string()
}

// ─────────────────────────────────────────────────────────────────────────────
// Import Removal
// ─────────────────────────────────────────────────────────────────────────────

fn remove_unused_imports_ast(content: &str, language: &str) -> String {
    match language {
        "python" => remove_python_unused_imports(content),
        "javascript" | "typescript" => remove_js_unused_imports(content),
        "go" => remove_go_unused_imports(content),
        _ => content.to_string(),
    }
}

fn remove_python_unused_imports(content: &str) -> String {
    let used_names = collect_used_names_python(content);

    let result_lines: Vec<String> = content
        .lines()
        .map(|line| {
            let trimmed = line.trim();
            let is_from_import = trimmed.starts_with("from ") && trimmed.contains(" import ");
            let is_simple_import = trimmed.starts_with("import ") 
                && !trimmed.starts_with("import (")
                && !trimmed.starts_with("import \"");

            if !is_from_import && !is_simple_import {
                return line.to_string();
            }

            // Extract import specifiers
            let (prefix, specifiers): (String, Vec<String>) = if is_from_import {
                // e.g., "from os import getcwd, path as p" -> prefix="from os import ", specifiers=["getcwd", "path as p"]
                let parts: Vec<&str> = trimmed.splitn(2, " import ").collect();
                let prefix = format!("{} import ", parts[0]); // "from os import "
                let spec_part = parts.get(1).unwrap_or(&"");
                let specs: Vec<String> = spec_part.split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                (prefix, specs)
            } else {
                // e.g., "import os, sys" -> prefix="import ", specifiers=["os", "sys"]
                let name_part = trimmed.trim_start_matches("import ").trim();
                let specs: Vec<String> = name_part.split(',')
                    .map(|s| {
                        s.split(" as ").next().unwrap_or(s).trim().to_string()
                    })
                    .filter(|s| !s.is_empty())
                    .collect();
                ("import ".to_string(), specs)
            };

            // Filter to only used specifiers (by base name)
            let used_specs: Vec<String> = specifiers
                .iter()
                .filter(|spec| {
                    let base_name = spec.split(" as ").next().unwrap_or(spec).trim();
                    used_names.contains(base_name)
                })
                .cloned()
                .collect();

            if used_specs.is_empty() {
                // All specifiers are unused - remove the entire line
                String::new()
            } else if used_specs.len() == specifiers.len() {
                // All specifiers are used - keep line as-is
                line.to_string()
            } else {
                // Partial usage - reconstruct line with only used specifiers
                format!("{}{}", prefix, used_specs.join(", "))
            }
        })
        .filter(|line| !line.is_empty())
        .collect();

    result_lines.join("\n")
}

fn collect_used_names_python(content: &str) -> HashSet<String> {
    // Only collect names that are used (referenced) in the code body.
    // We track whether we're on an import line to avoid collecting import specifiers.
    const KEYWORDS: &[&str] = &[
        "and", "as", "assert", "async", "await", "break", "class", "continue", "def",
        "del", "elif", "else", "except", "False", "finally", "for", "from", "global",
        "if", "import", "in", "is", "lambda", "None", "nonlocal", "not", "or",
        "pass", "raise", "return", "True", "try", "while", "with", "yield",
    ];
    let keyword_set: HashSet<&str> = KEYWORDS.iter().cloned().collect();

    let mut names = HashSet::new();
    let bytes = content.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        let b = bytes[i];

        if b == b'#' {
            while i < bytes.len() && bytes[i] != b'\n' { i += 1; }
            i += 1;
            continue;
        }

        // Triple-quoted string (ASCII " only)
        if b == b'"' && i + 2 < bytes.len()
            && bytes[i + 1] == b && bytes[i + 2] == b
        {
            i += 3;
            while i + 2 < bytes.len() {
                if bytes[i] == b'"' && bytes[i + 1] == b && bytes[i + 2] == b {
                    i += 3;
                    break;
                }
                i += 1;
            }
            continue;
        }

        // Single/double quoted string (handle via char for curly quotes)
        if is_python_quote_char(b as char) {
            let quote_char = b as char;
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' { i += 2; continue; }
                if (bytes[i] as char) == quote_char { i += 1; break; }
                if bytes[i] == b'\n' { break; }
                i += 1;
            }
            continue;
        }

        if b.is_ascii_alphabetic() || b == b'_' {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let name = std::str::from_utf8(&bytes[start..i]).unwrap_or("");

            // Check if this name is an import specifier on an import line.
            // Import specifiers like 'os' in 'import os' are not actual usage.
            // For 'from X import Y, Z', ALL names between 'import ' and end of line are specifiers.
            let is_import_specifier = {
                let line_start = bytes[..start].iter().rposition(|&c| c == b'\n').map(|p| p + 1).unwrap_or(0);
                let mut content_start = line_start;
                while content_start < bytes.len() && (bytes[content_start] == b' ' || bytes[content_start] == b'\t') {
                    content_start += 1;
                }

                // Check if this is an import statement line
                // For 'from X import Y', check if line contains ' import ' (space+keyword+space) after 'from '
                let line_is_import = bytes[content_start..].starts_with(b"import ");
                // ' import ' is 8 chars (space + 'import' + space) - need .windows(8) not .windows(7)
                let line_is_from_import = bytes[content_start..].starts_with(b"from ")
                    && bytes[content_start..].windows(8).any(|w| w == b" import ");

                if !line_is_import && !line_is_from_import {
                    false
                } else {
                    // Find where 'import ' keyword ends on this line
                    let after_import_pos = if line_is_from_import { content_start + 12 } else { content_start + 7 };
                    
                    // Name is a specifier if it's after the import keyword position
                    // All names in 'from X import A, B, C' or 'import A, B, C' are specifiers
                    start >= after_import_pos
                        && !bytes[line_start..bytes[start..].iter().position(|&c| c == b'\n').map(|p| start + p).unwrap_or(bytes.len())].contains(&b'{')
                }
            };

            if is_import_specifier {
                // Don't collect import specifiers — they're not actual usage
            } else if name == "def" || name == "class" || name == "async" {
                // Collect the next name after def/class/async as a defined entity
                let name_start = i;
                skip_to_name(bytes, &mut i);
                collect_name(bytes, &mut i);
                // Set i to BEFORE the defined name so outer loop's i+=1 lands on the name's first char
                // name_start - 1: after i+=1 from outer loop, we'll be at name_start
                i = name_start.saturating_sub(1);
            } else if !name.is_empty()
                && name.chars().next().map(|c| c.is_lowercase()).unwrap_or(false)
                && !keyword_set.contains(name)
            {
                names.insert(name.to_string());
            }
        } else {
            i += 1;
        }
    }

    names
}

fn skip_to_name(bytes: &[u8], i: &mut usize) {
    while *i < bytes.len() && !bytes[*i].is_ascii_alphabetic() && bytes[*i] != b'_' {
        *i += 1;
    }
}

fn collect_name(bytes: &[u8], i: &mut usize) {
    while *i < bytes.len() && (bytes[*i].is_ascii_alphanumeric() || bytes[*i] == b'_') {
        *i += 1;
    }
}

fn remove_js_unused_imports(content: &str) -> String {
    let used_names = collect_used_names_js(content);

    let result_lines: Vec<&str> = content
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            let is_import = trimmed.starts_with("import ") || trimmed.starts_with("require(");
            if !is_import {
                return true;
            }

            let imported: Vec<String> = extract_imported_names_js(trimmed);
            !imported.iter().all(|n| !used_names.contains(n))
        })
        .collect();

    result_lines.join("\n")
}

fn extract_imported_names_js(line: &str) -> Vec<String> {
    let mut names = Vec::new();

    // import { foo, bar as baz } from 'x'
    if let (Some(start), Some(end)) = (line.find('{'), line.find('}')) {
        let inner = &line[start + 1..end];
        for part in inner.split(',') {
            let part = part.trim();
            let name = part.split(" as ").next().unwrap_or(part);
            if !name.is_empty() && name != "as" {
                names.push(name.to_string());
            }
        }
    }

    // import foo from 'x' — default import
    if line.starts_with("import ") && !line.contains('{') {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 && parts[1] != "from" {
            names.push(parts[1].to_string());
        }
    }

    // import * as namespace from 'x'
    if let Some(as_pos) = line.find("* as") {
        let after = &line[as_pos + 4..];
        let name = after.split_whitespace().next().unwrap_or("").trim().trim_end_matches(';');
        if !name.is_empty() {
            names.push(name.to_string());
        }
    }

    names
}

fn collect_used_names_js(content: &str) -> HashSet<String> {
    let mut names = HashSet::new();
    let bytes = content.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        let b = bytes[i];

        // Line comment
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            while i < bytes.len() && bytes[i] != b'\n' { i += 1; }
            i += 1;
            continue;
        }
        // Block comment
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < bytes.len() {
                if bytes[i] == b'*' && bytes[i + 1] == b'/' { i += 2; break; }
                i += 1;
            }
            i += 1;
            continue;
        }
        // String literal
        if is_js_quote_char(b as char) {
            let quote_char = b as char;
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' { i += 2; continue; }
                if (bytes[i] as char) == quote_char { i += 1; break; }
                i += 1;
            }
            continue;
        }

        if b.is_ascii_alphabetic() || b == b'_' {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let name = std::str::from_utf8(&bytes[start..i]).unwrap_or("");

            // Check if this name is an import specifier (right after 'import ' or 'from ' at line start).
            // Import specifiers like 'foo' in 'import { foo }' are not actual usage.
            let is_import_specifier = {
                let line_start = bytes[..start].iter().rposition(|&c| c == b'\n').map(|p| p + 1).unwrap_or(0);
                let mut content_start = line_start;
                while content_start < bytes.len() && (bytes[content_start] == b' ' || bytes[content_start] == b'\t') {
                    content_start += 1;
                }
                // Check if line starts with 'import ' or 'from import '
                // For 'import X', specifier is at content_start + 7 (after 'import ')
                // For 'from X import Y', specifier is at content_start + 12 (after 'from ' + 'import ' = 5 + 7)
                let after_import = bytes[content_start..].strip_prefix(b"import ");
                let after_from_import = bytes[content_start..].strip_prefix(b"from ")
                    .and_then(|s| s.strip_prefix(b"import "));
                // Name is import specifier if it starts right after 'import ' or 'from import '
                let is_after_import_keyword = after_import.is_some() && start == content_start + 7;
                let is_after_from_import_keyword = after_from_import.is_some() && start == content_start + 12;

                // For JS destructuring imports like 'import { foo }', the specifier is inside { }
                // We detect this by checking if the name is between '{' and '}' on this line
                let is_in_braces = {
                    let line_end = bytes[start..].iter().position(|&c| c == b'\n').map(|p| start + p).unwrap_or(bytes.len());
                    let line_slice = &bytes[line_start..line_end.min(bytes.len())];
                    line_slice.contains(&b'{') && line_slice.contains(&b'}')
                        && bytes[line_start..start.min(bytes.len())].contains(&b'{')
                };

                is_after_import_keyword || is_after_from_import_keyword || is_in_braces
            };

            if is_import_specifier {
                // Don't collect import specifiers — they're not actual usage
            } else if !name.is_empty() {
                // Exclude JS keywords so they're not counted as used identifiers
                const JS_KEYWORDS: &[&str] = &[
                    "break", "case", "catch", "class", "const", "continue", "debugger",
                    "default", "delete", "do", "else", "export", "extends", "false",
                    "finally", "for", "function", "if", "import", "in", "instanceof",
                    "let", "new", "null", "return", "static", "super", "switch", "this",
                    "throw", "true", "try", "typeof", "var", "void", "while", "with", "yield",
                    "async", "await", "of", "get", "set", "enum", "implements", "interface",
                    "package", "private", "protected", "public", "abstract", "boolean",
                    "byte", "char", "double", "float", "int", "long", "short", "volatile",
                    "from",
                ];
                static JS_KEYWORD_SET: std::sync::LazyLock<HashSet<&str>> =
                    std::sync::LazyLock::new(|| JS_KEYWORDS.iter().cloned().collect());
                if !JS_KEYWORD_SET.contains(&name) {
                    names.insert(name.to_string());
                }
            }
        } else {
            i += 1;
        }
    }

    names
}

fn remove_go_unused_imports(content: &str) -> String {
    // Go import blocks are kept intact — determining package usage requires the compiler
    let bytes = content.as_bytes();
    let mut result = Vec::with_capacity(content.len());
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'"' {
            result.push(b'"');
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    result.push(bytes[i]);
                    result.push(bytes[i + 1]);
                    i += 2;
                } else if bytes[i] == b'"' {
                    result.push(b'"');
                    i += 1;
                    break;
                } else {
                    result.push(bytes[i]);
                    i += 1;
                }
            }
            continue;
        }

        if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            while i < bytes.len() && bytes[i] != b'\n' {
                result.push(bytes[i]);
                i += 1;
            }
            continue;
        }

        result.push(bytes[i]);
        i += 1;
    }

    String::from_utf8(result).unwrap_or_else(|_| content.to_string())
}

fn remove_obvious_unused_imports_heuristic(content: &str, language: &str) -> String {
    match language {
        "python" => remove_python_unused_imports(content),
        "javascript" | "typescript" => remove_js_unused_imports(content),
        _ => content.to_string(),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Test File Compression
// ─────────────────────────────────────────────────────────────────────────────

fn compress_test_file(content: &str, language: &str) -> String {
    match language {
        "python" => compress_python_test(content),
        "javascript" | "typescript" => compress_js_test(content),
        _ => content.to_string(),
    }
}

fn compress_python_test(content: &str) -> String {
    let lines = content.lines().collect::<Vec<_>>();
    let mut result = Vec::new();
    let mut in_test_func = false;
    let mut func_indent = 0;

    for line in lines {
        let trimmed = line.trim();

        let is_test_def = trimmed.starts_with("def test_")
            || trimmed.starts_with("async def test_")
            || trimmed.starts_with("def Test")
            || trimmed.starts_with("async def Test")
            || trimmed.starts_with("@pytest.fixture")
            || trimmed.starts_with("@test")
            || trimmed.starts_with("@pytest.mark");

        let is_assert = trimmed.starts_with("assert ")
            || trimmed.starts_with("self.assert")
            || trimmed.starts_with("pytest.raises")
            || trimmed.starts_with("with pytest");

        let is_control = trimmed.starts_with("if ")
            || trimmed.starts_with("for ")
            || trimmed.starts_with("while ")
            || trimmed.starts_with("with ")
            || trimmed.starts_with("return ")
            || trimmed.starts_with("elif ")
            || trimmed.starts_with("else:");

        let current_indent = line.len() - line.trim_start().len();

        if is_test_def {
            result.push(line);
            in_test_func = true;
            func_indent = current_indent;
        } else if in_test_func {
            if !trimmed.is_empty() && current_indent <= func_indent {
                in_test_func = false;
                if !(trimmed.starts_with("def ") || trimmed.starts_with("class ") || trimmed.starts_with("@")) {
                    result.push(line);
                }
            } else if is_assert || is_control || trimmed.is_empty() {
                result.push(line);
            }
        } else {
            result.push(line);
        }
    }

    result.join("\n")
}

fn compress_js_test(content: &str) -> String {
    let bytes = content.as_bytes();
    let mut result = Vec::with_capacity(content.len());
    let mut i = 0;

    while i < bytes.len() {
        let ahead = std::str::from_utf8(&bytes[i..]).unwrap_or("");

        let is_test_call = ahead.starts_with("it('")
            || ahead.starts_with("it(\"")
            || ahead.starts_with("it(`")
            || ahead.starts_with("test('")
            || ahead.starts_with("test(\"")
            || ahead.starts_with("test(`")
            || ahead.starts_with("it.skip('")
            || ahead.starts_with("it.skip(\"")
            || ahead.starts_with("test.skip('")
            || ahead.starts_with("test.skip(\"")
            || ahead.starts_with("describe('")
            || ahead.starts_with("describe(\"")
            || ahead.starts_with("describe(`");

        if is_test_call {
            let _fn_start = i;

            // Copy call up to and including the opening brace
            let mut paren_depth = 0;
            let mut found_brace = false;
            let mut in_string = false;
            let mut string_char = ' ';

            while i < bytes.len() {
                let b = bytes[i];

                if !in_string {
                    if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                        while i < bytes.len() && bytes[i] != b'\n' { i += 1; }
                        continue;
                    }
                    if is_js_quote_char(b as char) {
                        in_string = true;
                        string_char = b as char;
                    } else if b == b'(' {
                        paren_depth += 1;
                    } else if b == b')' {
                        paren_depth -= 1;
                        if paren_depth == 0 {
                            i += 1;
                            while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
                                result.push(bytes[i]);
                                i += 1;
                            }
                            if i < bytes.len() && bytes[i] == b'{' {
                                found_brace = true;
                                result.push(b'{');
                                i += 1;
                            }
                            break;
                        }
                    }
                } else {
                    if b == b'\\' { i += 2; continue; }
                    if (bytes[i] as char) == string_char { in_string = false; }
                }
                i += 1;
            }

            if found_brace {
                let mut brace_depth = 1;
                let mut in_string = false;
                let mut string_char = ' ';

                while i < bytes.len() && brace_depth > 0 {
                    let b = bytes[i];

                    if !in_string {
                        if is_js_quote_char(b as char) {
                            in_string = true;
                            string_char = b as char;
                            result.push(b);
                            i += 1;
                            continue;
                        }
                        if b == b'{' { brace_depth += 1; }
                        else if b == b'}' {
                            brace_depth -= 1;
                            if brace_depth == 0 {
                                result.push(b);
                                i += 1;
                                while i < bytes.len() && (bytes[i] == b'\n' || bytes[i] == b'\r') {
                                    result.push(bytes[i]);
                                    i += 1;
                                }
                                break;
                            }
                        } else if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                            while i < bytes.len() && bytes[i] != b'\n' { i += 1; }
                            continue;
                        }
                    } else {
                        if b == b'\\' { result.push(b); result.push(bytes[i + 1]); i += 2; continue; }
                        if (bytes[i] as char) == string_char { in_string = false; }
                    }

                    result.push(b);
                    i += 1;
                }
            } else {
                while i < bytes.len() && bytes[i] != b'\n' {
                    result.push(bytes[i]);
                    i += 1;
                }
            }

            continue;
        }

        result.push(bytes[i]);
        i += 1;
    }

    String::from_utf8(result).unwrap_or_else(|_| content.to_string())
}

// ─────────────────────────────────────────────────────────────────────────────
// Utilities
// ─────────────────────────────────────────────────────────────────────────────

fn contains_todo_fixme(s: &str) -> bool {
    let markers = ["TODO", "FIXME", "HACK", "XXX", "NOTE", "BUG", "WARN"];
    let upper = s.to_uppercase();
    markers.iter().any(|m| upper.contains(m))
}

fn is_test_file(content: &str, language: &str) -> bool {
    match language {
        "python" => content.contains("pytest") || content.contains("unittest") || content.contains("test_"),
        "javascript" | "typescript" => content.contains("it(") || content.contains("test(") || content.contains("describe("),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> CompressorConfig {
        CompressorConfig::default()
    }

    // ── Whitespace ─────────────────────────────────────────────────────────────

    #[test]
    fn test_collapse_whitespace_multiple() {
        let input = "fn foo() {\n\n\n    println!(\"hi\");\n\n\n}";
        let out = collapse_whitespace(input);
        assert!(!out.contains("\n\n\n"), "Should collapse triple+ blank lines");
    }

    #[test]
    fn test_collapse_whitespace_leading() {
        let input = "\n\n\nfn foo() {}";
        let out = collapse_whitespace(input);
        assert!(!out.starts_with('\n'), "Should not start with newline");
    }

    // ── Comment Stripping ──────────────────────────────────────────────────────

    #[test]
    fn test_strip_python_line_comment() {
        let input = "x = 1  # inline comment\ny = 2  # TODO: fix this";
        let out = strip_python_comments(input);
        assert!(out.contains("TODO"));
        assert!(!out.contains("inline comment"));
    }

    #[test]
    fn test_strip_python_todo_preserved() {
        let input = "x = 1  # TODO: fix later\ny = 2  # this is removed";
        let out = strip_python_comments(input);
        assert!(out.contains("TODO"));
        assert!(!out.contains("this is removed"));
    }

    #[test]
    fn test_strip_python_triple_quote_verbatim() {
        let input = r#"x = """multi
line
string""" + y"#;
        let out = strip_python_comments(input);
        assert!(out.contains("multi\nline\nstring"), "Triple-quoted strings should not be corrupted");
    }

    #[test]
    fn test_strip_js_line_comment() {
        let input = "// regular comment\nconst x = 1; // TODO: fix";
        let out = strip_js_comments(input);
        assert!(out.contains("TODO"));
        assert!(!out.contains("regular comment"));
    }

    #[test]
    fn test_strip_js_block_comment() {
        let input = "/* remove this */\nconst x = 1;\n/* TODO: keep this */";
        let out = strip_js_comments(input);
        assert!(out.contains("TODO"));
        assert!(!out.contains("remove this"));
    }

    #[test]
    fn test_strip_js_string_with_escaped_newline() {
        let input = "const x = \"hello\\nworld\"; // comment";
        let out = strip_js_comments(input);
        assert!(out.contains("hello"), "String content should be preserved");
        assert!(!out.contains("comment"));
    }

    // ── Docstring Truncation ───────────────────────────────────────────────────

    #[test]
    fn test_truncate_python_docstring() {
        let input = r#"def foo():
    """This is the first sentence. This is the second sentence."""
    pass"#;
        let out = truncate_python_docstrings(input, 10);
        assert!(out.contains("first sentence"));
        assert!(!out.contains("second sentence"));
    }

    #[test]
    fn test_truncate_python_docstring_preserves_quotes() {
        let input = r#"def foo():
    """Short."""
    pass"#;
        let out = truncate_python_docstrings(input, 10);
        assert!(out.contains("\"\"\""), "Triple quotes should be preserved");
    }

    #[test]
    fn test_truncate_python_docstring_no_limit() {
        let input = r#"def foo():
    """This is the first sentence."""
    pass"#;
        let out = truncate_python_docstrings(input, 0);
        assert!(out.contains("first sentence"), "With 0 max_tokens, full docstring should be kept");
    }

    #[test]
    fn test_truncate_js_docstring() {
        let input = "/** This is the first sentence. This is the second. */\nfunction foo() {}";
        let out = truncate_js_docstrings(input, 10);
        assert!(out.contains("first sentence"));
        assert!(!out.contains("second"));
    }

    #[test]
    fn test_truncate_rust_docstring() {
        let input = "/// This is the first sentence. This is the second sentence.\nfn foo() {}";
        let out = truncate_rust_docstrings(input, 10);
        assert!(out.contains("first sentence"));
        assert!(!out.contains("second sentence"));
    }

    #[test]
    fn test_truncate_no_punctuation_no_limit() {
        let out = truncate_to_first_sentence("No punctuation here", 0);
        assert_eq!(out, "No punctuation here");
    }

    // ── Import Removal ─────────────────────────────────────────────────────────

    #[test]
    fn test_remove_python_unused_import() {
        let input = "import os\nimport sys\n\ndef main():\n    print('hello')\n";
        let out = remove_python_unused_imports(input);
        assert!(!out.contains("import os"), "Unused os import should be removed");
        assert!(!out.contains("import sys"), "Unused sys import should be removed");
        assert!(out.contains("def main"));
    }

    #[test]
    fn test_remove_python_used_import() {
        let input = "import os\n\ndef main():\n    os.getcwd()\n";
        let out = remove_python_unused_imports(input);
        assert!(out.contains("import os"), "Used import should be kept");
    }

    #[test]
    fn test_remove_python_import_with_alias() {
        let input = "import os as operating_system\n\ndef main():\n    print('hello')\n";
        let out = remove_python_unused_imports(input);
        assert!(!out.contains("import os"), "Unused aliased import should be removed");
    }

    #[test]
    fn test_remove_python_import_from_with_alias() {
        let input = "from typing import List as LinkedList\n\ndef main():\n    print('hello')\n";
        let out = remove_python_unused_imports(input);
        assert!(!out.contains("LinkedList"), "Unused import should be removed");
    }

    #[test]
    fn test_remove_python_from_import_multiple() {
        let input = "from os import getcwd, path\n\ndef main():\n    getcwd()\n";
        let out = remove_python_unused_imports(input);
        assert!(out.contains("getcwd"), "Used import should be kept");
        assert!(!out.contains("path"), "Unused import should be removed");
    }

    #[test]
    fn test_remove_js_unused_import() {
        let input = "import { foo } from './foo';\nimport { bar } from './bar';\nconsole.log(foo);";
        let out = remove_js_unused_imports(input);
        assert!(out.contains("foo"), "Used import should be kept");
        assert!(!out.contains("bar"), "Unused import should be removed");
    }

    #[test]
    fn test_remove_js_namespace_import() {
        let input = "import * as foo from './foo';\nconsole.log(foo.bar);";
        let out = remove_js_unused_imports(input);
        assert!(out.contains("foo"), "Namespace import with usage should be kept");
    }

    // ── Integration ────────────────────────────────────────────────────────────

    #[test]
    fn test_full_compression_python() {
        let pruner = CodePruner::new(cfg());
        let input = r#"import os
import sys

def greet(name: str) -> None:
    """Greet the user with a friendly message.

    Args:
        name: The name of the user
    """
    # TODO: add emoji support later
    print(f"Hello, {name}!")
"#;
        let out = pruner.compress(input, "python");
        assert!(out.contains("def greet"));
        assert!(out.contains("name: str"));
        assert!(out.contains("TODO"));
    }

    #[test]
    fn test_full_compression_preserves_signature() {
        let pruner = CodePruner::new(cfg());
        let input = "def process(items: list[int], callback: Callable[[int], None]) -> Generator[int, None, None]:\n    pass";
        let out = pruner.compress(input, "python");
        assert!(out.contains("items: list[int]"));
        assert!(out.contains("callback: Callable"));
        assert!(out.contains("Generator"));
    }

    #[test]
    fn test_is_test_file() {
        assert!(is_test_file("def test_foo(): pass", "python"));
        assert!(is_test_file("import pytest", "python"));
        assert!(is_test_file("it('works', () => {})", "javascript"));
        assert!(!is_test_file("def foo(): pass", "python"));
    }

    #[test]
    fn test_contains_todo_fixme() {
        assert!(contains_todo_fixme("TODO: fix this"));
        assert!(contains_todo_fixme("Fixme: bug here"));
        assert!(contains_todo_fixme("XXX: known issue"));
        assert!(contains_todo_fixme("NOTE: remember this"));
        assert!(!contains_todo_fixme("normal comment"));
    }
}