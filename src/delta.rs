//! Delta comparison between two polycrap runs.
//!
//! Load a previous run's JSON output with [`load_baseline`], then call
//! [`compute_delta`] to get per-function change status.
//!
//! ## Typical CI workflow
//!
//! ```text
//! # On main branch — save baseline
//! polycrap --lcov lcov.info --format json --output baseline.json
//!
//! # On a PR branch — compare and fail on regressions
//! polycrap --lcov lcov.info --baseline baseline.json --fail-regression
//! ```

use crate::merge::CrapEntry;
use anyhow::{Context, Result};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Default tolerance for regression detection.
pub const DEFAULT_EPSILON: f64 = 0.01;

/// Change status of a single function relative to the baseline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DeltaStatus {
    Regressed,
    Improved,
    New,
    Unchanged,
    Moved,
}

/// One function from the current run, annotated with its change since the baseline.
#[derive(Debug, Clone, Serialize)]
pub struct DeltaEntry {
    #[serde(flatten)]
    pub current: CrapEntry,
    pub baseline_crap: Option<f64>,
    pub delta: Option<f64>,
    pub status: DeltaStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_file: Option<PathBuf>,
}

/// A function present in the baseline but absent in the current run.
#[derive(Debug, Clone, Serialize)]
pub struct RemovedEntry {
    pub function: String,
    pub file: PathBuf,
    pub baseline_crap: f64,
}

/// The full comparison result.
#[derive(Debug)]
pub struct DeltaReport {
    pub entries: Vec<DeltaEntry>,
    pub removed: Vec<RemovedEntry>,
}

impl DeltaReport {
    #[must_use]
    pub fn regression_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| e.status == DeltaStatus::Regressed)
            .count()
    }
}

/// Load a JSON baseline produced by a previous `polycrap --format json` run.
pub fn load_baseline(path: &Path) -> Result<Vec<CrapEntry>> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("reading baseline {}", path.display()))?;
    let envelope: crate::report::Envelope = serde_json::from_str(&raw).with_context(|| {
        format!(
            "parsing baseline {} — must be JSON from `polycrap --format json`",
            path.display()
        )
    })?;
    Ok(envelope.entries)
}

fn path_key(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

fn classify_score(
    delta: f64,
    epsilon: f64,
) -> DeltaStatus {
    if delta > epsilon {
        DeltaStatus::Regressed
    } else if delta < -epsilon {
        DeltaStatus::Improved
    } else {
        DeltaStatus::Unchanged
    }
}

fn build_pass_one_entry(
    e: &CrapEntry,
    baseline_entry: Option<&CrapEntry>,
    epsilon: f64,
) -> DeltaEntry {
    let (baseline_crap, delta, status) = match baseline_entry {
        None => (None, None, DeltaStatus::New),
        Some(b) => {
            let d = e.crap - b.crap;
            (Some(b.crap), Some(d), classify_score(d, epsilon))
        },
    };
    DeltaEntry {
        current: e.clone(),
        baseline_crap,
        delta,
        status,
        previous_file: None,
    }
}

fn pass_one_exact(
    current: &[CrapEntry],
    baseline: &[CrapEntry],
    epsilon: f64,
) -> (Vec<DeltaEntry>, HashSet<(String, String)>) {
    let baseline_index: HashMap<(String, String), &CrapEntry> = baseline
        .iter()
        .map(|e| ((path_key(&e.file), e.function.clone()), e))
        .collect();
    let mut matched: HashSet<(String, String)> = HashSet::new();
    let entries = current
        .iter()
        .map(|e| {
            let key = (path_key(&e.file), e.function.clone());
            let baseline_entry = baseline_index.get(&key).copied();
            if baseline_entry.is_some() {
                matched.insert(key);
            }
            build_pass_one_entry(e, baseline_entry, epsilon)
        })
        .collect();
    (entries, matched)
}

fn apply_move_pairing(
    entry: &mut DeltaEntry,
    baseline_entry: &CrapEntry,
    epsilon: f64,
) {
    let d = entry.current.crap - baseline_entry.crap;
    let score_status = classify_score(d, epsilon);
    entry.baseline_crap = Some(baseline_entry.crap);
    entry.delta = Some(d);
    entry.previous_file = Some(baseline_entry.file.clone());
    entry.status = match score_status {
        DeltaStatus::Unchanged => DeltaStatus::Moved,
        other => other,
    };
}

fn pass_two_name_fallback(
    entries: &mut [DeltaEntry],
    baseline: &[CrapEntry],
    matched: &mut HashSet<(String, String)>,
    epsilon: f64,
) {
    let mut new_idx_by_name: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, de) in entries.iter().enumerate() {
        if de.status == DeltaStatus::New {
            new_idx_by_name
                .entry(de.current.function.clone())
                .or_default()
                .push(i);
        }
    }
    let mut baseline_unmatched_by_name: HashMap<String, Vec<&CrapEntry>> = HashMap::new();
    for e in baseline {
        let key = (path_key(&e.file), e.function.clone());
        if !matched.contains(&key) {
            baseline_unmatched_by_name
                .entry(e.function.clone())
                .or_default()
                .push(e);
        }
    }
    for (name, new_idxs) in &new_idx_by_name {
        if new_idxs.len() != 1 {
            continue;
        }
        let Some(baseline_group) = baseline_unmatched_by_name.get(name) else {
            continue;
        };
        if baseline_group.len() != 1 {
            continue;
        }
        let baseline_entry = baseline_group[0];
        apply_move_pairing(&mut entries[new_idxs[0]], baseline_entry, epsilon);
        matched.insert((
            path_key(&baseline_entry.file),
            baseline_entry.function.clone(),
        ));
    }
}

fn collect_removed(
    baseline: &[CrapEntry],
    matched: &HashSet<(String, String)>,
) -> Vec<RemovedEntry> {
    baseline
        .iter()
        .filter(|e| !matched.contains(&(path_key(&e.file), e.function.clone())))
        .map(|e| RemovedEntry {
            function: e.function.clone(),
            file: e.file.clone(),
            baseline_crap: e.crap,
        })
        .collect()
}

/// Join current results against a baseline and compute per-function deltas.
#[must_use]
pub fn compute_delta(
    current: &[CrapEntry],
    baseline: &[CrapEntry],
    epsilon: f64,
) -> DeltaReport {
    let (mut entries, mut matched) = pass_one_exact(current, baseline, epsilon);
    pass_two_name_fallback(&mut entries, baseline, &mut matched, epsilon);
    let removed = collect_removed(baseline, &matched);
    DeltaReport { entries, removed }
}

#[cfg(test)]
#[expect(
    clippy::float_cmp,
    reason = "CRAP-score deltas are deterministic floats; exact equality is the right comparison"
)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn entry(
        function: &str,
        crap: f64,
    ) -> CrapEntry {
        CrapEntry {
            file: PathBuf::from("src/lib.py"),
            function: function.to_string(),
            line: 1,
            cyclomatic: 1.0,
            coverage: Some(100.0),
            crap,
            crate_name: None,
        }
    }

    #[test]
    fn new_when_not_in_baseline() {
        let report = compute_delta(&[entry("foo", 5.0)], &[], DEFAULT_EPSILON);
        assert_eq!(report.entries[0].status, DeltaStatus::New);
        assert!(report.entries[0].baseline_crap.is_none());
    }

    #[test]
    fn regressed_when_score_increased() {
        let report = compute_delta(&[entry("foo", 10.0)], &[entry("foo", 5.0)], DEFAULT_EPSILON);
        assert_eq!(report.entries[0].status, DeltaStatus::Regressed);
        assert_eq!(report.entries[0].baseline_crap, Some(5.0));
    }

    #[test]
    fn improved_when_score_decreased() {
        let report = compute_delta(&[entry("foo", 3.0)], &[entry("foo", 8.0)], DEFAULT_EPSILON);
        assert_eq!(report.entries[0].status, DeltaStatus::Improved);
    }

    #[test]
    fn unchanged_within_epsilon() {
        let report = compute_delta(
            &[entry("foo", 5.005)],
            &[entry("foo", 5.0)],
            DEFAULT_EPSILON,
        );
        assert_eq!(report.entries[0].status, DeltaStatus::Unchanged);
    }

    #[test]
    fn removed_entries_identified() {
        let report = compute_delta(
            &[entry("bar", 2.0)],
            &[entry("foo", 5.0), entry("bar", 2.0)],
            DEFAULT_EPSILON,
        );
        assert_eq!(report.removed.len(), 1);
        assert_eq!(report.removed[0].function, "foo");
    }

    #[test]
    fn functions_in_different_files_pair_as_moved() {
        let current = vec![CrapEntry {
            file: PathBuf::from("src/lib.py"),
            function: "foo".into(),
            line: 1,
            cyclomatic: 1.0,
            coverage: Some(100.0),
            crap: 5.0,
            crate_name: None,
        }];
        let baseline = vec![CrapEntry {
            file: PathBuf::from("src/main.py"),
            function: "foo".into(),
            line: 1,
            cyclomatic: 1.0,
            coverage: Some(100.0),
            crap: 5.0,
            crate_name: None,
        }];
        let report = compute_delta(&current, &baseline, DEFAULT_EPSILON);
        assert_eq!(report.entries[0].status, DeltaStatus::Moved);
        assert_eq!(
            report.entries[0].previous_file,
            Some(PathBuf::from("src/main.py"))
        );
    }
}
