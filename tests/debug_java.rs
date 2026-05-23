use polycrap::{complexity, language};

#[test]
fn debug_java_parsing() {
    let source = r#"public class OrderProcessor {
    public double calculateTotal(Order order) {
        double total = 0;
        for (Item item : order.getItems()) {
            if (item.isDiscounted()) {
                total += item.getPrice() * 0.8;
            } else {
                total += item.getPrice();
            }
        }
        return total;
    }
}"#;

    let tmp = std::env::temp_dir().join("polycrap_test_java_debug.java");
    std::fs::write(&tmp, source).unwrap();

    let fns = complexity::analyze_file_with_lang(&tmp, language::by_name("java").unwrap())
        .expect("analysis failed");

    println!("Found {} functions", fns.len());
    for f in &fns {
        println!("  {} start={} CC={}", f.name, f.start_line, f.cyclomatic);
    }
    assert!(!fns.is_empty(), "Expected to find at least one function");
}
