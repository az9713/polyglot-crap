use anyhow::{Context, Result};
use std::path::Path;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Node, Query, QueryCursor};

use crate::language::LanguageConfig;

/// One function's complexity, with enough location info to join against coverage.
#[derive(Debug, Clone)]
pub struct FunctionComplexity {
    pub file: std::path::PathBuf,
    pub name: String,
    /// 1-indexed first line of the function (inclusive).
    pub start_line: usize,
    /// 1-indexed last line of the function (inclusive).
    pub end_line: usize,
    /// McCabe cyclomatic complexity, minimum 1.0.
    pub cyclomatic: f64,
}

pub fn walk_file(path: &Path, source: &str, config: &LanguageConfig) -> Result<Vec<FunctionComplexity>> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&config.language)
        .with_context(|| format!("failed to set language for {:?}", path))?;

    let tree = parser
        .parse(source, None)
        .with_context(|| format!("failed to parse {:?}", path))?;

    let fn_query = Query::new(&config.language, config.fn_query_src)
        .with_context(|| format!("bad fn query for language {}", config.name))?;
    let cc_query = Query::new(&config.language, config.cc_query_src)
        .with_context(|| format!("bad cc query for language {}", config.name))?;

    let fn_def_idx = fn_query
        .capture_index_for_name("fn.def")
        .with_context(|| format!("fn query missing @fn.def capture for {}", config.name))?;
    let fn_name_idx = fn_query.capture_index_for_name("fn.name");
    let class_name_idx = fn_query.capture_index_for_name("class.name");

    let branch_idx = cc_query
        .capture_index_for_name("branch")
        .with_context(|| format!("cc query missing @branch capture for {}", config.name))?;

    let source_bytes = source.as_bytes();
    let root = tree.root_node();

    let mut cursor = QueryCursor::new();
    let mut fn_matches = cursor.matches(&fn_query, root, source_bytes);

    let mut results = Vec::new();

    while let Some(m) = fn_matches.next() {
        let def_node = m
            .captures
            .iter()
            .find(|c| c.index == fn_def_idx)
            .map(|c| c.node);
        let Some(def_node) = def_node else { continue };

        let fn_name = fn_name_idx
            .and_then(|idx| m.captures.iter().find(|c| c.index == idx))
            .map(|c| node_text(c.node, source_bytes));

        let class_name = class_name_idx
            .and_then(|idx| m.captures.iter().find(|c| c.index == idx))
            .map(|c| node_text(c.node, source_bytes));

        let name = build_name(fn_name.as_deref(), class_name.as_deref(), &def_node);

        let complexity = count_branches(&cc_query, branch_idx, def_node, source_bytes);

        results.push(FunctionComplexity {
            file: path.to_path_buf(),
            name,
            start_line: def_node.start_position().row + 1,
            end_line: def_node.end_position().row + 1,
            cyclomatic: complexity as f64,
        });
    }

    Ok(results)
}

fn count_branches(query: &Query, branch_idx: u32, scope: Node, source: &[u8]) -> usize {
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(query, scope, source);
    let mut count = 0;
    while let Some(m) = matches.next() {
        for c in m.captures {
            if c.index == branch_idx {
                count += 1;
            }
        }
    }
    1 + count
}

fn node_text(node: Node, source: &[u8]) -> String {
    std::str::from_utf8(&source[node.start_byte()..node.end_byte()])
        .unwrap_or("<invalid utf8>")
        .to_string()
}

fn build_name(fn_name: Option<&str>, class_name: Option<&str>, node: &Node) -> String {
    match (fn_name, class_name) {
        (Some(f), Some(c)) => format!("{}::{}", strip_receiver(c), f),
        (Some(f), None) => f.to_string(),
        (None, _) => format!("<anonymous:{}>", node.start_position().row + 1),
    }
}

// Go receivers look like "(r *Receiver)" — extract the type name.
fn strip_receiver(raw: &str) -> &str {
    let trimmed = raw.trim_matches(|c: char| c == '(' || c == ')').trim();
    trimmed
        .split_whitespace()
        .last()
        .map(|s| s.trim_start_matches('*'))
        .unwrap_or(trimmed)
}
