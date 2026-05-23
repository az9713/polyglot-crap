mod walker;

pub use walker::FunctionComplexity;

use anyhow::Result;
use std::path::Path;

use crate::language;

pub fn analyze_file(path: &Path) -> Result<Vec<FunctionComplexity>> {
    let Some(config) = language::detect(path) else {
        return Ok(vec![]);
    };
    let source = std::fs::read_to_string(path)?;
    walker::walk_file(path, &source, config)
}

pub fn analyze_file_with_lang(path: &Path, lang: &'static crate::language::LanguageConfig) -> Result<Vec<FunctionComplexity>> {
    let source = std::fs::read_to_string(path)?;
    walker::walk_file(path, &source, lang)
}

/// Walk a directory tree and return complexity for all recognized source files.
///
/// If `force_lang` is `Some`, every file in the tree is analyzed with that language
/// regardless of extension (useful with `--lang`).
pub fn analyze_tree<S: AsRef<str>>(root: &Path, excludes: &[S], force_lang: Option<&'static crate::language::LanguageConfig>) -> Result<Vec<FunctionComplexity>> {
    use ignore::WalkBuilder;
    use rayon::prelude::*;

    let excludes: Vec<&str> = excludes.iter().map(|s| s.as_ref()).collect();

    let paths: Vec<std::path::PathBuf> = WalkBuilder::new(root)
        .hidden(false)
        .git_ignore(true)
        .build()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .map(|e| e.into_path())
        .filter(|p| {
            if force_lang.is_none() && language::detect(p).is_none() {
                return false;
            }
            let rel = p.strip_prefix(root).unwrap_or(p);
            let rel_str = rel.to_string_lossy();
            !excludes.iter().any(|ex| rel_str.contains(ex))
        })
        .collect();

    let results: Vec<FunctionComplexity> = paths
        .par_iter()
        .filter_map(|p| {
            if let Some(lang) = force_lang {
                analyze_file_with_lang(p, lang).ok()
            } else {
                analyze_file(p).ok()
            }
        })
        .flatten()
        .collect();

    Ok(results)
}
