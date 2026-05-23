//! Smoke-test every language's fn+cc queries by parsing a trivial snippet.
//! This catches "Invalid node type" errors in the .scm files at CI time.

use polycrap::{complexity, language};

macro_rules! check_lang {
    ($name:literal, $ext:literal, $src:expr) => {{
        let config = language::by_name($name).expect(concat!($name, " not registered"));
        let tmp = std::env::temp_dir().join(concat!("polycrap_qtest.", $ext));
        std::fs::write(&tmp, $src).unwrap();
        let fns = complexity::analyze_file_with_lang(&tmp, config)
            .unwrap_or_else(|e| panic!("language '{}' query error: {}", $name, e));
        assert!(!fns.is_empty(), "language '{}': no functions found in test snippet", $name);
        println!("  {} → {} function(s), CC={}", $name, fns.len(), fns[0].cyclomatic);
    }};
}

#[test]
fn all_language_queries_are_valid() {
    println!();
    check_lang!("python", "py", "def f(x):\n    if x > 0:\n        return x\n    return 0\n");
    check_lang!("javascript", "js", "function f(x) { if (x > 0) return x; return 0; }");
    check_lang!("typescript", "ts", "function f(x: number): number { if (x > 0) return x; return 0; }");
    check_lang!("tsx", "tsx", "function App() { return <div/>; }");
    check_lang!("go", "go", "package p\nfunc F(x int) int { if x > 0 { return x }; return 0 }");
    check_lang!("java", "java", "class T { int f(int x) { if (x > 0) return x; return 0; } }");
    check_lang!("csharp", "cs", "class T { int F(int x) { if (x > 0) return x; return 0; } }");
    check_lang!("ruby", "rb", "def f(x)\n  if x > 0\n    return x\n  end\n  0\nend\n");
    check_lang!("rust", "rs", "fn f(x: i32) -> i32 { if x > 0 { x } else { 0 } }");
    check_lang!("c", "c", "int f(int x) { if (x > 0) return x; return 0; }");
    check_lang!("cpp", "cpp", "int f(int x) { if (x > 0) return x; return 0; }");
    check_lang!("php", "php", "<?php function f($x) { if ($x > 0) return $x; return 0; }");
}
