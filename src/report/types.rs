//! Shared rendering primitives — used by every renderer that draws rows.
//!
//! - [`Grade`]: three-tier severity classification driving icon/colour.
//! - [`coverage_bar`]: 10-block ASCII bar for human tables.
//! - [`delta_display`]: Δ-column text for delta rows.

use crate::delta::{DeltaEntry, DeltaStatus};
use comfy_table::Color;

/// Three-tier severity used for row icons and colour.
///
/// `Moderate` sits between `threshold / 3` and `threshold` — a visible warning
/// that a function is worth watching before it crosses the line.
pub(crate) enum Grade {
    Clean,
    Moderate,
    Crappy,
}

impl Grade {
    pub(crate) fn of(
        score: f64,
        threshold: f64,
    ) -> Self {
        if score > threshold {
            Self::Crappy
        } else if score > threshold / 3.0 {
            Self::Moderate
        } else {
            Self::Clean
        }
    }

    pub(crate) fn icon(&self) -> &'static str {
        match self {
            Self::Clean => "✓",
            Self::Moderate => "▲",
            Self::Crappy => "✗",
        }
    }

    pub(crate) fn color(&self) -> Color {
        match self {
            Self::Clean => Color::Green,
            Self::Moderate => Color::Yellow,
            Self::Crappy => Color::Red,
        }
    }
}

/// Render a coverage value as a 10-block bar followed by the numeric percentage.
///
/// `None` (no coverage data) renders as an empty bar and a dash.
pub(crate) fn coverage_bar(pct: Option<f64>) -> String {
    match pct {
        None => format!("{:░<10}    —", ""),
        Some(p) => {
            let filled = ((p / 100.0) * 10.0).round() as usize;
            let filled = filled.min(10);
            format!(
                "{}{} {:>5.1}%",
                "█".repeat(filled),
                "░".repeat(10 - filled),
                p
            )
        },
    }
}

/// Format the Δ column value for a single delta entry.
pub(crate) fn delta_display(de: &DeltaEntry) -> String {
    match de.status {
        DeltaStatus::Regressed | DeltaStatus::Improved => {
            format!("{:+.1}", de.delta.unwrap())
        },
        DeltaStatus::New => "NEW".to_string(),
        DeltaStatus::Unchanged | DeltaStatus::Moved => String::new(),
    }
}

/// Render a Location-cell string, optionally appending `← <prev>` when the
/// entry was paired by name across files.
pub(crate) fn format_location_with_prev(
    file: &std::path::Path,
    line: usize,
    previous_file: Option<&std::path::Path>,
) -> String {
    match previous_file {
        Some(prev) => format!("`{}:{}` ← `{}`", file.display(), line, prev.display()),
        None => format!("`{}:{}`", file.display(), line),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coverage_bar_is_all_empty_for_zero_percent() {
        let bar = coverage_bar(Some(0.0));
        assert!(
            bar.starts_with("░░░░░░░░░░"),
            "0% must start with 10 empty blocks, got: {bar}"
        );
        assert!(bar.contains("0.0%"), "0% must include numeric label");
    }

    #[test]
    fn coverage_bar_is_all_full_for_100_percent() {
        let bar = coverage_bar(Some(100.0));
        assert!(
            bar.starts_with("██████████"),
            "100% must start with 10 full blocks, got: {bar}"
        );
        assert!(bar.contains("100.0%"), "100% must include numeric label");
    }

    #[test]
    fn coverage_bar_is_half_full_for_50_percent() {
        let bar = coverage_bar(Some(50.0));
        assert!(
            bar.starts_with("█████░░░░░"),
            "50% must have 5 full then 5 empty blocks, got: {bar}"
        );
    }

    #[test]
    fn coverage_bar_none_is_all_empty_with_dash() {
        let bar = coverage_bar(None);
        assert!(
            bar.contains("░░░░░░░░░░"),
            "None must render with all-empty bar, got: {bar}"
        );
        assert!(bar.contains("—"), "None must use — instead of a percentage");
    }

    #[test]
    fn grade_tier_boundaries_are_correct() {
        assert_eq!(
            Grade::of(10.0, 30.0).icon(),
            "✓",
            "exactly threshold/3 → Clean"
        );
        assert_eq!(
            Grade::of(10.001, 30.0).icon(),
            "▲",
            "just above threshold/3 → Moderate"
        );
        assert_eq!(
            Grade::of(30.0, 30.0).icon(),
            "▲",
            "exactly threshold → Moderate (not Crappy)"
        );
        assert_eq!(
            Grade::of(30.001, 30.0).icon(),
            "✗",
            "just above threshold → Crappy"
        );
    }
}
