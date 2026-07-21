//! Deterministic, read-only repository identity and correlation evidence.

use std::fmt::{Display, Formatter};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Correlator contract version retained with every score.
pub const CORRELATION_VERSION: &str = "deterministic-v1";

/// Git identity collected without mutating the inspected repository.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RepositoryIdentity {
    /// Stable content-derived repository ID.
    pub repository_id: String,
    /// Canonical repository root.
    pub root: PathBuf,
    /// Canonical worktree path.
    pub worktree: PathBuf,
    /// Shared Git directory, distinguishing linked worktrees.
    pub git_common_dir: PathBuf,
    /// Normalized origin URL, when configured.
    pub remote_url: Option<String>,
    /// Branch name, or `None` for detached HEAD.
    pub branch: Option<String>,
    /// Current commit.
    pub head: String,
    /// Whether tracked or untracked changes exist.
    pub dirty: bool,
}

/// Safe marker that explicitly assigns future native sessions in a project.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CurrentSessionMarker {
    /// Global session identity.
    pub global_session_id: String,
    /// Human objective without transcript content.
    pub objective: String,
    /// RFC 3339 update timestamp.
    pub updated_at: String,
}

/// One explainable deterministic scoring signal.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CorrelationEvidence {
    /// Stable feature name.
    pub feature: &'static str,
    /// Feature contribution to the final score.
    pub weight: f64,
    /// Whether the feature matched.
    pub matched: bool,
    /// Non-secret explanation.
    pub explanation: String,
}

/// Versioned deterministic score.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CorrelationScore {
    /// Clamped score from zero to one.
    pub confidence: f64,
    /// Algorithm version.
    pub version: &'static str,
    /// Per-feature evidence.
    pub evidence: Vec<CorrelationEvidence>,
}

/// State of one deterministic correlation signal.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SignalMatch {
    /// Evidence did not match.
    #[default]
    Different,
    /// Evidence matched.
    Match,
}

impl SignalMatch {
    fn matched(self) -> bool {
        self == Self::Match
    }
}

/// Signals available for two sessions.
#[derive(Clone, Debug, Default)]
pub struct CorrelationSignals {
    /// Same normalized repository identity.
    pub same_repository: SignalMatch,
    /// Same worktree.
    pub same_worktree: SignalMatch,
    /// Same branch.
    pub same_branch: SignalMatch,
    /// Equal HEAD or proven commit ancestry.
    pub related_head: SignalMatch,
    /// Sessions overlap or occur within the configured time window.
    pub temporally_close: SignalMatch,
}

/// Repository and marker failure.
#[derive(Debug)]
pub enum CorrelationError {
    /// Git command failed or returned unusable output.
    Git(String),
    /// Filesystem access failed.
    Io(std::io::Error),
    /// Marker JSON is malformed or violates the safe shape.
    Marker(String),
}

impl Display for CorrelationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Git(message) => write!(formatter, "git identity error: {message}"),
            Self::Io(error) => write!(formatter, "filesystem error: {error}"),
            Self::Marker(message) => write!(formatter, "session marker error: {message}"),
        }
    }
}

impl std::error::Error for CorrelationError {}

impl From<std::io::Error> for CorrelationError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

/// Discovers a repository using read-only Git commands.
///
/// No command used here updates the index, worktree, configuration, or refs.
///
/// # Errors
///
/// Returns an error when Git identity cannot be read or paths cannot be
/// canonicalized.
pub fn discover_repository(path: &Path) -> Result<RepositoryIdentity, CorrelationError> {
    let root = canonical_git_path(path, "--show-toplevel")?;
    let common_raw = git_output(path, &["rev-parse", "--git-common-dir"])?;
    let git_common_dir = {
        let candidate = PathBuf::from(common_raw);
        let absolute = if candidate.is_absolute() {
            candidate
        } else {
            root.join(candidate)
        };
        absolute.canonicalize()?
    };
    let head = git_output(path, &["rev-parse", "HEAD"])?;
    let branch_output = git_output(path, &["rev-parse", "--abbrev-ref", "HEAD"])?;
    let branch = (branch_output != "HEAD").then_some(branch_output);
    let remote_url = optional_git_output(path, &["config", "--get", "remote.origin.url"])
        .map(|remote| normalize_remote(&remote));
    let dirty =
        !git_output(path, &["status", "--porcelain", "--untracked-files=normal"])?.is_empty();
    let repository_seed = remote_url
        .as_deref()
        .unwrap_or_else(|| git_common_dir.to_str().unwrap_or_default());
    Ok(RepositoryIdentity {
        repository_id: format!("repo_{}", short_hash(repository_seed)),
        worktree: root.clone(),
        root,
        git_common_dir,
        remote_url,
        branch,
        head,
        dirty,
    })
}

/// Checks commit ancestry without modifying repository state.
#[must_use]
pub fn commits_related(path: &Path, left: &str, right: &str) -> bool {
    git_success(path, &["merge-base", "--is-ancestor", left, right])
        || git_success(path, &["merge-base", "--is-ancestor", right, left])
}

/// Produces an explainable score without semantic-model authority.
#[must_use]
pub fn score(signals: &CorrelationSignals) -> CorrelationScore {
    let features = [
        ("repository", 0.45, signals.same_repository.matched()),
        ("worktree", 0.15, signals.same_worktree.matched()),
        ("branch", 0.15, signals.same_branch.matched()),
        ("head_ancestry", 0.15, signals.related_head.matched()),
        (
            "temporal_proximity",
            0.10,
            signals.temporally_close.matched(),
        ),
    ];
    let evidence = features
        .into_iter()
        .map(|(feature, weight, matched)| CorrelationEvidence {
            feature,
            weight,
            matched,
            explanation: if matched {
                format!("{feature} matched")
            } else {
                format!("{feature} did not match")
            },
        })
        .collect::<Vec<_>>();
    CorrelationScore {
        confidence: evidence
            .iter()
            .filter(|item| item.matched)
            .map(|item| item.weight)
            .sum::<f64>()
            .clamp(0.0, 1.0),
        version: CORRELATION_VERSION,
        evidence,
    }
}

/// Reads `.sessionmesh/current.json` with strict shape validation.
///
/// # Errors
///
/// Returns an error for filesystem failures or an invalid marker.
pub fn read_marker(
    repository_root: &Path,
) -> Result<Option<CurrentSessionMarker>, CorrelationError> {
    let path = marker_path(repository_root);
    if !path.exists() {
        return Ok(None);
    }
    let bytes = fs::read(path)?;
    let marker: CurrentSessionMarker = serde_json::from_slice(&bytes)
        .map_err(|error| CorrelationError::Marker(error.to_string()))?;
    validate_marker(&marker)?;
    Ok(Some(marker))
}

/// Atomically writes the safe explicit-correlation marker.
///
/// # Errors
///
/// Returns an error when validation, serialization, or atomic publication
/// fails.
pub fn write_marker(
    repository_root: &Path,
    marker: &CurrentSessionMarker,
) -> Result<(), CorrelationError> {
    validate_marker(marker)?;
    let directory = repository_root.join(".sessionmesh");
    fs::create_dir_all(&directory)?;
    let target = marker_path(repository_root);
    let temporary = directory.join(format!(".current.{}.tmp", std::process::id()));
    let bytes = serde_json::to_vec_pretty(marker)
        .map_err(|error| CorrelationError::Marker(error.to_string()))?;
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)?;
    file.write_all(&bytes)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    drop(file);
    fs::rename(temporary, target)?;
    Ok(())
}

fn validate_marker(marker: &CurrentSessionMarker) -> Result<(), CorrelationError> {
    if !marker.global_session_id.starts_with("gs_") || marker.objective.trim().is_empty() {
        return Err(CorrelationError::Marker(
            "global_session_id and objective are required".to_owned(),
        ));
    }
    chrono::DateTime::parse_from_rfc3339(&marker.updated_at)
        .map_err(|_| CorrelationError::Marker("updated_at must be RFC 3339".to_owned()))?;
    Ok(())
}

fn marker_path(repository_root: &Path) -> PathBuf {
    repository_root.join(".sessionmesh/current.json")
}

fn canonical_git_path(path: &Path, argument: &str) -> Result<PathBuf, CorrelationError> {
    PathBuf::from(git_output(path, &["rev-parse", argument])?)
        .canonicalize()
        .map_err(CorrelationError::Io)
}

fn git_output(path: &Path, arguments: &[&str]) -> Result<String, CorrelationError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(arguments)
        .output()?;
    if !output.status.success() {
        return Err(CorrelationError::Git(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn optional_git_output(path: &Path, arguments: &[&str]) -> Option<String> {
    git_output(path, arguments)
        .ok()
        .filter(|value| !value.is_empty())
}

fn git_success(path: &Path, arguments: &[&str]) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(path)
        .args(arguments)
        .status()
        .is_ok_and(|status| status.success())
}

fn normalize_remote(remote: &str) -> String {
    let trimmed = remote.trim().trim_end_matches('/').trim_end_matches(".git");
    if let Some((user_host, path)) = trimmed.split_once(':')
        && !trimmed.contains("://")
    {
        let host = user_host.rsplit('@').next().unwrap_or(user_host);
        return format!("{}/{}", host.to_ascii_lowercase(), path.trim_matches('/'));
    }
    if let Ok(parsed) = url::Url::parse(trimmed)
        && let Some(host) = parsed.host_str()
    {
        return format!(
            "{}/{}",
            host.to_ascii_lowercase(),
            parsed.path().trim_matches('/')
        );
    }
    trimmed.to_owned()
}

fn short_hash(value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    let mut encoded = String::with_capacity(24);
    for byte in &digest[..12] {
        use std::fmt::Write as _;
        write!(&mut encoded, "{byte:02x}").expect("writing to a String cannot fail");
    }
    encoded
}

pub use sessionmesh_core::bootstrap_stage;

#[cfg(test)]
mod tests {
    use super::*;

    fn git(path: &Path, arguments: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(path)
            .args(arguments)
            .status()
            .expect("git should run");
        assert!(status.success());
    }

    fn repository() -> tempfile::TempDir {
        let directory = tempfile::tempdir().unwrap();
        git(directory.path(), &["init", "--quiet"]);
        git(
            directory.path(),
            &["config", "user.email", "fixture@example.invalid"],
        );
        git(directory.path(), &["config", "user.name", "Fixture"]);
        fs::write(directory.path().join("README.md"), "fixture\n").unwrap();
        git(directory.path(), &["add", "README.md"]);
        git(directory.path(), &["commit", "--quiet", "-m", "fixture"]);
        directory
    }

    #[test]
    fn discovers_no_remote_dirty_and_detached_repository_without_writes() {
        let directory = repository();
        let before = fs::read(directory.path().join(".git/index")).unwrap();
        git(directory.path(), &["checkout", "--quiet", "--detach"]);
        fs::write(directory.path().join("untracked.txt"), "dirty").unwrap();

        let identity = discover_repository(directory.path()).unwrap();

        assert_eq!(identity.remote_url, None);
        assert_eq!(identity.branch, None);
        assert!(identity.dirty);
        assert_eq!(
            before,
            fs::read(directory.path().join(".git/index")).unwrap()
        );
    }

    #[test]
    fn normalizes_equivalent_remote_forms() {
        assert_eq!(
            normalize_remote("git@GitHub.com:Owner/project.git"),
            "github.com/Owner/project"
        );
        assert_eq!(
            normalize_remote("https://github.com/Owner/project.git"),
            "github.com/Owner/project"
        );
    }

    #[test]
    fn marker_round_trip_is_strict_and_contains_no_transcript() {
        let directory = tempfile::tempdir().unwrap();
        let marker = CurrentSessionMarker {
            global_session_id: "gs_123".to_owned(),
            objective: "Build repository identity".to_owned(),
            updated_at: "2026-07-20T14:30:00Z".to_owned(),
        };
        write_marker(directory.path(), &marker).unwrap();

        assert_eq!(read_marker(directory.path()).unwrap(), Some(marker));
        let encoded = fs::read_to_string(marker_path(directory.path())).unwrap();
        assert!(!encoded.contains("transcript"));
        fs::write(
            marker_path(directory.path()),
            r#"{"global_session_id":"gs_123","objective":"x","updated_at":"bad","secret":"x"}"#,
        )
        .unwrap();
        assert!(read_marker(directory.path()).is_err());
    }

    #[test]
    fn scoring_retains_every_feature_and_version() {
        let result = score(&CorrelationSignals {
            same_repository: SignalMatch::Match,
            same_worktree: SignalMatch::Match,
            same_branch: SignalMatch::Different,
            related_head: SignalMatch::Match,
            temporally_close: SignalMatch::Different,
        });
        assert!((result.confidence - 0.75).abs() < f64::EPSILON);
        assert_eq!(result.version, CORRELATION_VERSION);
        assert_eq!(result.evidence.len(), 5);
    }
}
