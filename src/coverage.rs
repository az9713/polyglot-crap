//! Parse LCOV coverage reports into a per-file, per-line hit map.
//!
//! LCOV is the common output format for many coverage tools across languages:
//! Python's `coverage.py --lcov`, JavaScript's Istanbul/Jest/Vitest with
//! lcov reporter, `gcov2lcov` for Go, JaCoCo for Java, etc.
//!
//! A minimal record looks like:
//!
//! ```text
//! SF:src/foo.py          ← source file
//! DA:43,7                ← line 43 was executed 7 times
//! DA:44,0                ← line 44 was reachable but never executed
//! end_of_record
//! ```
//!
//! We only consume `SF`, `DA`, and `end_of_record`. Function-level records
//! (`FN`/`FNDA`) are tempting but unreliable: they tell us where a function
//! *starts* but not where it *ends*, so we can't compute coverage of the
//! function's body from them. Instead, we intersect the line-level `DA`
//! records with spans we already have from the AST.

use anyhow::{Context, Result};
use lcov::reader::Error as LcovReadError;
use lcov::record::ParseRecordError;
use lcov::{Reader, Record};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

/// Per-file coverage, indexed by line number.
///
/// Only lines that appear in a `DA` record are tracked — these are the
/// "executable" lines per the coverage tool's mapping. Blank lines, comments,
/// and purely declarative lines do not appear here, and we treat them as
/// "not applicable" rather than "uncovered".
#[derive(Debug, Default, Clone)]
pub struct FileCoverage {
    /// Line number (1-indexed) → hit count.
    pub lines: BTreeMap<u32, u64>,
}

impl FileCoverage {
    /// Percentage of executable lines in `[start..=end]` that were hit at
    /// least once.
    ///
    /// Returns 100.0 if no executable lines fall inside the span. A function
    /// composed entirely of declarative code genuinely has nothing to cover
    /// and should not be penalized.
    #[must_use]
    pub fn coverage_in_span(
        &self,
        start: usize,
        end: usize,
    ) -> f64 {
        let start = start as u32;
        let end = end as u32;
        let executable: Vec<_> = self.lines.range(start..=end).collect();
        if executable.is_empty() {
            return 100.0;
        }
        let covered = executable.iter().filter(|(_, hits)| **hits > 0).count();
        (covered as f64 / executable.len() as f64) * 100.0
    }
}

/// Parse an LCOV file into a map keyed by the source paths it declares.
///
/// **Path normalization is deliberately NOT done here.** Paths in LCOV may
/// be absolute, relative to the CWD at the time coverage was generated, or
/// relative to the project root. The caller is responsible for matching
/// them against the paths [`crate::complexity`] produces — see
/// [`crate::merge`].
pub fn parse_lcov(path: &Path) -> Result<HashMap<PathBuf, FileCoverage>> {
    let reader =
        Reader::open_file(path).with_context(|| format!("opening LCOV file {}", path.display()))?;

    let mut files: HashMap<PathBuf, FileCoverage> = HashMap::new();
    let mut current_path: Option<PathBuf> = None;

    for record in reader {
        let record = match record {
            Ok(r) => r,
            Err(LcovReadError::ParseRecord(_, ParseRecordError::UnknownRecord)) => continue,
            Err(e) => {
                return Err(
                    anyhow::Error::new(e).context(format!("parsing record in {}", path.display()))
                );
            },
        };
        match record {
            Record::SourceFile { path: sf_path } => {
                current_path = Some(sf_path.clone());
                files.entry(sf_path).or_default();
            },
            Record::LineData { line, count, .. } => {
                if let Some(ref p) = current_path
                    && let Some(fc) = files.get_mut(p)
                {
                    *fc.lines.entry(line).or_insert(0) += count;
                }
            },
            Record::EndOfRecord => {
                current_path = None;
            },
            _ => {},
        }
    }

    Ok(files)
}

#[cfg(test)]
#[expect(
    clippy::float_cmp,
    reason = "coverage % is computed from integer line counts; exact equality is the right comparison"
)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::Path;

    #[test]
    fn unknown_lcov_records_are_skipped_not_fatal() {
        let f = write_lcov(
            "VER:2\nTN:\nSF:src/foo.py\nDA:10,3\nUNKNOWN_RECORD:whatever\nend_of_record\n",
        );
        let result = parse_lcov(f.path()).expect("unknown records must not be fatal");
        let cov = result
            .get(Path::new("src/foo.py"))
            .expect("src/foo.py in result");
        assert_eq!(cov.lines[&10], 3);
    }

    fn write_lcov(content: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().expect("tempfile");
        f.write_all(content.as_bytes()).expect("write");
        f
    }

    #[test]
    fn parse_lcov_reads_correct_file_and_hit_counts() {
        let f = write_lcov("TN:\nSF:src/foo.py\nDA:10,3\nDA:11,0\nend_of_record\n");
        let result = parse_lcov(f.path()).expect("parse_lcov");

        let cov = result
            .get(Path::new("src/foo.py"))
            .expect("src/foo.py must be in result");
        assert_eq!(cov.lines[&10], 3, "line 10 should have 3 hits");
        assert_eq!(cov.lines[&11], 0, "line 11 should have 0 hits");
    }

    #[test]
    fn parse_lcov_accumulates_duplicate_line_entries() {
        let f = write_lcov("TN:\nSF:src/foo.py\nDA:10,2\nDA:10,3\nend_of_record\n");
        let result = parse_lcov(f.path()).expect("parse_lcov");
        assert_eq!(
            result[Path::new("src/foo.py")].lines[&10],
            5,
            "duplicate DA entries must be summed"
        );
    }

    #[test]
    fn parse_lcov_isolates_multiple_source_files() {
        let f = write_lcov(
            "TN:\nSF:src/a.py\nDA:1,1\nend_of_record\nSF:src/b.py\nDA:2,4\nend_of_record\n",
        );
        let result = parse_lcov(f.path()).expect("parse_lcov");

        let a = result.get(Path::new("src/a.py")).expect("a.py in result");
        let b = result.get(Path::new("src/b.py")).expect("b.py in result");

        assert_eq!(a.lines[&1], 1);
        assert_eq!(b.lines[&2], 4);
        assert!(!b.lines.contains_key(&1));
        assert!(!a.lines.contains_key(&2));
    }

    fn fc_from(lines: &[(u32, u64)]) -> FileCoverage {
        FileCoverage {
            lines: lines.iter().copied().collect(),
        }
    }

    #[test]
    fn empty_span_yields_full_coverage() {
        let fc = fc_from(&[(5, 1), (25, 1)]);
        assert_eq!(fc.coverage_in_span(10, 20), 100.0);
    }

    #[test]
    fn all_executable_lines_hit_is_100_percent() {
        let fc = fc_from(&[(10, 3), (11, 3), (12, 1)]);
        assert_eq!(fc.coverage_in_span(10, 12), 100.0);
    }

    #[test]
    fn half_hit_is_50_percent() {
        let fc = fc_from(&[(10, 5), (11, 0), (12, 1), (13, 0)]);
        assert_eq!(fc.coverage_in_span(10, 13), 50.0);
    }
}
