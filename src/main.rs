use anyhow::{Context, Result, bail};
use polycrap::{
    complexity,
    coverage::{self, FileCoverage},
    delta::{compute_delta, load_baseline},
    merge::{MissingCoveragePolicy, merge},
    report::{Format, SourceLinks, crappy_count, render, render_delta, render_delta_summary, render_summary},
    score::DEFAULT_THRESHOLD,
};
use clap::{Parser, ValueEnum};
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use indicatif::{ProgressBar, ProgressStyle};
use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Parser, Debug)]
#[command(
    name = "polycrap",
    about = "Compute the CRAP (Change Risk Anti-Patterns) metric for any programming language.",
    long_about = None,
    version
)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "the bools come from clap-derived `--flag` switches"
)]
struct Cli {
    /// Path to an LCOV coverage file.
    ///
    /// If omitted, every function is scored as if it had 0% coverage.
    #[arg(long, value_name = "FILE")]
    lcov: Option<PathBuf>,

    /// Root directory to analyze. Defaults to the current directory.
    #[arg(long, value_name = "DIR", default_value = ".")]
    path: PathBuf,

    /// Force a specific language, overriding extension detection.
    /// Use this when files lack a standard extension.
    /// Recognized values: python, javascript, typescript, tsx, go, java,
    /// csharp, ruby, rust, c, cpp, php.
    #[arg(long, value_name = "LANG")]
    lang: Option<String>,

    /// Glob patterns for files to skip (relative to `--path`).
    /// Use `**` to cross directory boundaries. May be repeated.
    #[arg(long, value_name = "GLOB")]
    exclude: Vec<String>,

    /// CRAP score above which a function is considered "crappy".
    /// Falls back to `.polycrap.toml` → built-in default (30).
    #[arg(long)]
    threshold: Option<f64>,

    /// Only print functions with a CRAP score above this cutoff.
    #[arg(long, value_name = "SCORE")]
    min: Option<f64>,

    /// Limit the report to the top N crappiest functions.
    #[arg(long, value_name = "N")]
    top: Option<usize>,

    /// How to handle functions with complexity data but no coverage data.
    /// Falls back to `.polycrap.toml` → built-in default (pessimistic).
    #[arg(long, value_enum)]
    missing: Option<MissingPolicy>,

    /// Output format.
    #[arg(long, value_enum, default_value_t = FormatArg::Human)]
    format: FormatArg,

    /// Print only aggregate statistics — no per-function table.
    #[arg(long)]
    summary: bool,

    /// Exit with a non-zero status if any function's CRAP score exceeds `--threshold`.
    #[arg(long)]
    fail_above: bool,

    /// Suppress functions matching these glob patterns. May be repeated.
    ///
    /// An entry containing `/` or `**` is treated as a path glob; otherwise
    /// it matches the function name.
    #[arg(long, value_name = "GLOB")]
    allow: Vec<String>,

    /// JSON baseline from a previous `--format json` run.
    #[arg(long, value_name = "FILE")]
    baseline: Option<PathBuf>,

    /// Exit non-zero if any function's CRAP score increased since `--baseline`.
    /// Requires `--baseline`.
    #[arg(long)]
    fail_regression: bool,

    /// Write output to FILE instead of stdout.
    #[arg(long, value_name = "FILE")]
    output: Option<PathBuf>,

    /// Maximum number of threads used for parallel source-file analysis.
    #[arg(long, value_name = "N")]
    jobs: Option<usize>,

    /// Tolerance used by the regression detector.
    #[arg(long, value_name = "VALUE", allow_negative_numbers = true)]
    epsilon: Option<f64>,

    /// Base URL of the source-hosting repo (e.g. `https://github.com/owner/repo`).
    #[arg(long, value_name = "URL")]
    repo_url: Option<String>,

    /// Commit SHA or branch name to deep-link into.
    #[arg(long, value_name = "REF")]
    commit_ref: Option<String>,
}

#[derive(ValueEnum, Clone, Copy, Debug)]
enum MissingPolicy {
    Pessimistic,
    Optimistic,
    Skip,
}

impl From<MissingPolicy> for MissingCoveragePolicy {
    fn from(p: MissingPolicy) -> Self {
        match p {
            MissingPolicy::Pessimistic => Self::Pessimistic,
            MissingPolicy::Optimistic => Self::Optimistic,
            MissingPolicy::Skip => Self::Skip,
        }
    }
}

#[derive(ValueEnum, Clone, Copy, Debug)]
enum FormatArg {
    Human,
    Json,
    Github,
    Markdown,
    PrComment,
    Sarif,
}

impl From<FormatArg> for Format {
    fn from(f: FormatArg) -> Self {
        match f {
            FormatArg::Human => Self::Human,
            FormatArg::Json => Self::Json,
            FormatArg::Github => Self::GitHub,
            FormatArg::Markdown => Self::Markdown,
            FormatArg::PrComment => Self::PrComment,
            FormatArg::Sarif => Self::Sarif,
        }
    }
}

fn is_path_allow_pattern(pattern: &str) -> bool {
    pattern.contains('/') || pattern.contains("**")
}

fn build_allow_set(patterns: &[&str]) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for pat in patterns {
        let glob = GlobBuilder::new(pat)
            .build()
            .with_context(|| format!("invalid allow pattern: {pat:?}"))?;
        builder.add(glob);
    }
    builder.build().context("building allow glob set")
}

fn build_path_allow_set(patterns: &[&str]) -> Result<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    for pat in patterns {
        let glob = GlobBuilder::new(pat)
            .literal_separator(true)
            .build()
            .with_context(|| format!("invalid allow pattern: {pat:?}"))?;
        builder.add(glob);
    }
    builder.build().context("building allow path glob set")
}

fn path_set_matches_suffix(set: &GlobSet, path: &Path) -> bool {
    if set.is_empty() {
        return false;
    }
    let components: Vec<_> = path.components().collect();
    for i in 0..components.len() {
        let suffix: PathBuf = components[i..].iter().collect();
        if set.is_match(&suffix) {
            return true;
        }
    }
    false
}

fn apply_filters(
    entries: &mut Vec<polycrap::merge::CrapEntry>,
    allow_patterns: &[String],
    min: Option<f64>,
    top: Option<usize>,
) -> Result<()> {
    if !allow_patterns.is_empty() {
        let (path_pats, name_pats): (Vec<&str>, Vec<&str>) = allow_patterns
            .iter()
            .map(String::as_str)
            .partition(|p| is_path_allow_pattern(p));
        let name_set = build_allow_set(&name_pats)?;
        let path_set = build_path_allow_set(&path_pats)?;
        entries.retain(|e| {
            !name_set.is_match(&e.function) && !path_set_matches_suffix(&path_set, &e.file)
        });
    }
    if let Some(min) = min {
        entries.retain(|e| e.crap >= min);
    }
    if let Some(top) = top {
        entries.truncate(top);
    }
    Ok(())
}

fn load_coverage(lcov: Option<&PathBuf>) -> Result<HashMap<PathBuf, FileCoverage>> {
    match lcov {
        Some(path) => coverage::parse_lcov(path)
            .with_context(|| format!("parsing LCOV file {}", path.display())),
        None => Ok(HashMap::new()),
    }
}

fn open_output(path: Option<&PathBuf>) -> Result<Box<dyn Write>> {
    Ok(match path {
        Some(p) => Box::new(BufWriter::new(File::create(p).with_context(|| {
            format!("creating output file {}", p.display())
        })?)),
        None => Box::new(io::stdout()),
    })
}

fn spinner(msg: &'static str) -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::with_template("{spinner:.cyan} {msg}")
            .unwrap()
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏", ""]),
    );
    pb.set_message(msg);
    pb.enable_steady_tick(Duration::from_millis(80));
    pb
}

fn warn_unmapped(files: &[PathBuf]) {
    if files.is_empty() {
        return;
    }
    let n = files.len();
    eprintln!(
        "warning: {} source file{} had no matching entry in the LCOV report \
         — verify your --lcov path or coverage tool configuration:",
        n,
        if n == 1 { "" } else { "s" },
    );
    for f in files {
        eprintln!("  {}", f.display());
    }
}

fn validate_args(cli: &Cli) -> Result<()> {
    if !cli.path.exists() {
        bail!("path does not exist: {}", cli.path.display());
    }
    if cli.fail_regression && cli.baseline.is_none() {
        bail!("--fail-regression requires --baseline");
    }
    if matches!(cli.jobs, Some(0)) {
        bail!("invalid --jobs value: must be a positive integer");
    }
    if let Some(eps) = cli.epsilon
        && eps < 0.0
    {
        bail!("invalid --epsilon value: must be non-negative");
    }
    if let Some(ref lang) = cli.lang {
        if polycrap::language::by_name(lang).is_none() {
            let valid = polycrap::language::all_names().join(", ");
            bail!("unknown language {lang:?}. Valid options: {valid}");
        }
    }
    Ok(())
}

struct RenderOpts<'a> {
    threshold: f64,
    epsilon: f64,
    format: Format,
    summary: bool,
    links: Option<&'a SourceLinks>,
}

fn do_render(
    entries: &[polycrap::merge::CrapEntry],
    baseline: Option<&PathBuf>,
    opts: &RenderOpts,
    out: &mut dyn Write,
) -> Result<(bool, bool)> {
    if let Some(baseline_path) = baseline {
        let baseline_data = load_baseline(baseline_path)?;
        let report = compute_delta(entries, &baseline_data, opts.epsilon);
        let has_crappy = crappy_count(entries, opts.threshold) > 0;
        let has_regression = report.regression_count() > 0;
        if opts.summary {
            render_delta_summary(&report, out)?;
        } else {
            render_delta(&report, opts.threshold, opts.format, opts.links, out)?;
        }
        Ok((has_crappy, has_regression))
    } else {
        let has_crappy = crappy_count(entries, opts.threshold) > 0;
        if opts.summary {
            render_summary(entries, opts.threshold, out)?;
        } else {
            render(entries, opts.threshold, opts.format, opts.links, out)?;
        }
        Ok((has_crappy, false))
    }
}

fn resolve_source_links(
    cli_repo_url: Option<String>,
    cli_commit_ref: Option<String>,
) -> Option<SourceLinks> {
    let repo_url = cli_repo_url.or_else(|| {
        let server = std::env::var("GITHUB_SERVER_URL").ok()?;
        let repo = std::env::var("GITHUB_REPOSITORY").ok()?;
        Some(format!(
            "{}/{}",
            server.trim_end_matches('/'),
            repo.trim_start_matches('/')
        ))
    })?;
    let commit_ref = cli_commit_ref.or_else(|| std::env::var("GITHUB_SHA").ok())?;
    Some(SourceLinks::new(repo_url, commit_ref))
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    validate_args(&cli)?;

    let cwd = std::env::current_dir().unwrap_or_else(|_| cli.path.clone());
    let config = polycrap::config::load(&cwd)?;

    let threshold = cli.threshold.or(config.threshold).unwrap_or(DEFAULT_THRESHOLD);

    let missing_policy: MissingCoveragePolicy = cli
        .missing
        .map(Into::into)
        .or(config.missing)
        .unwrap_or(MissingCoveragePolicy::Pessimistic);

    let fail_above = cli.fail_above || config.fail_above.unwrap_or(false);
    let fail_regression = cli.fail_regression || config.fail_regression.unwrap_or(false);

    let epsilon = cli
        .epsilon
        .or(config.epsilon)
        .unwrap_or(polycrap::delta::DEFAULT_EPSILON);

    let mut effective_exclude = config.exclude;
    effective_exclude.extend(cli.exclude);
    let mut effective_allow = config.allow;
    effective_allow.extend(cli.allow);

    let force_lang: Option<&'static polycrap::language::LanguageConfig> = cli
        .lang
        .as_deref()
        .and_then(polycrap::language::by_name);

    if let Some(n) = cli.jobs.or(config.jobs) {
        rayon::ThreadPoolBuilder::new()
            .num_threads(n)
            .build_global()
            .with_context(|| format!("configuring rayon thread pool to {n} threads"))?;
    }

    let pb = spinner("Analyzing source files…");
    let fns = complexity::analyze_tree(&cli.path, &effective_exclude, force_lang)
        .with_context(|| format!("analyzing {}", cli.path.display()))?;

    pb.set_message("Parsing coverage report…");
    let coverage = load_coverage(cli.lcov.as_ref())?;
    pb.finish_and_clear();

    let merge_result = merge(fns, coverage, missing_policy);
    warn_unmapped(&merge_result.unmapped_files);
    let mut entries = merge_result.entries;
    apply_filters(
        &mut entries,
        &effective_allow,
        cli.min.or(config.min),
        cli.top.or(config.top),
    )?;

    let mut out_box = open_output(cli.output.as_ref())?;
    let links = resolve_source_links(cli.repo_url, cli.commit_ref);
    let opts = RenderOpts {
        threshold,
        epsilon,
        format: cli.format.into(),
        summary: cli.summary,
        links: links.as_ref(),
    };
    let (has_crappy, has_regression) =
        do_render(&entries, cli.baseline.as_ref(), &opts, out_box.as_mut())?;

    if (fail_above && has_crappy) || (fail_regression && has_regression) {
        std::process::exit(1);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_glob_classifier_keeps_function_patterns() {
        assert!(!is_path_allow_pattern("trivial"));
        assert!(!is_path_allow_pattern("Foo::*"));
        assert!(!is_path_allow_pattern("generated_*"));
        assert!(!is_path_allow_pattern("*"));
    }

    #[test]
    fn path_glob_classifier_recognizes_path_patterns() {
        assert!(is_path_allow_pattern("src/generated/**"));
        assert!(is_path_allow_pattern("tests/**"));
        assert!(is_path_allow_pattern("**/build.rs"));
        assert!(is_path_allow_pattern("a/b"));
    }

    #[test]
    fn path_set_matches_relative_pattern_against_absolute_file() {
        let set = build_path_allow_set(&["src/generated/**"]).unwrap();
        let abs = Path::new("/home/u/project/src/generated/foo.py");
        assert!(path_set_matches_suffix(&set, abs));
    }

    #[test]
    fn path_set_does_not_match_unrelated_file() {
        let set = build_path_allow_set(&["src/generated/**"]).unwrap();
        let other = Path::new("/home/u/project/src/main.py");
        assert!(!path_set_matches_suffix(&set, other));
    }

    #[test]
    fn empty_path_set_is_no_op() {
        let set = build_path_allow_set(&[]).unwrap();
        assert!(!path_set_matches_suffix(&set, Path::new("any/path.py")));
    }
}
