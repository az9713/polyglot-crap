//! `--summary` — aggregate-only output for any non-JSON/non-GitHub format.

use super::per_crate::{has_crate_data, write_per_crate_human};
use crate::delta::{DeltaReport, DeltaStatus};
use crate::merge::CrapEntry;
use anyhow::Result;
use owo_colors::OwoColorize;
use std::io::Write;

/// Print only aggregate statistics — no per-function table.
pub fn render_summary(
    entries: &[CrapEntry],
    threshold: f64,
    out: &mut dyn Write,
) -> Result<()> {
    if has_crate_data(entries) {
        write_per_crate_human(entries, threshold, out)?;
    }
    let total = entries.len();
    let crappy = super::crappy_count(entries, threshold);
    let worst = entries.first();

    if crappy == 0 {
        writeln!(
            out,
            "{} Analyzed: {} · Crappy: 0 (threshold {})",
            "✓".green(),
            total,
            threshold,
        )?;
    } else {
        let worst_str = worst
            .map(|e| format!(" · Worst: {} (CRAP {:.1})", e.function, e.crap))
            .unwrap_or_default();
        writeln!(
            out,
            "{} Analyzed: {} · Crappy: {} (threshold {}){worst_str}",
            "✗".red(),
            total,
            crappy,
            threshold,
        )?;
    }
    Ok(())
}

/// Print only aggregate delta statistics — no per-function table.
pub fn render_delta_summary(
    report: &DeltaReport,
    out: &mut dyn Write,
) -> Result<()> {
    let regressed = report
        .entries
        .iter()
        .filter(|e| e.status == DeltaStatus::Regressed)
        .count();
    let improved = report
        .entries
        .iter()
        .filter(|e| e.status == DeltaStatus::Improved)
        .count();
    let new = report
        .entries
        .iter()
        .filter(|e| e.status == DeltaStatus::New)
        .count();
    let moved = report
        .entries
        .iter()
        .filter(|e| e.status == DeltaStatus::Moved)
        .count();
    let unchanged = report
        .entries
        .iter()
        .filter(|e| e.status == DeltaStatus::Unchanged)
        .count();
    writeln!(
        out,
        "{}  {}  {}  {}  {}  {}",
        format!("↑ {regressed} regressed").red(),
        format!("↓ {improved} improved").green(),
        format!("★ {new} new").yellow(),
        format!("↔ {moved} moved").cyan(),
        format!("· {unchanged} unchanged").dimmed(),
        format!("— {} removed", report.removed.len()).dimmed(),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn entry(
        crate_name: Option<&str>,
        function: &str,
        crap: f64,
    ) -> CrapEntry {
        CrapEntry {
            file: PathBuf::from("src/lib.rs"),
            function: function.into(),
            line: 1,
            cyclomatic: 1.0,
            coverage: Some(100.0),
            crap,
            crate_name: crate_name.map(std::string::ToString::to_string),
        }
    }

    #[test]
    fn render_summary_leads_with_per_crate_table_for_workspace() {
        let entries = vec![entry(Some("alpha"), "a1", 1.0)];
        let mut buf = Vec::new();
        render_summary(&entries, 30.0, &mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(s.contains("Per-crate summary:"));
        assert!(s.contains("Analyzed: 1"));
    }

    #[test]
    fn render_summary_skips_per_crate_when_not_workspace() {
        let entries = vec![entry(None, "a1", 1.0)];
        let mut buf = Vec::new();
        render_summary(&entries, 30.0, &mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(!s.contains("Per-crate summary"));
        assert!(s.contains("Analyzed: 1"));
    }

    #[test]
    fn render_delta_summary_counts_moved_correctly() {
        use crate::delta::{DeltaEntry, DeltaReport, DeltaStatus};
        let mk_entry = |fn_name: &str, status: DeltaStatus| DeltaEntry {
            current: CrapEntry {
                file: PathBuf::from("src/a.rs"),
                function: fn_name.into(),
                line: 1,
                cyclomatic: 1.0,
                coverage: Some(100.0),
                crap: 1.0,
                crate_name: None,
            },
            baseline_crap: Some(1.0),
            delta: Some(0.0),
            status,
            previous_file: None,
        };
        let report = DeltaReport {
            entries: vec![
                mk_entry("moved", DeltaStatus::Moved),
                mk_entry("u1", DeltaStatus::Unchanged),
                mk_entry("u2", DeltaStatus::Unchanged),
                mk_entry("u3", DeltaStatus::Unchanged),
            ],
            removed: vec![],
        };
        let mut buf = Vec::new();
        render_delta_summary(&report, &mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(
            s.contains("↔ 1 moved"),
            "summary must report 1 moved, not 3:\n{s}"
        );
    }
}
