//! Join complexity data (per-function) with coverage data (per-file) into
//! CRAP entries.
//!
//! ## The path-matching problem
//!
//! This is where the silent failure mode lives. The complexity pass gives
//! us absolute paths (whatever was passed to `analyze_tree`). LCOV files
//! can contain:
//!
//! 1. **Absolute paths**  — `/home/alice/project/src/foo.py`
//! 2. **Project-relative paths** — `src/foo.py`
//! 3. **Paths with `./` or `../` components** — `./src/foo.py`
//!
//! Our strategy: build a lookup keyed on **canonicalized suffix matches**.
//! For every coverage path we can't canonicalize (because it's relative),
//! we try progressively shorter suffixes against canonical complexity paths.

use crate::complexity::FunctionComplexity;
use crate::coverage::FileCoverage;
use crate::score::crap;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// One row in the final report.
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct CrapEntry {
    pub file: PathBuf,
    pub function: String,
    pub line: usize,
    pub cyclomatic: f64,
    /// Percentage; may be `None` if we could not find coverage data for
    /// this file at all. That's different from "0% covered" — it means the
    /// coverage report didn't mention the file.
    pub coverage: Option<f64>,
    pub crap: f64,
    /// Language tag set when the entry's file was tagged during analysis.
    /// Always `None` in non-tagged runs.
    #[serde(rename = "crate", default, skip_serializing_if = "Option::is_none")]
    pub crate_name: Option<String>,
}

/// How to treat functions we have complexity data for but no coverage data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MissingCoveragePolicy {
    /// Assume 0% coverage. Pessimistic — good for CI gates, where unmapped
    /// files are a red flag worth surfacing.
    Pessimistic,
    /// Assume 100% coverage. Optimistic — suitable for interactive use where
    /// you've scoped coverage to a subset of the tree intentionally.
    Optimistic,
    /// Skip the function entirely; don't emit a row.
    Skip,
}

/// Output of [`merge`]: the scored entries plus any source files that had no
/// matching entry in the LCOV report.
pub struct MergeResult {
    /// CRAP entries sorted by score descending.
    pub entries: Vec<CrapEntry>,
    /// Source files for which no coverage data could be found in the LCOV
    /// report. Only populated when a non-empty coverage map was provided.
    pub unmapped_files: Vec<PathBuf>,
}

/// Merge complexity and coverage data into a sorted [`MergeResult`]
/// (entries ranked highest score first).
#[expect(
    clippy::needless_pass_by_value,
    reason = "callers always have a fresh HashMap they don't reuse; taking by value matches the consuming pipeline"
)]
#[must_use]
pub fn merge(
    complexity: Vec<FunctionComplexity>,
    coverage: HashMap<PathBuf, FileCoverage>,
    policy: MissingCoveragePolicy,
) -> MergeResult {
    let index = PathIndex::build(&coverage);
    let has_coverage = !coverage.is_empty();

    let mut mapped_files: HashSet<PathBuf> = HashSet::new();
    let mut seen_files: HashSet<PathBuf> = HashSet::new();

    let mut entries: Vec<CrapEntry> = complexity
        .into_iter()
        .filter_map(|fc| {
            let cov = index
                .lookup(&fc.file)
                .map(|cov_file| cov_file.coverage_in_span(fc.start_line, fc.end_line));

            if has_coverage {
                if cov.is_some() {
                    mapped_files.insert(fc.file.clone());
                }
                seen_files.insert(fc.file.clone());
            }

            let cov_for_scoring = match (cov, policy) {
                (Some(c), _) => c,
                (None, MissingCoveragePolicy::Pessimistic) => 0.0,
                (None, MissingCoveragePolicy::Optimistic) => 100.0,
                (None, MissingCoveragePolicy::Skip) => return None,
            };

            let crap_score = crap(fc.cyclomatic, cov_for_scoring);
            Some(CrapEntry {
                file: fc.file,
                function: fc.name,
                line: fc.start_line,
                cyclomatic: fc.cyclomatic,
                coverage: cov,
                crap: crap_score,
                crate_name: None,
            })
        })
        .collect();

    entries.sort_by(|a, b| {
        b.crap
            .partial_cmp(&a.crap)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut unmapped_files: Vec<PathBuf> = seen_files
        .into_iter()
        .filter(|f| !mapped_files.contains(f))
        .collect();
    unmapped_files.sort();

    MergeResult {
        entries,
        unmapped_files,
    }
}

/// A path lookup index that handles absolute-vs-relative mismatches between
/// the complexity pass (which has whatever was on the command line) and the
/// coverage file (which has whatever the coverage tool decided to write).
struct PathIndex<'a> {
    /// Canonicalized absolute paths → coverage data. Fast path.
    by_absolute: HashMap<PathBuf, &'a FileCoverage>,
    /// Original (possibly relative) paths kept for suffix matching.
    by_relative: Vec<(PathBuf, &'a FileCoverage)>,
}

impl<'a> PathIndex<'a> {
    fn build(coverage: &'a HashMap<PathBuf, FileCoverage>) -> Self {
        let mut by_absolute = HashMap::new();
        let mut by_relative = Vec::new();

        for (raw_path, cov) in coverage {
            // CRITICAL: we only canonicalize *absolute* paths here. A relative
            // path like `src/lib.py` in an LCOV file means "some file whose
            // component-suffix is this" — it must NOT be resolved against the
            // caller's CWD, because the CWD is an accident of invocation.
            if raw_path.is_absolute() {
                match raw_path.canonicalize() {
                    Ok(abs) => {
                        by_absolute.insert(abs, cov);
                    },
                    Err(_) => {
                        by_relative.push((raw_path.clone(), cov));
                    },
                }
            } else {
                by_relative.push((raw_path.clone(), cov));
            }
        }

        Self {
            by_absolute,
            by_relative,
        }
    }

    fn lookup(
        &self,
        query: &Path,
    ) -> Option<&'a FileCoverage> {
        // Fast path: direct canonical match.
        if let Ok(abs) = query.canonicalize()
            && let Some(cov) = self.by_absolute.get(&abs)
        {
            return Some(*cov);
        }

        // Slow path: suffix match.
        for (rel, cov) in &self.by_relative {
            if path_has_suffix(query, rel) {
                return Some(*cov);
            }
        }

        None
    }
}

/// True if `haystack` ends with `needle`, compared component by component.
fn path_has_suffix(
    haystack: &Path,
    needle: &Path,
) -> bool {
    let hay: Vec<_> = haystack.components().collect();
    let nee: Vec<_> = needle.components().collect();
    if nee.len() > hay.len() {
        return false;
    }
    hay[hay.len() - nee.len()..] == nee[..]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    fn cov_with(lines: &[(u32, u64)]) -> FileCoverage {
        FileCoverage {
            lines: lines.iter().copied().collect::<BTreeMap<_, _>>(),
        }
    }

    #[test]
    fn suffix_match_works_for_relative_coverage_paths() {
        let mut cov_map = HashMap::new();
        cov_map.insert(PathBuf::from("src/foo.py"), cov_with(&[(10, 1), (11, 1)]));
        let index = PathIndex::build(&cov_map);

        let complexity_path = PathBuf::from("/home/alice/project/src/foo.py");
        let result = index.lookup(&complexity_path);
        assert!(result.is_some(), "expected suffix match to succeed");
    }

    #[test]
    fn suffix_match_rejects_partial_component_matches() {
        let a = PathBuf::from("/project/src/oofoo.py");
        let b = PathBuf::from("foo.py");
        assert!(!path_has_suffix(&a, &b));
    }

    #[test]
    fn equal_length_paths_match_when_identical() {
        let a = PathBuf::from("/project/src/foo.py");
        let b = PathBuf::from("/project/src/foo.py");
        assert!(path_has_suffix(&a, &b));
    }

    #[test]
    fn longer_needle_does_not_match() {
        let hay = PathBuf::from("src/foo.py");
        let needle = PathBuf::from("/abs/project/src/foo.py");
        assert!(!path_has_suffix(&hay, &needle));
    }

    #[test]
    fn merge_sorts_by_descending_crap() {
        let complexity = vec![
            FunctionComplexity {
                file: PathBuf::from("a.py"),
                name: "easy".into(),
                start_line: 1,
                end_line: 3,
                cyclomatic: 1.0,
            },
            FunctionComplexity {
                file: PathBuf::from("a.py"),
                name: "hard".into(),
                start_line: 10,
                end_line: 30,
                cyclomatic: 10.0,
            },
        ];
        let result = merge(
            complexity,
            HashMap::new(),
            MissingCoveragePolicy::Pessimistic,
        );
        assert_eq!(result.entries[0].function, "hard");
        assert_eq!(result.entries[1].function, "easy");
    }

    #[test]
    fn skip_policy_drops_rows_without_coverage() {
        let complexity = vec![FunctionComplexity {
            file: PathBuf::from("nowhere.py"),
            name: "foo".into(),
            start_line: 1,
            end_line: 5,
            cyclomatic: 3.0,
        }];
        let result = merge(complexity, HashMap::new(), MissingCoveragePolicy::Skip);
        assert!(result.entries.is_empty());
    }

    #[test]
    fn relative_coverage_paths_are_not_resolved_against_cwd() {
        let mut cov_map = HashMap::new();
        cov_map.insert(PathBuf::from("src/lib.py"), cov_with(&[(10, 1)]));
        let index = PathIndex::build(&cov_map);

        assert!(
            index.by_absolute.is_empty(),
            "relative coverage paths must not populate by_absolute"
        );
        assert_eq!(index.by_relative.len(), 1);

        let found = index.lookup(Path::new("/somewhere/else/src/lib.py"));
        assert!(found.is_some());
    }
}
