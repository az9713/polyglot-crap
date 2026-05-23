//! Language registry — maps file extensions to tree-sitter grammars and query files.

use once_cell::sync::Lazy;
use std::path::Path;

/// Configuration for a single language: grammar, extensions, and query sources.
pub struct LanguageConfig {
    pub name: &'static str,
    pub language: tree_sitter::Language,
    pub extensions: &'static [&'static str],
    /// S-expression query that captures function definitions.
    /// Capture names: `@fn.def`, `@fn.name`, optional `@class.name`
    pub fn_query_src: &'static str,
    /// S-expression query that captures CC decision points. Capture name: `@branch`
    pub cc_query_src: &'static str,
}

static REGISTRY: Lazy<Vec<LanguageConfig>> = Lazy::new(|| {
    vec![
        LanguageConfig {
            name: "python",
            language: tree_sitter_python::LANGUAGE.into(),
            extensions: &["py", "pyw"],
            fn_query_src: include_str!("complexity/queries/python_fn.scm"),
            cc_query_src: include_str!("complexity/queries/python_cc.scm"),
        },
        LanguageConfig {
            name: "javascript",
            language: tree_sitter_javascript::LANGUAGE.into(),
            extensions: &["js", "jsx", "mjs", "cjs"],
            fn_query_src: include_str!("complexity/queries/javascript_fn.scm"),
            cc_query_src: include_str!("complexity/queries/javascript_cc.scm"),
        },
        LanguageConfig {
            name: "typescript",
            language: tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            extensions: &["ts"],
            fn_query_src: include_str!("complexity/queries/typescript_fn.scm"),
            cc_query_src: include_str!("complexity/queries/typescript_cc.scm"),
        },
        LanguageConfig {
            name: "tsx",
            language: tree_sitter_typescript::LANGUAGE_TSX.into(),
            extensions: &["tsx"],
            fn_query_src: include_str!("complexity/queries/typescript_fn.scm"),
            cc_query_src: include_str!("complexity/queries/typescript_cc.scm"),
        },
        LanguageConfig {
            name: "go",
            language: tree_sitter_go::LANGUAGE.into(),
            extensions: &["go"],
            fn_query_src: include_str!("complexity/queries/go_fn.scm"),
            cc_query_src: include_str!("complexity/queries/go_cc.scm"),
        },
        LanguageConfig {
            name: "java",
            language: tree_sitter_java::LANGUAGE.into(),
            extensions: &["java"],
            fn_query_src: include_str!("complexity/queries/java_fn.scm"),
            cc_query_src: include_str!("complexity/queries/java_cc.scm"),
        },
        LanguageConfig {
            name: "csharp",
            language: tree_sitter_c_sharp::LANGUAGE.into(),
            extensions: &["cs"],
            fn_query_src: include_str!("complexity/queries/csharp_fn.scm"),
            cc_query_src: include_str!("complexity/queries/csharp_cc.scm"),
        },
        LanguageConfig {
            name: "ruby",
            language: tree_sitter_ruby::LANGUAGE.into(),
            extensions: &["rb"],
            fn_query_src: include_str!("complexity/queries/ruby_fn.scm"),
            cc_query_src: include_str!("complexity/queries/ruby_cc.scm"),
        },
        LanguageConfig {
            name: "rust",
            language: tree_sitter_rust::LANGUAGE.into(),
            extensions: &["rs"],
            fn_query_src: include_str!("complexity/queries/rust_fn.scm"),
            cc_query_src: include_str!("complexity/queries/rust_cc.scm"),
        },
        LanguageConfig {
            name: "c",
            language: tree_sitter_c::LANGUAGE.into(),
            extensions: &["c", "h"],
            fn_query_src: include_str!("complexity/queries/c_fn.scm"),
            cc_query_src: include_str!("complexity/queries/c_cc.scm"),
        },
        LanguageConfig {
            name: "cpp",
            language: tree_sitter_cpp::LANGUAGE.into(),
            extensions: &["cpp", "cc", "cxx", "hpp", "hxx"],
            fn_query_src: include_str!("complexity/queries/cpp_fn.scm"),
            cc_query_src: include_str!("complexity/queries/cpp_cc.scm"),
        },
        LanguageConfig {
            name: "php",
            language: tree_sitter_php::LANGUAGE_PHP.into(),
            extensions: &["php"],
            fn_query_src: include_str!("complexity/queries/php_fn.scm"),
            cc_query_src: include_str!("complexity/queries/php_cc.scm"),
        },
    ]
});

/// Detect the language for a file based on its extension.
pub fn detect(path: &Path) -> Option<&'static LanguageConfig> {
    let ext = path.extension()?.to_str()?;
    REGISTRY.iter().find(|c| c.extensions.contains(&ext))
}

/// Look up a language by name (e.g. `"python"`, `"go"`).
pub fn by_name(name: &str) -> Option<&'static LanguageConfig> {
    REGISTRY.iter().find(|c| c.name == name)
}

/// All registered language names, sorted.
pub fn all_names() -> Vec<&'static str> {
    let mut names: Vec<_> = REGISTRY.iter().map(|c| c.name).collect();
    names.sort_unstable();
    names
}
