//! `--format pr-comment` — opinionated PR-comment markdown.

use super::links::{SourceLinks, linkify};
use super::types::{Grade, delta_display};
use super::write_pr_comment_marker;
use crate::delta::{DeltaEntry, DeltaReport, DeltaStatus, RemovedEntry};
use crate::merge::CrapEntry;
use anyhow::Result;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Maximum rows per section in `--format pr-comment` output.
pub(crate) const MAX_ROWS_PER_SECTION: usize = 25;

pub(crate) fn longest_common_path_prefix(paths: &[PathBuf]) -> PathBuf {
    if paths.len() < 2 {
        return PathBuf::new();
    }
    let first: Vec<_> = paths[0].components().collect();
    let mut common_len = first.len();
    for p in &paths[1..] {
        let matched = first
            .iter()
            .zip(p.components())
            .take_while(|(a, b)| **a == *b)
            .count();
        common_len = common_len.min(matched);
        if common_len == 0 {
            break;
        }
    }
    first[..common_len].iter().collect()
}

pub(crate) fn compute_render_prefix(paths: &[PathBuf]) -> PathBuf {
    let lcp = longest_common_path_prefix(paths);
    if !lcp.as_os_str().is_empty() {
        return lcp;
    }
    let cwd = std::env::current_dir().unwrap_or_default();
    if !cwd.as_os_str().is_empty() && !paths.is_empty() && paths.iter().all(|p| p.starts_with(&cwd))
    {
        return cwd;
    }
    PathBuf::new()
}

fn strip_to_display(
    path: &Path,
    prefix: &Path,
) -> String {
    if prefix.as_os_str().is_empty() {
        return path.display().to_string();
    }
    path.strip_prefix(prefix)
        .map_or_else(|_| path.display().to_string(), |p| p.display().to_string())
}

fn write_pr_comment_row(
    out: &mut dyn Write,
    de: &DeltaEntry,
    threshold: f64,
    prefix: &Path,
    links: Option<&SourceLinks>,
) -> Result<()> {
    let e = &de.current;
    let grade = Grade::of(e.crap, threshold);
    let cov = e.coverage.map_or("—".to_string(), |p| format!("{p:.1}"));
    let loc_text = strip_to_display(&e.file, prefix);
    let func = linkify(format!("`{}`", e.function), links, &e.file, e.line);
    let prev_suffix = de
        .previous_file
        .as_ref()
        .map(|p| format!(" ← `{}`", strip_to_display(p, prefix)))
        .unwrap_or_default();
    let loc_inner = format!("`{loc_text}:{}`{prev_suffix}", e.line);
    let loc = linkify(loc_inner, links, &e.file, e.line);
    writeln!(
        out,
        "| {} | {:.1} | {} | {} | {} | {} | {} |",
        grade.icon(),
        e.crap,
        delta_display(de),
        e.cyclomatic as usize,
        cov,
        func,
        loc,
    )?;
    Ok(())
}

fn write_pr_comment_abs_row(
    out: &mut dyn Write,
    e: &CrapEntry,
    threshold: f64,
    prefix: &Path,
    links: Option<&SourceLinks>,
) -> Result<()> {
    let grade = Grade::of(e.crap, threshold);
    let cov = e.coverage.map_or("—".to_string(), |p| format!("{p:.1}"));
    let loc_text = strip_to_display(&e.file, prefix);
    let func = linkify(format!("`{}`", e.function), links, &e.file, e.line);
    let loc = linkify(format!("`{loc_text}:{}`", e.line), links, &e.file, e.line);
    writeln!(
        out,
        "| {} | {:.1} | {} | {} | {} | {} |",
        grade.icon(),
        e.crap,
        e.cyclomatic as usize,
        cov,
        func,
        loc,
    )?;
    Ok(())
}

fn write_truncation_footer(
    out: &mut dyn Write,
    omitted: usize,
) -> Result<()> {
    writeln!(out)?;
    writeln!(
        out,
        "_…and {omitted} more, see CI artifact for the full report._"
    )?;
    Ok(())
}

fn write_truncation_if_capped(
    out: &mut dyn Write,
    total: usize,
) -> Result<()> {
    if total > MAX_ROWS_PER_SECTION {
        write_truncation_footer(out, total - MAX_ROWS_PER_SECTION)?;
    }
    Ok(())
}

fn abs_delta(de: &DeltaEntry) -> f64 {
    de.delta.unwrap_or(0.0).abs()
}

struct DeltaBuckets<'a> {
    regressed: Vec<&'a DeltaEntry>,
    new_entries: Vec<&'a DeltaEntry>,
    improved: Vec<&'a DeltaEntry>,
    moved: Vec<&'a DeltaEntry>,
    hot_spots: Vec<&'a DeltaEntry>,
    removed: Vec<&'a RemovedEntry>,
}

impl<'a> DeltaBuckets<'a> {
    fn from_report(
        report: &'a DeltaReport,
        threshold: f64,
    ) -> Self {
        let mut regressed: Vec<&DeltaEntry> = report
            .entries
            .iter()
            .filter(|e| e.status == DeltaStatus::Regressed)
            .collect();
        regressed.sort_by(|a, b| abs_delta(b).total_cmp(&abs_delta(a)));

        let mut new_entries: Vec<&DeltaEntry> = report
            .entries
            .iter()
            .filter(|e| e.status == DeltaStatus::New)
            .collect();
        new_entries.sort_by(|a, b| b.current.crap.total_cmp(&a.current.crap));

        let mut improved: Vec<&DeltaEntry> = report
            .entries
            .iter()
            .filter(|e| e.status == DeltaStatus::Improved)
            .collect();
        improved.sort_by(|a, b| abs_delta(b).total_cmp(&abs_delta(a)));

        let mut moved: Vec<&DeltaEntry> = report
            .entries
            .iter()
            .filter(|e| e.status == DeltaStatus::Moved)
            .collect();
        moved.sort_by(|a, b| b.current.crap.total_cmp(&a.current.crap));

        let mut hot_spots: Vec<&DeltaEntry> = report
            .entries
            .iter()
            .filter(|e| e.status == DeltaStatus::Unchanged && e.current.crap > threshold)
            .collect();
        hot_spots.sort_by(|a, b| b.current.crap.total_cmp(&a.current.crap));

        let mut removed: Vec<&RemovedEntry> = report.removed.iter().collect();
        removed.sort_by(|a, b| b.baseline_crap.total_cmp(&a.baseline_crap));

        Self {
            regressed,
            new_entries,
            improved,
            moved,
            hot_spots,
            removed,
        }
    }

    fn common_prefix(&self) -> PathBuf {
        let entry_paths = self
            .regressed
            .iter()
            .chain(&self.new_entries)
            .chain(&self.improved)
            .chain(&self.moved)
            .chain(&self.hot_spots)
            .flat_map(|de| {
                std::iter::once(de.current.file.clone()).chain(de.previous_file.iter().cloned())
            });
        let removed_paths = self.removed.iter().map(|r| r.file.clone());
        let paths: Vec<PathBuf> = entry_paths.chain(removed_paths).collect();
        compute_render_prefix(&paths)
    }
}

fn write_pr_comment_delta_headline(
    out: &mut dyn Write,
    regressions: usize,
) -> Result<()> {
    if regressions == 0 {
        writeln!(out, "## ✅ No CRAP regressions")?;
    } else {
        writeln!(out, "## ⚠️ {regressions} CRAP regression(s) detected")?;
    }
    writeln!(out)?;
    Ok(())
}

fn write_pr_comment_breakdown(
    out: &mut dyn Write,
    b: &DeltaBuckets,
    unchanged: usize,
) -> Result<()> {
    writeln!(
        out,
        "↑ {} regressed · ★ {} new · ↔ {} moved · ↓ {} improved · {} unchanged · — {} removed",
        b.regressed.len(),
        b.new_entries.len(),
        b.moved.len(),
        b.improved.len(),
        unchanged,
        b.removed.len(),
    )?;
    Ok(())
}

fn write_pr_comment_primary(
    out: &mut dyn Write,
    b: &DeltaBuckets,
    threshold: f64,
    prefix: &Path,
    links: Option<&SourceLinks>,
) -> Result<()> {
    let total = b.regressed.len() + b.new_entries.len();
    if total == 0 {
        return Ok(());
    }
    writeln!(out)?;
    writeln!(out, "| | CRAP | Δ | CC | Cov % | Function | Location |")?;
    writeln!(out, "|---|---:|---:|---:|---:|---|---|")?;
    for de in b
        .regressed
        .iter()
        .chain(b.new_entries.iter())
        .take(MAX_ROWS_PER_SECTION)
    {
        write_pr_comment_row(out, de, threshold, prefix, links)?;
    }
    write_truncation_if_capped(out, total)
}

fn write_pr_comment_improved_section(
    out: &mut dyn Write,
    b: &DeltaBuckets,
    threshold: f64,
    prefix: &Path,
    links: Option<&SourceLinks>,
) -> Result<()> {
    if b.improved.is_empty() {
        return Ok(());
    }
    writeln!(out)?;
    writeln!(
        out,
        "<details><summary>↓ {} improved</summary>",
        b.improved.len()
    )?;
    writeln!(out)?;
    writeln!(out, "| | CRAP | Δ | CC | Cov % | Function | Location |")?;
    writeln!(out, "|---|---:|---:|---:|---:|---|---|")?;
    for de in b.improved.iter().take(MAX_ROWS_PER_SECTION) {
        write_pr_comment_row(out, de, threshold, prefix, links)?;
    }
    write_truncation_if_capped(out, b.improved.len())?;
    writeln!(out)?;
    writeln!(out, "</details>")?;
    Ok(())
}

fn write_pr_comment_moved_section(
    out: &mut dyn Write,
    b: &DeltaBuckets,
    threshold: f64,
    prefix: &Path,
    links: Option<&SourceLinks>,
) -> Result<()> {
    if b.moved.is_empty() {
        return Ok(());
    }
    writeln!(out)?;
    writeln!(out, "<details><summary>↔ {} moved</summary>", b.moved.len())?;
    writeln!(out)?;
    writeln!(out, "| | CRAP | Δ | CC | Cov % | Function | Location |")?;
    writeln!(out, "|---|---:|---:|---:|---:|---|---|")?;
    for de in b.moved.iter().take(MAX_ROWS_PER_SECTION) {
        write_pr_comment_row(out, de, threshold, prefix, links)?;
    }
    write_truncation_if_capped(out, b.moved.len())?;
    writeln!(out)?;
    writeln!(out, "</details>")?;
    Ok(())
}

fn write_pr_comment_hot_spots_section(
    out: &mut dyn Write,
    b: &DeltaBuckets,
    threshold: f64,
    prefix: &Path,
    links: Option<&SourceLinks>,
) -> Result<()> {
    if b.hot_spots.is_empty() {
        return Ok(());
    }
    writeln!(out)?;
    writeln!(
        out,
        "<details><summary>🔥 Top hot spots above threshold</summary>"
    )?;
    writeln!(out)?;
    writeln!(out, "| | CRAP | CC | Cov % | Function | Location |")?;
    writeln!(out, "|---|---:|---:|---:|---|---|")?;
    for de in b.hot_spots.iter().take(MAX_ROWS_PER_SECTION) {
        write_pr_comment_abs_row(out, &de.current, threshold, prefix, links)?;
    }
    write_truncation_if_capped(out, b.hot_spots.len())?;
    writeln!(out)?;
    writeln!(out, "</details>")?;
    Ok(())
}

fn write_pr_comment_removed_section(
    out: &mut dyn Write,
    b: &DeltaBuckets,
    prefix: &Path,
) -> Result<()> {
    if b.removed.is_empty() {
        return Ok(());
    }
    writeln!(out)?;
    writeln!(
        out,
        "<details><summary>— {} removed</summary>",
        b.removed.len()
    )?;
    writeln!(out)?;
    for r in b.removed.iter().take(MAX_ROWS_PER_SECTION) {
        let loc = strip_to_display(&r.file, prefix);
        writeln!(
            out,
            "- `{}` (was {:.1}) — `{}`",
            r.function, r.baseline_crap, loc
        )?;
    }
    write_truncation_if_capped(out, b.removed.len())?;
    writeln!(out)?;
    writeln!(out, "</details>")?;
    Ok(())
}

fn unchanged_count(report: &DeltaReport) -> usize {
    report
        .entries
        .iter()
        .filter(|e| e.status == DeltaStatus::Unchanged)
        .count()
}

pub(crate) fn render_delta_pr_comment(
    report: &DeltaReport,
    threshold: f64,
    links: Option<&SourceLinks>,
    out: &mut dyn Write,
) -> Result<()> {
    write_pr_comment_marker(out)?;
    if report.entries.is_empty() && report.removed.is_empty() {
        writeln!(out, "_No functions found._")?;
        return Ok(());
    }
    let buckets = DeltaBuckets::from_report(report, threshold);
    let prefix = buckets.common_prefix();
    write_pr_comment_delta_headline(out, buckets.regressed.len())?;
    write_pr_comment_breakdown(out, &buckets, unchanged_count(report))?;
    write_pr_comment_primary(out, &buckets, threshold, &prefix, links)?;
    write_pr_comment_secondary_sections(out, &buckets, threshold, &prefix, links)
}

fn write_pr_comment_secondary_sections(
    out: &mut dyn Write,
    buckets: &DeltaBuckets,
    threshold: f64,
    prefix: &Path,
    links: Option<&SourceLinks>,
) -> Result<()> {
    write_pr_comment_improved_section(out, buckets, threshold, prefix, links)?;
    write_pr_comment_moved_section(out, buckets, threshold, prefix, links)?;
    write_pr_comment_hot_spots_section(out, buckets, threshold, prefix, links)?;
    write_pr_comment_removed_section(out, buckets, prefix)
}

fn write_pr_comment_abs_headline(
    out: &mut dyn Write,
    crappy: usize,
    threshold: f64,
) -> Result<()> {
    if crappy == 0 {
        writeln!(out, "## ✅ No CRAP threshold violations")?;
    } else {
        writeln!(
            out,
            "## ⚠️ {crappy} function(s) exceed CRAP threshold {threshold}"
        )?;
    }
    writeln!(out)?;
    Ok(())
}

fn above_threshold_sorted(
    entries: &[CrapEntry],
    threshold: f64,
) -> Vec<&CrapEntry> {
    let mut above: Vec<&CrapEntry> = entries.iter().filter(|e| e.crap > threshold).collect();
    above.sort_by(|a, b| b.crap.total_cmp(&a.crap));
    above
}

fn write_pr_comment_abs_table(
    out: &mut dyn Write,
    above: &[&CrapEntry],
    threshold: f64,
    links: Option<&SourceLinks>,
) -> Result<()> {
    if above.is_empty() {
        return Ok(());
    }
    let paths: Vec<PathBuf> = above.iter().map(|e| e.file.clone()).collect();
    let prefix = compute_render_prefix(&paths);
    writeln!(out)?;
    writeln!(out, "| | CRAP | CC | Cov % | Function | Location |")?;
    writeln!(out, "|---|---:|---:|---:|---|---|")?;
    for e in above.iter().take(MAX_ROWS_PER_SECTION) {
        write_pr_comment_abs_row(out, e, threshold, &prefix, links)?;
    }
    write_truncation_if_capped(out, above.len())
}

pub(crate) fn render_pr_comment(
    entries: &[CrapEntry],
    threshold: f64,
    links: Option<&SourceLinks>,
    out: &mut dyn Write,
) -> Result<()> {
    write_pr_comment_marker(out)?;
    if entries.is_empty() {
        writeln!(out, "_No functions found._")?;
        return Ok(());
    }
    write_pr_comment_abs_headline(out, super::crappy_count(entries, threshold), threshold)?;
    writeln!(
        out,
        "{} function(s) analyzed · threshold {threshold}",
        entries.len()
    )?;
    let above = above_threshold_sorted(entries, threshold);
    write_pr_comment_abs_table(out, &above, threshold, links)
}

#[cfg(test)]
mod tests {
    use super::super::{Format, render};
    use super::*;

    fn delta_entry(
        file: &str,
        function: &str,
        crap: f64,
        baseline: Option<f64>,
        status: DeltaStatus,
    ) -> DeltaEntry {
        DeltaEntry {
            current: CrapEntry {
                file: PathBuf::from(file),
                function: function.into(),
                line: 1,
                cyclomatic: 5.0,
                coverage: Some(80.0),
                crap,
                crate_name: None,
            },
            baseline_crap: baseline,
            delta: baseline.map(|b| crap - b),
            status,
            previous_file: None,
        }
    }

    fn render_delta_pr_to_string(report: &DeltaReport) -> String {
        let mut buf = Vec::new();
        render_delta_pr_comment(report, 30.0, None, &mut buf).unwrap();
        String::from_utf8(buf).unwrap()
    }

    fn render_delta_pr_with_links(
        report: &DeltaReport,
        links: &SourceLinks,
    ) -> String {
        let mut buf = Vec::new();
        render_delta_pr_comment(report, 30.0, Some(links), &mut buf).unwrap();
        String::from_utf8(buf).unwrap()
    }

    #[test]
    fn lcp_empty_for_fewer_than_two_paths() {
        assert_eq!(longest_common_path_prefix(&[]), PathBuf::new());
        assert_eq!(
            longest_common_path_prefix(&[PathBuf::from("/a/b/c")]),
            PathBuf::new()
        );
    }

    #[test]
    fn lcp_finds_component_wise_prefix() {
        let paths = vec![
            PathBuf::from("/home/runner/work/repo/src/a.rs"),
            PathBuf::from("/home/runner/work/repo/src/b.rs"),
            PathBuf::from("/home/runner/work/repo/tests/c.rs"),
        ];
        assert_eq!(
            longest_common_path_prefix(&paths),
            PathBuf::from("/home/runner/work/repo")
        );
    }

    #[test]
    fn lcp_does_not_collapse_partial_component() {
        let paths = vec![PathBuf::from("/a/foo"), PathBuf::from("/a/foobar")];
        assert_eq!(longest_common_path_prefix(&paths), PathBuf::from("/a"));
    }

    #[test]
    fn lcp_no_overlap_returns_empty() {
        let paths = vec![PathBuf::from("src/a.rs"), PathBuf::from("tests/b.rs")];
        assert_eq!(longest_common_path_prefix(&paths), PathBuf::new());
    }

    #[test]
    fn render_prefix_empty_paths_returns_empty() {
        assert_eq!(compute_render_prefix(&[]), PathBuf::new());
    }

    #[test]
    fn render_prefix_paths_outside_cwd_returns_empty() {
        let outside = PathBuf::from("/tmp/definitely_not_under_cwd_xyzzy/foo.rs");
        assert_eq!(compute_render_prefix(&[outside]), PathBuf::new());
    }

    #[test]
    fn render_prefix_falls_back_to_cwd_when_path_under_cwd() {
        let cwd = std::env::current_dir().expect("cwd");
        let inside = cwd.join("nested").join("foo.rs");
        assert_eq!(compute_render_prefix(&[inside]), cwd);
    }

    #[test]
    fn pr_comment_starts_with_marker() {
        let report = DeltaReport {
            entries: vec![delta_entry(
                "src/a.rs",
                "foo",
                10.0,
                Some(5.0),
                DeltaStatus::Regressed,
            )],
            removed: vec![],
        };
        let s = render_delta_pr_to_string(&report);
        assert!(
            s.starts_with("<!-- polycrap-report -->"),
            "pr-comment must start with marker"
        );
    }

    #[test]
    fn pr_comment_hides_unchanged_rows() {
        let report = DeltaReport {
            entries: vec![
                delta_entry(
                    "src/a.rs",
                    "regressed_fn",
                    12.0,
                    Some(5.0),
                    DeltaStatus::Regressed,
                ),
                delta_entry(
                    "src/a.rs",
                    "unchanged_fn",
                    5.0,
                    Some(5.0),
                    DeltaStatus::Unchanged,
                ),
            ],
            removed: vec![],
        };
        let s = render_delta_pr_to_string(&report);
        assert!(s.contains("regressed_fn"));
        assert!(
            !s.contains("unchanged_fn"),
            "unchanged rows must be hidden, got:\n{s}"
        );
        assert!(s.contains("1 unchanged"));
    }

    #[test]
    fn pr_comment_regressed_sorted_by_abs_delta_desc() {
        let report = DeltaReport {
            entries: vec![
                delta_entry(
                    "src/a.rs",
                    "small_jump",
                    6.0,
                    Some(5.0),
                    DeltaStatus::Regressed,
                ),
                delta_entry(
                    "src/a.rs",
                    "big_jump",
                    50.0,
                    Some(5.0),
                    DeltaStatus::Regressed,
                ),
                delta_entry(
                    "src/a.rs",
                    "medium_jump",
                    15.0,
                    Some(5.0),
                    DeltaStatus::Regressed,
                ),
            ],
            removed: vec![],
        };
        let s = render_delta_pr_to_string(&report);
        let big_pos = s.find("big_jump").unwrap();
        let med_pos = s.find("medium_jump").unwrap();
        let small_pos = s.find("small_jump").unwrap();
        assert!(
            big_pos < med_pos && med_pos < small_pos,
            "order wrong:\n{s}"
        );
    }

    #[test]
    fn pr_comment_improved_in_collapsed_details() {
        let report = DeltaReport {
            entries: vec![delta_entry(
                "src/a.rs",
                "improved_fn",
                3.0,
                Some(10.0),
                DeltaStatus::Improved,
            )],
            removed: vec![],
        };
        let s = render_delta_pr_to_string(&report);
        assert!(
            s.contains("<details><summary>↓ 1 improved</summary>"),
            "improved must be inside <details>, got:\n{s}"
        );
        assert!(s.contains("improved_fn"));
        assert!(s.contains("</details>"));
    }

    #[test]
    fn pr_comment_moved_in_collapsed_details() {
        let mut entry = delta_entry("src/new.rs", "render", 5.0, Some(5.0), DeltaStatus::Moved);
        entry.previous_file = Some(PathBuf::from("src/old.rs"));
        let report = DeltaReport {
            entries: vec![entry],
            removed: vec![],
        };
        let s = render_delta_pr_to_string(&report);
        assert!(
            s.contains("<details><summary>↔ 1 moved</summary>"),
            "moved must be inside <details>, got:\n{s}"
        );
        assert!(s.contains("← `old.rs`"), "must show prev path:\n{s}");
        assert!(s.contains("↔ 1 moved"));
    }

    #[test]
    fn pr_comment_moved_section_omitted_when_empty() {
        let report = DeltaReport {
            entries: vec![delta_entry(
                "src/a.rs",
                "foo",
                5.0,
                Some(5.0),
                DeltaStatus::Unchanged,
            )],
            removed: vec![],
        };
        let s = render_delta_pr_to_string(&report);
        assert!(
            !s.contains("↔ 0 moved</summary>"),
            "empty moved section must be omitted, got:\n{s}"
        );
    }

    #[test]
    fn pr_comment_removed_in_collapsed_details() {
        let report = DeltaReport {
            entries: vec![],
            removed: vec![RemovedEntry {
                function: "gone_fn".into(),
                file: PathBuf::from("src/a.rs"),
                baseline_crap: 8.0,
            }],
        };
        let s = render_delta_pr_to_string(&report);
        assert!(s.contains("<details><summary>— 1 removed</summary>"));
        assert!(s.contains("gone_fn"));
    }

    #[test]
    fn pr_comment_hot_spots_block_only_when_above_threshold() {
        let report = DeltaReport {
            entries: vec![
                delta_entry(
                    "src/a.rs",
                    "hot_fn",
                    80.0,
                    Some(80.0),
                    DeltaStatus::Unchanged,
                ),
                delta_entry(
                    "src/a.rs",
                    "cool_fn",
                    5.0,
                    Some(5.0),
                    DeltaStatus::Unchanged,
                ),
            ],
            removed: vec![],
        };
        let s = render_delta_pr_to_string(&report);
        assert!(
            s.contains("🔥 Top hot spots above threshold"),
            "hot spots block missing:\n{s}"
        );
        assert!(s.contains("hot_fn"));
        assert!(
            !s.contains("cool_fn"),
            "below-threshold unchanged must not appear"
        );
    }

    #[test]
    fn pr_comment_caps_primary_table_at_25_with_truncation_footer() {
        let entries: Vec<DeltaEntry> = (0..30)
            .map(|i| {
                delta_entry(
                    "src/a.rs",
                    &format!("fn_{i:02}"),
                    100.0 - f64::from(i),
                    Some(1.0),
                    DeltaStatus::Regressed,
                )
            })
            .collect();
        let report = DeltaReport {
            entries,
            removed: vec![],
        };
        let s = render_delta_pr_to_string(&report);
        assert!(s.contains("fn_00"));
        assert!(s.contains("fn_24"));
        assert!(!s.contains("fn_25"), "row 26 must be capped out");
        assert!(s.contains("…and 5 more"));
    }

    #[test]
    fn pr_comment_strips_longest_common_path_prefix() {
        let report = DeltaReport {
            entries: vec![
                delta_entry(
                    "/home/runner/work/repo/src/a.rs",
                    "fn_a",
                    12.0,
                    Some(5.0),
                    DeltaStatus::Regressed,
                ),
                delta_entry(
                    "/home/runner/work/repo/tests/b.rs",
                    "fn_b",
                    14.0,
                    Some(5.0),
                    DeltaStatus::Regressed,
                ),
            ],
            removed: vec![],
        };
        let s = render_delta_pr_to_string(&report);
        assert!(
            s.contains("`src/a.rs:1`"),
            "expected stripped path src/a.rs, got:\n{s}"
        );
        assert!(s.contains("`tests/b.rs:1`"));
        assert!(
            !s.contains("/home/runner"),
            "common prefix must be stripped:\n{s}"
        );
    }

    #[test]
    fn pr_comment_clean_headline_when_no_regressions() {
        let report = DeltaReport {
            entries: vec![delta_entry(
                "src/a.rs",
                "improved_fn",
                3.0,
                Some(10.0),
                DeltaStatus::Improved,
            )],
            removed: vec![],
        };
        let s = render_delta_pr_to_string(&report);
        assert!(s.contains("## ✅ No CRAP regressions"));
    }

    #[test]
    fn pr_comment_absolute_starts_with_marker() {
        let entries = vec![CrapEntry {
            file: PathBuf::from("src/a.rs"),
            function: "foo".into(),
            line: 1,
            cyclomatic: 1.0,
            coverage: Some(100.0),
            crap: 1.0,
            crate_name: None,
        }];
        let mut buf = Vec::new();
        render(&entries, 30.0, Format::PrComment, None, &mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(s.starts_with("<!-- polycrap-report -->"));
    }

    #[test]
    fn pr_comment_absolute_no_violations_shows_pass_heading() {
        let entries = vec![CrapEntry {
            file: PathBuf::from("src/a.rs"),
            function: "foo".into(),
            line: 1,
            cyclomatic: 1.0,
            coverage: Some(100.0),
            crap: 1.0,
            crate_name: None,
        }];
        let mut buf = Vec::new();
        render(&entries, 30.0, Format::PrComment, None, &mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(s.contains("## ✅ No CRAP threshold violations"));
    }

    #[test]
    fn pr_comment_absolute_table_contains_above_threshold_rows() {
        let entries = vec![CrapEntry {
            file: PathBuf::from("src/a.rs"),
            function: "very_crappy".into(),
            line: 42,
            cyclomatic: 10.0,
            coverage: Some(0.0),
            crap: 110.0,
            crate_name: None,
        }];
        let mut buf = Vec::new();
        render(&entries, 30.0, Format::PrComment, None, &mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(
            s.contains("`very_crappy`"),
            "above-threshold function must appear as a row:\n{s}"
        );
        assert!(
            s.contains("|---|---:|---:|---:|---|---|"),
            "table header separator must be present:\n{s}"
        );
    }
}
