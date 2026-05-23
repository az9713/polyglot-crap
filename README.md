# polycrap — CRAP Metric for Every Language

> **CRAP(m) = comp(m)² × (1 − cov(m)/100)³ + comp(m)**

A standalone CLI that computes the [Change Risk Anti-Patterns (CRAP)](https://www.artima.com/weblogs/viewpost.jsp?thread=210016) metric for Python, JavaScript, TypeScript, Java, Go, C, C++, C#, Ruby, Rust, and PHP — all from one binary.

---

## The Origin Story

### It Starts with a YouTube Video

This project grew out of the video **"Is Your AI Code Producing CRAP? (Here's How To Fix It)"** by Simon:

[![Is Your AI Code Producing CRAP?](https://img.youtube.com/vi/XuMR1pgc6pc/maxresdefault.jpg)](https://www.youtube.com/watch?v=XuMR1pgc6pc)

> [Watch on YouTube →](https://www.youtube.com/watch?v=XuMR1pgc6pc)

The video explores a striking problem: as AI-assisted coding gets faster, the *complexity* of the code it produces often escapes notice. Tests are written to cover the happy path; branchy error-handling and business logic accumulates untested. The CRAP metric is a single number that captures exactly this: **how dangerous is it that this function isn't tested?**

### The cargo-crap starting point

[cargo-crap](https://github.com/minikin/cargo-crap) is an excellent Rust tool that implements the CRAP metric using `syn` (Rust's AST parser) and LCOV coverage files. The pipeline is elegant:

```
Rust source → syn AST → FunctionComplexity
LCOV file   → HashMap<File, LineCoverage>
                          ↓
                    merge() → Vec<CrapEntry>
                          ↓
               render(format, threshold, ...)
```

The insight from studying cargo-crap's architecture: **roughly 80% of the tool is language-agnostic.** The entire coverage parsing, path matching, scoring, delta comparison, and rendering pipeline works identically for every language. Only the ~600-line AST walker is Rust-specific.

### The leap to polyglot

The question became: *can we replace that one Rust-specific module with a universal AST engine that handles all languages?*

The answer is **tree-sitter**.

---

## What is tree-sitter?

[tree-sitter](https://tree-sitter.github.io/tree-sitter/) is a parser generator and incremental parsing library originally built for the Atom editor (now used in Neovim, Helix, GitHub's code search, and many others). It generates **concrete syntax trees** for source files in dozens of languages.

Key properties that make it ideal for polycrap:

| Property | Why it matters for CRAP |
|---|---|
| **Universal Rust API** | One `Parser` struct, one `Query` type — language is just a config value |
| **S-expression query language** | CC decision points declared in `.scm` files, not Rust code |
| **Grammars ship as crates** | `tree-sitter-python`, `tree-sitter-java`, etc. — just add to `Cargo.toml` |
| **Correct parse trees** | Named fields (`name:`, `body:`) make queries precise and non-fragile |
| **Zero external deps** | Grammars compile C via the `cc` crate — no system parsers required |

### How tree-sitter queries work

A tree-sitter **query** is an S-expression pattern that matches nodes in the parse tree. For example, this is `python_fn.scm` — the query that finds every function in a Python file:

```scheme
(function_definition
  name: (identifier) @fn.name
  body: (_) @fn.body) @fn.def
```

When you run this query against a Python file, tree-sitter returns every node tagged with `@fn.def`, plus its child `@fn.name`. The capture name convention polycrap uses:

| Capture | Meaning |
|---|---|
| `@fn.def` | The full function/method node (start + end line) |
| `@fn.name` | The function name identifier |
| `@class.name` | Enclosing class name (for `ClassName::method` display) |
| `@branch` | Any decision point that increments CC |

The CC query for Python (`python_cc.scm`) is equally readable:

```scheme
[
  (if_statement)
  (elif_clause)
  (for_statement)
  (while_statement)
  (except_clause)
  (conditional_expression)
] @branch

(boolean_operator) @branch
```

Every `if`, `for`, `while`, `except`, ternary, and boolean `and`/`or` adds 1 to the complexity count. The base complexity is 1 (the linear path), so `cyclomatic = 1 + branch_count`.

---

## What is the CRAP metric?

CRAP was defined by Alberto Savoia and Bob Evans in 2007. The formula is:

```
CRAP(m) = comp(m)² × (1 − cov(m)/100)³ + comp(m)
```

where:
- **comp(m)** = cyclomatic complexity (minimum 1, every decision point adds 1)
- **cov(m)** = test coverage percentage `[0, 100]`

### Intuition behind the formula

The formula has two terms:

1. **`comp² × (1 − cov/100)³`** — the quadratic *risk* term. Complexity is squared to penalise complexity growth harshly. The cubic uncoverage factor means even modest coverage brings the first term down fast: at 50% coverage, the multiplier is only `0.5³ = 0.125`.

2. **`+ comp`** — the linear *irreducible complexity* term. Even with 100% coverage, CRAP equals CC. Tests cap the damage complexity does, but don't eliminate the complexity itself.

### Key properties

| Scenario | Result |
|---|---|
| CC=1, 100% covered | CRAP = 1.0 — the lower bound |
| CC=4, 50% covered | 16 × 0.125 + 4 = **6.0** |
| CC=6, 0% covered | 36 × 1 + 6 = **42.0** (published example) |
| CC=7, 0% covered | 49 + 7 = **56.0** |
| CC=9, 0% covered | 81 + 9 = **90.0** |
| CC=30, 100% covered | 900 × 0 + 30 = 30.0 — just at threshold |
| CC=31, 100% covered | ~31.something > 30 — **no escape above CC=30** |

The last two rows reveal the most important property: **above CC ≈ 30, no amount of test coverage can bring the score below the default threshold of 30.** A function that complex is crappy by definition.

### Default threshold: 30

polycrap flags functions with CRAP > 30 as ✗ (crappy). Functions between `threshold/3` and `threshold` get ▲ (moderate). Below that is ✓ (clean).

---

## Architecture

```
Source file (any extension)
         │
         ▼
src/language.rs          ← detect language from extension → LanguageConfig
         │                 (or force with --lang)
         ▼
src/complexity/
  mod.rs                 ← analyze_file / analyze_tree (rayon parallel walk)
  walker.rs              ← universal tree-sitter walker
  queries/
    python_fn.scm        ← @fn.def, @fn.name captures
    python_cc.scm        ← @branch captures
    javascript_fn.scm
    javascript_cc.scm
    ...                  ← one fn+cc pair per language
         │
         ▼  Vec<FunctionComplexity>   ← language-agnostic from here on
         │
         ├── src/coverage.rs   ← LCOV parser (SF/DA records)
         ├── src/merge.rs      ← path-matching join, CRAP scoring
         ├── src/score.rs      ← CRAP formula
         ├── src/delta.rs      ← baseline comparison
         └── src/report/       ← human / JSON / GitHub / Markdown / SARIF
```

The abstraction boundary at `Vec<FunctionComplexity>` is the key design decision: everything below it is universal, everything above it is language-specific (just the `.scm` query files).

### The universal walker (`src/complexity/walker.rs`)

```rust
// For every source file:
// 1. Run fn_query on root → collect (def_node, name, class) tuples
// 2. For each def_node, run cc_query within that scope → count @branch captures
// 3. cyclomatic = 1 + branch_count

pub fn walk_file(path: &Path, source: &str, config: &LanguageConfig)
    -> Result<Vec<FunctionComplexity>>
```

tree-sitter 0.25 uses the `StreamingIterator` trait for query results (for memory efficiency with large files — results are generated lazily without heap allocation). The walker uses a `while let Some(m) = matches.next()` loop to consume matches one at a time.

### Path matching (`src/merge.rs`)

LCOV files use relative paths; the AST walker produces absolute paths. The `PathIndex` struct implements two-level matching:

1. **Fast path**: canonical absolute path → direct hash lookup
2. **Slow path**: component-wise suffix matching — `src/foo.py` matches `/home/user/project/src/foo.py` without needing to know the project root

---

## Supported Languages

| Language | Extensions | Branch nodes counted |
|---|---|---|
| Python | `.py`, `.pyw` | `if`, `elif`, `for`, `while`, `except`, ternary, `and`/`or` |
| JavaScript | `.js`, `.jsx`, `.mjs` | `if`, `for`, `for...in`, `while`, `do`, `switch case`, `catch`, ternary, `&&`/`\|\|`/`??` |
| TypeScript | `.ts` | same as JS + `method_signature` |
| TSX | `.tsx` | same as TypeScript |
| Go | `.go` | `if`, `for`, `select` cases, `switch` cases, `&&`/`\|\|` |
| Java | `.java` | `if`, `for`, `for...each`, `while`, `do`, `switch` block, `catch`, ternary, `&&`/`\|\|` |
| C# | `.cs` | `if`, `for`, `foreach`, `while`, `do`, `switch section`, `catch`, ternary, `&&`/`\|\|`/`??` |
| Ruby | `.rb` | `if`, `elsif`, `unless`, `case`/`when`, `while`, `for`, `rescue`, ternary |
| Rust | `.rs` | `if`, `for`, `while`, `loop`, `match arm` |
| C | `.c`, `.h` | `if`, `for`, `while`, `do`, `switch case`, `&&`/`\|\|` |
| C++ | `.cpp`, `.cc`, `.hpp` | `if`, `for`, range-for, `while`, `do`, `switch case`, `catch`, ternary, `&&`/`\|\|` |
| PHP | `.php` | `if`, `elseif`, `for`, `foreach`, `while`, `do`, `switch case`, `catch`, `match`, ternary, `&&`/`\|\|`/`??` |

---

## Real Examples

### Python

```bash
polycrap --path src/ --lcov coverage.lcov
# or without coverage (all functions scored at 0%):
polycrap --path src/mymodule.py --lang python
```

**Sample output** (no coverage file, all scored at 0%):

```
┌───┬──────┬────┬─────────────────┬──────────────┬─────────────────────┐
│   ┆ CRAP ┆ CC ┆ Coverage        ┆ Function     ┆ Location            │
╞═══╪══════╪════╪═════════════════╪══════════════╪═════════════════════╡
│ ✗ ┆ 56.0 ┆  7 ┆ ░░░░░░░░░░    — ┆ complex_func ┆ sample.py:4         │
├╌╌╌┼╌╌╌╌╌╌┼╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┤
│ ▲ ┆ 20.0 ┆  4 ┆ ░░░░░░░░░░    — ┆ with_boolean ┆ sample.py:16        │
├╌╌╌┼╌╌╌╌╌╌┼╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┤
│ ✓ ┆  2.0 ┆  1 ┆ ░░░░░░░░░░    — ┆ simple       ┆ sample.py:1         │
└───┴──────┴────┴─────────────────┴──────────────┴─────────────────────┘
✗ 1/3 function(s) exceed CRAP threshold 30.
```

Reading the result: `complex_func` has CC=7 (1 base + 6 branches). Untested (0% coverage): CRAP = 7² × 1 + 7 = **56**. Needs either simplification or tests.

### Java

```bash
# Generate coverage with JaCoCo → export as LCOV, then:
polycrap --path src/main/java/ --lcov jacoco.lcov
# Quick complexity audit without coverage:
polycrap --path OrderProcessor.java --lang java
```

**Sample output**:

```
┌───┬──────┬────┬─────────────────┬────────────────┬────────────────────────┐
│   ┆ CRAP ┆ CC ┆ Coverage        ┆ Function       ┆ Location               │
╞═══╪══════╪════╪═════════════════╪════════════════╪════════════════════════╡
│ ✗ ┆ 90.0 ┆  9 ┆ ░░░░░░░░░░    — ┆ processPayment ┆ OrderProcessor.java:18 │
├╌╌╌┼╌╌╌╌╌╌┼╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┤
│ ▲ ┆ 20.0 ┆  4 ┆ ░░░░░░░░░░    — ┆ calculateTotal ┆ OrderProcessor.java:3  │
├╌╌╌┼╌╌╌╌╌╌┼╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┤
│ ▲ ┆ 12.0 ┆  3 ┆ ░░░░░░░░░░    — ┆ validate       ┆ OrderProcessor.java:44 │
├╌╌╌┼╌╌╌╌╌╌┼╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┤
│ ✓ ┆  2.0 ┆  1 ┆ ░░░░░░░░░░    — ┆ processPaypal  ┆ OrderProcessor.java:48 │
└───┴──────┴────┴─────────────────┴────────────────┴────────────────────────┘
✗ 1/4 function(s) exceed CRAP threshold 30.
```

`processPayment` has CC=9 (null check, card branch, high-value check, VIP check, else-if paypal, catch, else-if crypto, two `||` operators). Score: 9² + 9 = **90**. Split it up.

### TypeScript

```bash
# With Jest/Istanbul coverage:
jest --coverage --coverageReporters=lcov
polycrap --path src/ --lcov coverage/lcov.info

# Quick audit:
polycrap --path api/auth.ts --lang typescript
```

**Sample output**:

```
┌───┬──────┬────┬─────────────────┬────────────────┬──────────────────┐
│   ┆ CRAP ┆ CC ┆ Coverage        ┆ Function       ┆ Location         │
╞═══╪══════╪════╪═════════════════╪════════════════╪══════════════════╡
│ ✗ ┆ 56.0 ┆  7 ┆ ░░░░░░░░░░    — ┆ authenticate   ┆ auth.ts:7        │
├╌╌╌┼╌╌╌╌╌╌┼╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┤
│ ✗ ┆ 42.0 ┆  6 ┆ ░░░░░░░░░░    — ┆ <anonymous:22> ┆ auth.ts:22       │
├╌╌╌┼╌╌╌╌╌╌┼╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┤
│ ▲ ┆ 12.0 ┆  3 ┆ ░░░░░░░░░░    — ┆ transform      ┆ auth.ts:40       │
└───┴──────┴────┴─────────────────┴────────────────┴──────────────────┘
✗ 2/6 function(s) exceed CRAP threshold 30.
```

Note `<anonymous:22>` — polycrap detects unnamed arrow functions by line number. The `fetchData` async arrow function (with retry logic, status checks, and catch) accumulates CC=6, scoring 42.

---

## Installation

```bash
# From source (requires Rust + a C compiler for the tree-sitter grammars)
git clone https://github.com/az9713/polyglot-crap
cd polyglot-crap/polycrap
cargo install --path .

# Verify
polycrap --version
```

**Prerequisites:** A C compiler (`cl.exe` on Windows, `gcc`/`clang` on Linux/macOS). The tree-sitter grammar crates compile C via the `cc` crate build script — no system-level tree-sitter installation needed.

---

## Usage

```bash
# Analyze a directory (auto-detects language from extension)
polycrap --path src/

# Force a language (useful for files without standard extensions)
polycrap --path legacy_code/ --lang python

# With LCOV coverage file
polycrap --path src/ --lcov coverage/lcov.info

# Top 10 worst functions
polycrap --path src/ --lcov lcov.info --top 10

# CI gate: exit 1 if any function exceeds threshold
polycrap --path src/ --lcov lcov.info --fail-above

# Save baseline, then detect regressions
polycrap --path src/ --lcov lcov.info --format json --output baseline.json
# ... make changes, generate new lcov.info ...
polycrap --path src/ --lcov lcov.info --baseline baseline.json --fail-regression

# Multiple output formats
polycrap --path src/ --format json          # machine-readable
polycrap --path src/ --format markdown      # for PR artifacts
polycrap --path src/ --format sarif         # GitHub Code Scanning
polycrap --path src/ --format github        # GitHub Actions annotations
polycrap --path src/ --format pr-comment    # opinionated PR comment

# Exclude generated files
polycrap --path src/ --exclude 'generated/**' --exclude 'vendor/**'

# Suppress known-noisy functions
polycrap --path src/ --allow 'OrderProcessor::*' --allow 'legacy_*'
```

### Configuration file

Create `.polycrap.toml` at your project root:

```toml
threshold = 30.0
missing = "pessimistic"   # pessimistic | optimistic | skip
fail_above = true
epsilon = 0.01

exclude = ["generated/**", "vendor/**", "tests/fixtures/**"]
allow = ["legacy_*"]
```

---

## The Test Suite — 105 Tests

```
cargo test --all-targets

test result: ok. 98 passed   ← library unit tests (src/*)
test result: ok.  5 passed   ← CLI unit tests (src/main.rs)
test result: ok.  1 passed   ← debug_java integration test
test result: ok.  1 passed   ← all_language_queries_are_valid
                   ─────────
                 105 total, 0 failed
```

### What the tests cover

#### 98 library unit tests (`src/`)

These are the tests inherited from cargo-crap and adapted for polycrap. They test every module in isolation:

| Module | Tests | What they verify |
|---|---|---|
| `score.rs` | 7 | CRAP formula — lower bound (CC=1, 100% → 1.0), published example (CC=6, 0% → 42.0), monotonicity in CC and coverage, threshold boundary, clamp behavior |
| `coverage.rs` | ~8 | LCOV parsing — SF/DA/end_of_record parsing, multi-file LCOV, DA line accumulation |
| `merge.rs` | ~15 | Path matching — absolute paths, relative LCOV paths via suffix matching, the critical invariant that relative paths are never canonicalized against CWD |
| `delta.rs` | ~12 | Baseline comparison — exact match, name-only fallback (moved functions), ambiguous names stay unpaired, DeltaStatus classification |
| `report/human.rs` | ~8 | Human table formatting — grade symbols (✓/▲/✗), coverage bar rendering |
| `report/json.rs` | ~6 | JSON envelope — schema URL, version field, roundtrip deserialize |
| `report/github.rs` | ~5 | GitHub Actions `::warning` format, percent-encoding of special chars |
| `report/markdown.rs` | ~8 | GFM table structure, link generation |
| `report/pr_comment.rs` | ~12 | PR comment marker (`<!-- polycrap-report -->`), section capping, `<details>` collapse, longest-common-prefix computation |
| `report/sarif.rs` | ~6 | SARIF 2.1.0 structure — schema field, driver name `"polycrap"`, result per crappy function |
| `report/summary.rs` | ~4 | Summary mode — aggregate counts, delta summary |
| `report/types.rs` | ~5 | Grade classification, coverage bar at 0%/50%/100%/None |
| `report.rs` (dispatcher) | ~2 | crappy_count, PR comment marker |

#### 5 CLI unit tests (`src/main.rs`)

Test the `--allow` glob classification logic:
- `is_path_allow_pattern` correctly distinguishes `src/generated/**` (path glob) from `Foo::*` (name glob)
- `path_set_matches_suffix` correctly matches relative patterns against absolute paths
- Empty sets are no-ops

#### 1 debug_java integration test (`tests/debug_java.rs`)

Writes a Java class to a temp file, runs `analyze_file_with_lang`, and asserts at least one function is found with a non-trivial CC. Added during development to diagnose the `switch_case`→`switch_block_statement_group` node-type bug.

#### 1 all_language_queries_are_valid test (`tests/query_validation.rs`)

The most important test added by polycrap. It runs all 12 languages through a trivial snippet and verifies:
1. The `fn_query` and `cc_query` compile without errors (catches typos in node type names)
2. At least one function is found in each snippet
3. CC is at least 2 (the if-branch is counted)

This is the **regression guard for query correctness**. During development it caught three real bugs:
- `switch_case` in `java_cc.scm` — Java uses `switch_block_statement_group`
- `logical_expression` in `javascript_cc.scm` — JavaScript uses `binary_expression`
- `elseif_clause` in `php_cc.scm` — PHP uses `else_if_clause`
- `switch_case` in `c_cc.scm` and `cpp_cc.scm` — C/C++ use `case_statement`

### How to interpret results

| Symbol | Meaning | Action |
|---|---|---|
| ✓ | CRAP ≤ threshold/3 | Clean — no action needed |
| ▲ | threshold/3 < CRAP ≤ threshold | Moderate — worth watching, add tests before it grows |
| ✗ | CRAP > threshold (default 30) | Crappy — either add tests or refactor to reduce CC |

**Coverage column**: the `░░░░░░░░░░` bar shows 0-100% in 10 blocks. A dash (`—`) means no coverage data was found for that file — this is treated as 0% with `--missing pessimistic` (the default), 100% with `--missing optimistic`, or the function is skipped entirely with `--missing skip`.

---

## Development Journey

```
YouTube video: "Is Your AI Code Producing CRAP?"
        ↓
Study cargo-crap architecture — discover 80% is language-agnostic
        ↓
Research: can tree-sitter replace the syn AST walker?
        ↓
Feasibility doc: YES — universal parser, S-expression queries, all languages
        ↓
Plan: new standalone binary "polycrap", all 12 languages from day 1
        ↓
Copy 7 cargo-crap modules verbatim (coverage, merge, score, delta, report/*)
Adapt 3 modules (config, report.rs, main.rs — change names/markers)
        ↓
Write language.rs — LanguageConfig registry, LANGUAGE.into() for each grammar
Write 24 query files — 12 languages × (fn.scm + cc.scm)
Write complexity/walker.rs — StreamingIterator loop, build_name, strip_receiver
Write complexity/mod.rs — analyze_file, analyze_tree with force_lang
Write lib.rs, main.rs — wire it all together, add --lang flag
        ↓
Fix ABI mismatch: tree-sitter-c-sharp 0.23 uses grammar ABI v15;
upgrade tree-sitter core 0.24→0.25 (supports ABI 13–15)
        ↓
Fix 4 query bugs caught by query_validation test:
  java: switch_case → switch_block_statement_group
  js:   logical_expression → binary_expression
  php:  elseif_clause → else_if_clause
  c/c++: switch_case → case_statement
        ↓
105 tests, 0 failures ✓
```

---

## Relation to cargo-crap

polycrap is a separate tool, not a fork. It shares the pipeline architecture and several verbatim-copied modules (with attribution) from [cargo-crap](https://github.com/minikin/cargo-crap) by Mykhailo Mykhailiuk, used under the MIT license.

The key differences:

| | cargo-crap | polycrap |
|---|---|---|
| Languages | Rust only | 12 languages |
| AST | `syn` crate | `tree-sitter` |
| Config file | `.cargo-crap.toml` | `.polycrap.toml` |
| Cargo integration | `cargo crap` subcommand, `--workspace` | Standalone binary |
| Language override | N/A | `--lang <name>` |

---

## License

MIT
