//! Deterministic, read-only discovery of Codex rollout sources.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

/// Candidate Codex home with separate runtime and display paths.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CodexHome {
    /// Path readable by the current native or container process.
    pub readable_path: PathBuf,
    /// Original host-facing path retained for provenance.
    pub original_path: PathBuf,
}

/// Source of an effective Codex home candidate.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum HomeOrigin {
    /// Explicit `SessionMesh` configuration.
    Explicit,
    /// Explicitly supplied `CODEX_HOME` environment value.
    Environment,
    /// Standard `~/.codex` fallback.
    Default,
}

/// Pure discovery inputs. Ambient environment is never read implicitly.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CodexDiscoveryInput {
    /// Explicitly configured native or mapped container homes.
    pub explicit_homes: Vec<CodexHome>,
    /// Optional `$CODEX_HOME` already expanded by configuration.
    pub environment_home: Option<CodexHome>,
    /// Isolated user home used only for the default candidate.
    pub user_home: PathBuf,
}

/// Native Codex source category.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum CodexSourceKind {
    /// `session_index.jsonl`.
    SessionIndex,
    /// One dated `rollout-*.jsonl` stream.
    Rollout,
}

/// Codex product surface represented by the canonical local store.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum CodexSurface {
    /// Codex CLI and compatible consumers of `$CODEX_HOME`.
    Cli,
}

/// Native filesystem permissions retained for diagnostics and provenance.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourcePermissions {
    /// Platform read-only flag.
    pub readonly: bool,
    /// Unix permission bits when available.
    pub unix_mode: Option<u32>,
}

/// Discovered native source with stable identity and preserved display path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CodexSource {
    /// Canonical runtime identity used to collapse aliases.
    pub identity: PathBuf,
    /// Runtime path opened only for reading.
    pub readable_path: PathBuf,
    /// Host-facing provenance path.
    pub original_path: PathBuf,
    /// Source category.
    pub kind: CodexSourceKind,
    /// Native permission metadata observed without changing it.
    pub permissions: SourcePermissions,
    /// Whether permission checks and a read-only open succeeded.
    pub readable: bool,
}

/// One distinct Codex installation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CodexInstallation {
    /// Canonical home identity.
    pub identity: PathBuf,
    /// Runtime home as configured.
    pub readable_home: PathBuf,
    /// User-facing home label.
    pub original_home: PathBuf,
    /// Winning precedence layer.
    pub origin: HomeOrigin,
    /// Explicitly identified product surfaces.
    pub surfaces: Vec<CodexSurface>,
    /// Deterministically ordered native sources.
    pub sources: Vec<CodexSource>,
    /// Valid JSON objects in the optional session index.
    pub valid_index_entries: u64,
    /// Malformed or non-object index records.
    pub malformed_index_entries: u64,
}

/// User-facing diagnostic severity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiscoverySeverity {
    /// Expected optional state.
    Info,
    /// A candidate or record could not be inspected.
    Warning,
}

/// Structured discovery observation without instruction semantics.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiscoveryDiagnostic {
    /// Stable machine-readable code.
    pub code: &'static str,
    /// Display severity.
    pub severity: DiscoverySeverity,
    /// Relevant display path.
    pub path: PathBuf,
    /// Sanitized explanation.
    pub message: String,
}

/// Complete deterministic discovery outcome.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CodexDiscoveryReport {
    /// Distinct installations ordered by canonical identity.
    pub installations: Vec<CodexInstallation>,
    /// Non-fatal diagnostics ordered by path and code.
    pub diagnostics: Vec<DiscoveryDiagnostic>,
}

/// Discovers Codex installations without writes or native file locks.
#[must_use]
pub fn discover(input: &CodexDiscoveryInput) -> CodexDiscoveryReport {
    let mut diagnostics = Vec::new();
    let mut installations = BTreeMap::new();
    for (home, origin) in effective_homes(input) {
        let identity = match fs::canonicalize(&home.readable_path) {
            Ok(identity) => identity,
            Err(error) => {
                let code = if fs::symlink_metadata(&home.readable_path)
                    .is_ok_and(|metadata| metadata.file_type().is_symlink())
                {
                    "codex_home_stale"
                } else if home.readable_path.exists() {
                    "codex_home_unreadable"
                } else {
                    "codex_home_missing"
                };
                diagnostics.push(DiscoveryDiagnostic {
                    code,
                    severity: DiscoverySeverity::Warning,
                    path: home.original_path,
                    message: error.kind().to_string(),
                });
                continue;
            }
        };
        if !is_directory_readable(&identity) {
            diagnostics.push(DiscoveryDiagnostic {
                code: "codex_home_unreadable",
                severity: DiscoverySeverity::Warning,
                path: home.original_path,
                message: "directory has no read permission".to_owned(),
            });
            continue;
        }
        installations
            .entry(identity.clone())
            .or_insert_with(|| inspect_installation(identity, home, origin, &mut diagnostics));
    }
    diagnostics.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.code.cmp(right.code))
            .then_with(|| left.message.cmp(&right.message))
    });
    CodexDiscoveryReport {
        installations: installations.into_values().collect(),
        diagnostics,
    }
}

fn effective_homes(input: &CodexDiscoveryInput) -> Vec<(CodexHome, HomeOrigin)> {
    if !input.explicit_homes.is_empty() {
        return input
            .explicit_homes
            .iter()
            .cloned()
            .map(|home| (home, HomeOrigin::Explicit))
            .collect();
    }
    if let Some(home) = &input.environment_home {
        return vec![(home.clone(), HomeOrigin::Environment)];
    }
    let path = input.user_home.join(".codex");
    vec![(
        CodexHome {
            readable_path: path.clone(),
            original_path: path,
        },
        HomeOrigin::Default,
    )]
}

fn inspect_installation(
    identity: PathBuf,
    home: CodexHome,
    origin: HomeOrigin,
    diagnostics: &mut Vec<DiscoveryDiagnostic>,
) -> CodexInstallation {
    let mut sources = Vec::new();
    let mut identities = BTreeSet::new();
    let index = home.readable_path.join("session_index.jsonl");
    let (valid_index_entries, malformed_index_entries) =
        inspect_index(&home, &index, &mut sources, &mut identities, diagnostics);
    inspect_rollouts(&home, &mut sources, &mut identities, diagnostics);
    sources.sort_by(|left, right| {
        left.kind
            .cmp(&right.kind)
            .then_with(|| left.identity.cmp(&right.identity))
    });
    CodexInstallation {
        identity,
        readable_home: home.readable_path,
        original_home: home.original_path,
        origin,
        surfaces: vec![CodexSurface::Cli],
        sources,
        valid_index_entries,
        malformed_index_entries,
    }
}

fn inspect_index(
    home: &CodexHome,
    path: &Path,
    sources: &mut Vec<CodexSource>,
    identities: &mut BTreeSet<PathBuf>,
    diagnostics: &mut Vec<DiscoveryDiagnostic>,
) -> (u64, u64) {
    if !path.exists() {
        diagnostics.push(DiscoveryDiagnostic {
            code: "codex_session_index_missing",
            severity: DiscoverySeverity::Info,
            path: home.original_path.join("session_index.jsonl"),
            message: "optional session index is absent".to_owned(),
        });
        return (0, 0);
    }
    let Some(source) = source_from_path(home, path, CodexSourceKind::SessionIndex) else {
        diagnostics.push(unreadable_source_diagnostic(home, path));
        return (0, 0);
    };
    let readable = source.readable;
    identities.insert(source.identity.clone());
    sources.push(source);
    if !readable {
        diagnostics.push(unreadable_source_diagnostic(home, path));
        return (0, 0);
    }

    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) => {
            diagnostics.push(DiscoveryDiagnostic {
                code: "codex_session_index_unreadable",
                severity: DiscoverySeverity::Warning,
                path: original_for(home, path),
                message: error.kind().to_string(),
            });
            return (0, 0);
        }
    };
    let mut valid = 0;
    let mut malformed = 0;
    for (line_number, line) in BufReader::new(file).lines().enumerate() {
        match line {
            Ok(line)
                if serde_json::from_str::<serde_json::Value>(&line)
                    .is_ok_and(|value| value.is_object()) =>
            {
                valid += 1;
            }
            Ok(_) => {
                malformed += 1;
                diagnostics.push(index_diagnostic(
                    home,
                    path,
                    "codex_session_index_entry_malformed",
                    format!("invalid JSON object at line {}", line_number + 1),
                ));
            }
            Err(error) => {
                malformed += 1;
                diagnostics.push(index_diagnostic(
                    home,
                    path,
                    "codex_session_index_entry_unreadable",
                    format!("line {}: {}", line_number + 1, error.kind()),
                ));
            }
        }
    }
    (valid, malformed)
}

fn index_diagnostic(
    home: &CodexHome,
    path: &Path,
    code: &'static str,
    message: String,
) -> DiscoveryDiagnostic {
    DiscoveryDiagnostic {
        code,
        severity: DiscoverySeverity::Warning,
        path: original_for(home, path),
        message,
    }
}

fn inspect_rollouts(
    home: &CodexHome,
    sources: &mut Vec<CodexSource>,
    identities: &mut BTreeSet<PathBuf>,
    diagnostics: &mut Vec<DiscoveryDiagnostic>,
) {
    let sessions = home.readable_path.join("sessions");
    if sessions.exists() {
        visit_rollout_tree(home, &sessions, 0, sources, identities, diagnostics);
    }
}

fn visit_rollout_tree(
    home: &CodexHome,
    directory: &Path,
    depth: usize,
    sources: &mut Vec<CodexSource>,
    identities: &mut BTreeSet<PathBuf>,
    diagnostics: &mut Vec<DiscoveryDiagnostic>,
) {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) => {
            diagnostics.push(DiscoveryDiagnostic {
                code: "codex_sessions_directory_unreadable",
                severity: DiscoverySeverity::Warning,
                path: original_for(home, directory),
                message: error.kind().to_string(),
            });
            return;
        }
    };
    let mut paths = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    paths.sort();
    for path in paths {
        if path.is_dir() && depth < 3 && is_date_component(&path, depth) {
            visit_rollout_tree(home, &path, depth + 1, sources, identities, diagnostics);
        } else if depth == 3 && is_rollout_file(&path) {
            match source_from_path(home, &path, CodexSourceKind::Rollout) {
                Some(source) if identities.insert(source.identity.clone()) => {
                    if !source.readable {
                        diagnostics.push(unreadable_source_diagnostic(home, &path));
                    }
                    sources.push(source);
                }
                Some(_) => {}
                None => diagnostics.push(unreadable_source_diagnostic(home, &path)),
            }
        }
    }
}

fn is_date_component(path: &Path, depth: usize) -> bool {
    let expected_length = [4, 2, 2][depth];
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            name.len() == expected_length && name.bytes().all(|byte| byte.is_ascii_digit())
        })
}

fn is_rollout_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with("rollout-"))
        && path.extension() == Some(OsStr::new("jsonl"))
        && path.is_file()
}

fn source_from_path(home: &CodexHome, path: &Path, kind: CodexSourceKind) -> Option<CodexSource> {
    let identity = fs::canonicalize(path).ok()?;
    let metadata = fs::metadata(&identity).ok()?;
    let readable = is_file_readable(&metadata) && fs::File::open(&identity).is_ok();
    Some(CodexSource {
        identity,
        readable_path: path.to_path_buf(),
        original_path: original_for(home, path),
        kind,
        permissions: source_permissions(&metadata),
        readable,
    })
}

fn original_for(home: &CodexHome, path: &Path) -> PathBuf {
    path.strip_prefix(&home.readable_path).map_or_else(
        |_| home.original_path.clone(),
        |relative| home.original_path.join(relative),
    )
}

fn unreadable_source_diagnostic(home: &CodexHome, path: &Path) -> DiscoveryDiagnostic {
    DiscoveryDiagnostic {
        code: "codex_source_unreadable",
        severity: DiscoverySeverity::Warning,
        path: original_for(home, path),
        message: "source cannot be opened read-only".to_owned(),
    }
}

#[cfg(unix)]
fn is_directory_readable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    fs::metadata(path).is_ok_and(|metadata| {
        let mode = metadata.permissions().mode();
        mode & 0o444 != 0 && mode & 0o111 != 0
    })
}

#[cfg(not(unix))]
fn is_directory_readable(path: &Path) -> bool {
    fs::metadata(path).is_ok()
}

#[cfg(unix)]
fn is_file_readable(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;

    metadata.permissions().mode() & 0o444 != 0
}

#[cfg(not(unix))]
fn is_file_readable(_metadata: &fs::Metadata) -> bool {
    true
}

#[cfg(unix)]
fn source_permissions(metadata: &fs::Metadata) -> SourcePermissions {
    use std::os::unix::fs::PermissionsExt;

    SourcePermissions {
        readonly: metadata.permissions().readonly(),
        unix_mode: Some(metadata.permissions().mode() & 0o777),
    }
}

#[cfg(not(unix))]
fn source_permissions(metadata: &fs::Metadata) -> SourcePermissions {
    SourcePermissions {
        readonly: metadata.permissions().readonly(),
        unix_mode: None,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    struct Fixture {
        directory: TempDir,
        home: PathBuf,
        rollout: PathBuf,
    }

    impl Fixture {
        fn populated() -> Self {
            let directory = tempfile::tempdir().expect("temporary directory should exist");
            let home = directory.path().join("codex-home");
            let dated = home.join("sessions/2026/07/20");
            fs::create_dir_all(&dated).expect("dated directory should exist");
            let rollout = dated.join("rollout-a.jsonl");
            fs::write(&rollout, b"{\"type\":\"event\"}\n").expect("rollout should write");
            fs::write(
                home.join("session_index.jsonl"),
                b"{\"id\":\"valid\"}\nnot-json\n",
            )
            .expect("index should write");
            Self {
                directory,
                home,
                rollout,
            }
        }

        fn input(&self) -> CodexDiscoveryInput {
            CodexDiscoveryInput {
                explicit_homes: vec![CodexHome {
                    readable_path: self.home.clone(),
                    original_path: PathBuf::from("~/.codex"),
                }],
                environment_home: None,
                user_home: self.directory.path().join("unused"),
            }
        }
    }

    #[test]
    fn discovers_rollout_and_malformed_index_entries() {
        let fixture = Fixture::populated();
        let report = discover(&fixture.input());
        let installation = &report.installations[0];

        assert_eq!(installation.sources.len(), 2);
        assert_eq!(installation.valid_index_entries, 1);
        assert_eq!(installation.malformed_index_entries, 1);
        assert_eq!(installation.surfaces, vec![CodexSurface::Cli]);
        assert!(installation.sources[1].permissions.unix_mode.is_some());
        assert_eq!(
            installation.sources[1].original_path,
            PathBuf::from("~/.codex/sessions/2026/07/20/rollout-a.jsonl")
        );
    }

    #[test]
    fn explicit_home_overrides_environment_and_default() {
        let fixture = Fixture::populated();
        let mut input = fixture.input();
        input.environment_home = Some(CodexHome {
            readable_path: fixture.directory.path().join("environment"),
            original_path: PathBuf::from("/environment"),
        });
        let report = discover(&input);

        assert_eq!(report.installations.len(), 1);
        assert_eq!(report.installations[0].origin, HomeOrigin::Explicit);
        assert!(
            !report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.path == Path::new("/environment"))
        );
    }

    #[test]
    fn environment_home_overrides_standard_home() {
        let fixture = Fixture::populated();
        let report = discover(&CodexDiscoveryInput {
            explicit_homes: Vec::new(),
            environment_home: Some(CodexHome {
                readable_path: fixture.home,
                original_path: PathBuf::from("$CODEX_HOME"),
            }),
            user_home: fixture.directory.path().join("standard"),
        });

        assert_eq!(report.installations[0].origin, HomeOrigin::Environment);
        assert_eq!(
            report.installations[0].original_home,
            PathBuf::from("$CODEX_HOME")
        );
    }

    #[test]
    fn default_home_is_used_without_overrides() {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        fs::create_dir(directory.path().join(".codex")).expect("default home should exist");
        let report = discover(&CodexDiscoveryInput {
            explicit_homes: Vec::new(),
            environment_home: None,
            user_home: directory.path().to_path_buf(),
        });

        assert_eq!(report.installations[0].origin, HomeOrigin::Default);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_home_and_rollout_aliases_are_deduplicated() {
        use std::os::unix::fs::symlink;

        let fixture = Fixture::populated();
        let alias = fixture.directory.path().join("alias");
        symlink(&fixture.home, &alias).expect("home symlink should exist");
        symlink(
            &fixture.rollout,
            fixture.home.join("sessions/2026/07/20/rollout-alias.jsonl"),
        )
        .expect("rollout symlink should exist");
        let report = discover(&CodexDiscoveryInput {
            explicit_homes: vec![
                CodexHome {
                    readable_path: fixture.home,
                    original_path: PathBuf::from("~/.codex"),
                },
                CodexHome {
                    readable_path: alias,
                    original_path: PathBuf::from("/alias"),
                },
            ],
            environment_home: None,
            user_home: PathBuf::from("/unused"),
        });

        assert_eq!(report.installations.len(), 1);
        assert_eq!(report.installations[0].sources.len(), 2);
    }

    #[test]
    fn multiple_installations_and_repeated_discovery_are_deterministic() {
        let first = Fixture::populated();
        let second = Fixture::populated();
        let input = CodexDiscoveryInput {
            explicit_homes: vec![
                CodexHome {
                    readable_path: second.home,
                    original_path: PathBuf::from("/second"),
                },
                CodexHome {
                    readable_path: first.home,
                    original_path: PathBuf::from("/first"),
                },
            ],
            environment_home: None,
            user_home: PathBuf::from("/unused"),
        };

        assert_eq!(discover(&input), discover(&input));
        assert_eq!(discover(&input).installations.len(), 2);
    }

    #[test]
    fn absent_index_and_missing_home_are_non_fatal() {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let home = directory.path().join("codex");
        fs::create_dir(&home).expect("partial home should exist");
        let partial = discover(&CodexDiscoveryInput {
            explicit_homes: vec![CodexHome {
                readable_path: home,
                original_path: PathBuf::from("~/.codex"),
            }],
            environment_home: None,
            user_home: PathBuf::from("/unused"),
        });
        let missing = discover(&CodexDiscoveryInput {
            explicit_homes: vec![CodexHome {
                readable_path: directory.path().join("missing"),
                original_path: PathBuf::from("/missing"),
            }],
            environment_home: None,
            user_home: PathBuf::from("/unused"),
        });

        assert_eq!(partial.installations.len(), 1);
        assert_eq!(partial.diagnostics[0].code, "codex_session_index_missing");
        assert!(missing.installations.is_empty());
        assert_eq!(missing.diagnostics[0].code, "codex_home_missing");
    }

    #[cfg(unix)]
    #[test]
    fn stale_symlink_and_unreadable_home_are_reported() {
        use std::os::unix::fs::{PermissionsExt, symlink};

        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let stale = directory.path().join("stale");
        symlink(directory.path().join("removed"), &stale).expect("stale symlink should exist");
        let unreadable = directory.path().join("unreadable");
        fs::create_dir(&unreadable).expect("unreadable home should exist");
        fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o000))
            .expect("permissions should change");
        let report = discover(&CodexDiscoveryInput {
            explicit_homes: vec![
                CodexHome {
                    readable_path: stale,
                    original_path: PathBuf::from("/stale"),
                },
                CodexHome {
                    readable_path: unreadable,
                    original_path: PathBuf::from("/unreadable"),
                },
            ],
            environment_home: None,
            user_home: PathBuf::from("/unused"),
        });

        assert!(report.installations.is_empty());
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "codex_home_stale")
        );
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "codex_home_unreadable")
        );
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_rollout_is_reported_without_aborting() {
        use std::os::unix::fs::PermissionsExt;

        let fixture = Fixture::populated();
        fs::set_permissions(&fixture.rollout, fs::Permissions::from_mode(0o000))
            .expect("permissions should change");
        let report = discover(&fixture.input());

        assert!(!report.installations[0].sources[1].readable);
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "codex_source_unreadable")
        );
    }

    #[test]
    fn discovery_does_not_modify_native_files() {
        let fixture = Fixture::populated();
        let before_rollout = fs::read(&fixture.rollout).expect("rollout should read");
        let index = fixture.home.join("session_index.jsonl");
        let before_index = fs::read(&index).expect("index should read");

        let _first = discover(&fixture.input());
        let _second = discover(&fixture.input());

        assert_eq!(fs::read(&fixture.rollout).unwrap(), before_rollout);
        assert_eq!(fs::read(index).unwrap(), before_index);
    }
}
