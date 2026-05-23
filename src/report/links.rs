//! Optional GitHub source-link wrapping for markdown / pr-comment output.

use std::path::{Path, PathBuf};

/// Repo URL + commit ref used by `markdown` / `pr-comment` renderers to wrap
/// Function and Location cells in clickable source links.
#[derive(Clone, Debug)]
pub struct SourceLinks {
    repo_url: String,
    commit_ref: String,
}

impl SourceLinks {
    #[expect(
        clippy::needless_pass_by_value,
        reason = "callers always have fresh Strings; taking &str would force .to_string() at every call site"
    )]
    #[must_use]
    pub fn new(
        repo_url: String,
        commit_ref: String,
    ) -> Self {
        Self {
            repo_url: repo_url.trim_end_matches('/').to_string(),
            commit_ref,
        }
    }

    #[must_use]
    pub fn url_for(
        &self,
        file: &Path,
        line: usize,
    ) -> String {
        let path = file.to_string_lossy().replace('\\', "/");
        format!(
            "{}/blob/{}/{}#L{}",
            self.repo_url, self.commit_ref, path, line
        )
    }
}

fn link_path(path: &Path) -> Option<PathBuf> {
    if path.is_relative() {
        return Some(path.to_path_buf());
    }
    let cwd = std::env::current_dir().ok()?;
    path.strip_prefix(&cwd)
        .ok()
        .map(std::path::Path::to_path_buf)
}

/// Wrap `inner` in a markdown link iff both `links` and a usable URL path are available.
pub(crate) fn linkify(
    inner: String,
    links: Option<&SourceLinks>,
    file: &Path,
    line: usize,
) -> String {
    match (links, link_path(file)) {
        (Some(l), Some(p)) => format!("[{inner}]({})", l.url_for(&p, line)),
        _ => inner,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_links_url_for_joins_components_with_one_slash() {
        let l = SourceLinks::new("https://github.com/owner/repo".into(), "abc123".into());
        let url = l.url_for(Path::new("src/foo.rs"), 42);
        assert_eq!(
            url,
            "https://github.com/owner/repo/blob/abc123/src/foo.rs#L42"
        );
    }

    #[test]
    fn source_links_strips_trailing_slash_from_repo_url() {
        let l = SourceLinks::new("https://github.com/owner/repo/".into(), "abc123".into());
        let url = l.url_for(Path::new("src/foo.rs"), 1);
        assert!(
            !url.contains("repo//blob"),
            "trailing slash must be normalized: {url}"
        );
        assert!(url.contains("/repo/blob/abc123/"));
    }

    #[test]
    fn source_links_url_uses_forward_slashes_even_for_windows_input() {
        let l = SourceLinks::new("https://github.com/o/r".into(), "sha".into());
        let url = l.url_for(Path::new(r"src\foo.rs"), 1);
        assert!(
            !url.contains('\\'),
            "URL must contain no backslashes, got: {url}"
        );
        assert_eq!(url, "https://github.com/o/r/blob/sha/src/foo.rs#L1");
    }
}
