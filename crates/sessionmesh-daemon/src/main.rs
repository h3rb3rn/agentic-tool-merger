//! `SessionMesh` daemon process entry point.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt::Write as FmtWrite;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use sessionmesh_api::{ApiState, router};
use sessionmesh_core::configuration::{ConfigInputs, ConfigLayer, resolve, user_config_path};
use sessionmesh_handoff::{LocalExtractor, OpenAiCompatibleExtractor};
use sessionmesh_ingest::codex::{
    CodexDiscoveryInput, CodexHome, CodexSourceKind, IncrementalInput, discover, ingest_file,
};
use sessionmesh_ingest::external::{
    ExternalSource, discover_claude, discover_continue, ingest_external,
};
use sessionmesh_storage::{MembershipDecision, Storage, StoredGlobalSession};
use sha2::{Digest, Sha256};
use tower_http::services::{ServeDir, ServeFile};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let environment = std::env::vars().collect::<BTreeMap<_, _>>();
    let user_home = environment
        .get("HOME")
        .map(PathBuf::from)
        .ok_or("HOME is required")?;
    let user_config = fs::read_to_string(user_config_path(&user_home)).ok();
    let config = resolve(ConfigInputs {
        user_home: &user_home,
        platform_data_home: environment.get("XDG_DATA_HOME").map(Path::new),
        user_toml: user_config.as_deref(),
        environment: &environment,
        cli: ConfigLayer::default(),
    })?;
    let storage = Storage::open(&config.database_path.value, &config.blob_store_path.value).await?;
    let token = provision_local_token(&config.home.value)?;
    let extractor: Option<std::sync::Arc<dyn LocalExtractor>> = config
        .llm_endpoint
        .value
        .as_ref()
        .zip(config.model.value.as_ref())
        .map(|(endpoint, model)| {
            std::sync::Arc::new(OpenAiCompatibleExtractor::new(
                endpoint.as_str(),
                model.clone(),
            )) as std::sync::Arc<dyn LocalExtractor>
        });
    let token_budget =
        usize::try_from(config.token_budget.value).map_err(|_| "token budget is unsupported")?;
    let state = ApiState::new(storage, token, Vec::new()).with_handoff(extractor, token_budget);
    let ingestion_state = state.clone();
    let discovery_input = CodexDiscoveryInput {
        explicit_homes: config
            .source_mappings
            .value
            .iter()
            .map(|mapping| CodexHome {
                readable_path: mapping.readable_path.clone(),
                original_path: mapping.original_path.clone(),
            })
            .collect(),
        environment_home: environment.get("CODEX_HOME").map(|path| CodexHome {
            readable_path: PathBuf::from(path),
            original_path: PathBuf::from(path),
        }),
        user_home: user_home.clone(),
    };
    let profile_root = environment
        .get("SESSIONMESH_PROFILE_ROOT")
        .map_or_else(|| user_home.clone(), PathBuf::from);
    let original_profile_root = environment
        .get("SESSIONMESH_PROFILE_ORIGINAL_ROOT")
        .map_or_else(|| user_home.clone(), PathBuf::from);
    let external_sources = external_sources(&profile_root, &original_profile_root);
    tokio::spawn(run_ingestion(
        ingestion_state,
        discovery_input,
        external_sources,
        config.watch_debounce_ms.value,
    ));
    let address = SocketAddr::new(config.bind_address.value, config.port.value);
    let listener = tokio::net::TcpListener::bind(address).await?;
    let web_root = web_root();
    let application = router(state).fallback_service(
        ServeDir::new(&web_root).fallback(ServeFile::new(web_root.join("index.html"))),
    );
    axum::serve(listener, application).await?;
    Ok(())
}

async fn run_ingestion(
    state: ApiState,
    discovery_input: CodexDiscoveryInput,
    external_sources: Vec<ExternalSource>,
    interval_ms: u64,
) {
    let mut interval = tokio::time::interval(std::time::Duration::from_millis(interval_ms));
    loop {
        interval.tick().await;
        let mut changed = scan_codex_once(&state, &discovery_input).await;
        changed |= scan_external_once(&state, &external_sources).await;
        if changed {
            let _ = reconcile_and_refresh(&state).await;
        }
    }
}

async fn scan_codex_once(state: &ApiState, discovery_input: &CodexDiscoveryInput) -> bool {
    let mut changed = false;
    let report = discover(discovery_input);
    for source in report
        .installations
        .iter()
        .flat_map(|installation| &installation.sources)
        .filter(|source| source.kind == CodexSourceKind::Rollout && source.readable)
    {
        let imported_at =
            chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now()).to_rfc3339();
        let source_label = source.identity.to_string_lossy();
        let fallback_session_id = source
            .readable_path
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("codex-session")
            .to_owned();
        let input = IncrementalInput {
            source_id: format!("codex:{source_label}"),
            source_path: source.readable_path.clone(),
            original_path: Some(source.original_path.to_string_lossy().into_owned()),
            adapter_version: env!("CARGO_PKG_VERSION").to_owned(),
            fallback_session_id,
            imported_at,
        };
        if let Ok(prepared) = ingest_file(state.storage(), &input).await {
            changed |= !prepared.batch.events.is_empty();
            for event in &prepared.batch.events {
                state.publish(event);
            }
        }
    }
    changed
}

fn external_sources(profile_root: &Path, original_root: &Path) -> Vec<ExternalSource> {
    let mut sources = discover_claude(
        &profile_root.join(".claude"),
        &original_root.join(".claude"),
    );
    sources.extend(discover_continue(
        &profile_root.join(".continue"),
        &original_root.join(".continue"),
    ));
    sources
}

async fn scan_external_once(state: &ApiState, sources: &[ExternalSource]) -> bool {
    let mut changed = false;
    for source in sources {
        let imported_at =
            chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now()).to_rfc3339();
        if let Ok(import) = ingest_external(state.storage(), source, &imported_at).await {
            changed |= import.changed;
            for event in &import.events {
                state.publish(event);
            }
        }
    }
    changed
}

#[derive(Clone, Debug)]
struct SessionSummary {
    id: String,
    cwd: Option<String>,
    objective: String,
    started_at: chrono::DateTime<chrono::FixedOffset>,
    ended_at: chrono::DateTime<chrono::FixedOffset>,
}

async fn reconcile_and_refresh(state: &ApiState) -> Result<(), Box<dyn Error + Send + Sync>> {
    let summaries = session_summaries(state.storage()).await?;
    let mut memberships = state.storage().list_all_session_members().await?;
    let globals = state.storage().list_global_sessions().await?;
    let mut touched = BTreeSet::new();
    for summary in &summaries {
        if memberships
            .iter()
            .any(|member| member.native_session_id == summary.id)
        {
            continue;
        }
        let candidate = memberships
            .iter()
            .filter_map(|member| {
                let other = summaries
                    .iter()
                    .find(|candidate| candidate.id == member.native_session_id)?;
                same_work_context(summary, other)
                    .then_some((member.global_session_id.clone(), other))
            })
            .min_by_key(|(_, other)| temporal_gap(summary, other));
        let (global_id, confidence, version) = if let Some((global_id, _)) = candidate {
            (global_id, 0.85, "workspace-temporal-v1")
        } else {
            let global_id = global_id(&summary.id);
            let now = summary.started_at.to_rfc3339();
            state
                .storage()
                .create_global_session(&StoredGlobalSession {
                    id: global_id.clone(),
                    objective: summary.objective.clone(),
                    created_at: now.clone(),
                    updated_at: now,
                })
                .await?;
            (global_id, 1.0, "automatic-seed-v1")
        };
        let linked_at = summary.ended_at.to_rfc3339();
        state
            .storage()
            .link_session(
                &MembershipDecision {
                    global_session_id: &global_id,
                    native_session_id: &summary.id,
                    actor: "automatic-correlator",
                    reason: Some("same workspace and bounded temporal proximity"),
                    created_at: &linked_at,
                },
                confidence,
                version,
            )
            .await?;
        state
            .storage()
            .touch_global_session(&global_id, &linked_at)
            .await?;
        memberships.push(sessionmesh_storage::StoredSessionMember {
            global_session_id: global_id.clone(),
            native_session_id: summary.id.clone(),
            confidence,
            correlation_version: version.to_owned(),
            manual_state: Some("accepted".to_owned()),
        });
        touched.insert(global_id);
    }
    for global in globals {
        if memberships
            .iter()
            .any(|member| member.global_session_id == global.id)
        {
            touched.insert(global.id);
        }
    }
    for global_id in touched {
        let _ = state.refresh_global_handoff(&global_id).await;
    }
    Ok(())
}

async fn session_summaries(
    storage: &Storage,
) -> Result<Vec<SessionSummary>, Box<dyn Error + Send + Sync>> {
    let mut grouped = BTreeMap::<String, Vec<sessionmesh_core::event::CanonicalEvent>>::new();
    for event in storage.list_canonical_events().await? {
        grouped
            .entry(event.native_session_id.clone())
            .or_default()
            .push(event);
    }
    let mut summaries = Vec::new();
    for (id, mut events) in grouped {
        events.sort_by(|left, right| {
            left.timestamp
                .as_str()
                .cmp(right.timestamp.as_str())
                .then(left.sequence.cmp(&right.sequence))
        });
        let Some(started_at) = events
            .first()
            .and_then(|event| chrono::DateTime::parse_from_rfc3339(event.timestamp.as_str()).ok())
        else {
            continue;
        };
        let ended_at = events
            .last()
            .and_then(|event| chrono::DateTime::parse_from_rfc3339(event.timestamp.as_str()).ok())
            .unwrap_or(started_at);
        let cwd = events
            .iter()
            .find_map(|event| event.workspace.as_ref()?.cwd.as_deref().map(normalize_cwd));
        let objective = events
            .iter()
            .find(|event| event.kind == sessionmesh_core::event::EventKind::UserMessage)
            .and_then(|event| event.payload.get("text"))
            .and_then(serde_json::Value::as_str)
            .filter(|text| !text.trim().is_empty())
            .map_or_else(
                || {
                    cwd.as_ref().map_or_else(
                        || "Continue agent work".to_owned(),
                        |cwd| format!("Continue work in {cwd}"),
                    )
                },
                compact_objective,
            );
        summaries.push(SessionSummary {
            id,
            cwd,
            objective,
            started_at,
            ended_at,
        });
    }
    summaries.sort_by_key(|summary| summary.started_at);
    Ok(summaries)
}

fn same_work_context(left: &SessionSummary, right: &SessionSummary) -> bool {
    left.cwd.is_some() && left.cwd == right.cwd && temporal_gap(left, right) <= 7 * 24 * 60 * 60
}

fn temporal_gap(left: &SessionSummary, right: &SessionSummary) -> i64 {
    if left.started_at > right.ended_at {
        (left.started_at - right.ended_at).num_seconds()
    } else if right.started_at > left.ended_at {
        (right.started_at - left.ended_at).num_seconds()
    } else {
        0
    }
}

fn normalize_cwd(cwd: &str) -> String {
    cwd.trim_end_matches('/').to_owned()
}

fn compact_objective(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(240)
        .collect()
}

fn global_id(seed: &str) -> String {
    let digest = Sha256::digest(seed.as_bytes());
    let mut id = String::from("gs_");
    for byte in &digest[..16] {
        write!(&mut id, "{byte:02x}").expect("writing to a String cannot fail");
    }
    id
}

fn web_root() -> PathBuf {
    std::env::var_os("SESSIONMESH_WEB_ROOT").map_or_else(
        || PathBuf::from("/usr/share/sessionmesh/web"),
        PathBuf::from,
    )
}

fn provision_local_token(home: &Path) -> Result<String, Box<dyn Error>> {
    fs::create_dir_all(home)?;
    set_directory_permissions(home)?;
    let token_path = home.join("api-token");
    if token_path.exists() {
        let token = fs::read_to_string(&token_path)?;
        validate_token(&token)?;
        return Ok(token);
    }

    let mut random = [0_u8; 32];
    getrandom::fill(&mut random)
        .map_err(|error| std::io::Error::other(format!("token entropy failed: {error}")))?;
    let mut token = String::with_capacity(64);
    for byte in random {
        write!(&mut token, "{byte:02x}")?;
    }
    let temporary = home.join(format!(".api-token.{}.tmp", std::process::id()));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)?;
    file.write_all(token.as_bytes())?;
    file.sync_all()?;
    set_file_permissions(&temporary)?;
    drop(file);
    match fs::hard_link(&temporary, &token_path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            fs::remove_file(&temporary)?;
            let existing = fs::read_to_string(&token_path)?;
            validate_token(&existing)?;
            return Ok(existing);
        }
        Err(error) => return Err(error.into()),
    }
    fs::remove_file(temporary)?;
    Ok(token)
}

fn validate_token(token: &str) -> Result<(), &'static str> {
    if token.len() == 64
        && token
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(())
    } else {
        Err("api-token is malformed; remove it to provision a new token")
    }
}

#[cfg(unix)]
fn set_directory_permissions(path: &Path) -> Result<(), std::io::Error> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn set_directory_permissions(_path: &Path) -> Result<(), std::io::Error> {
    Ok(())
}

#[cfg(unix)]
fn set_file_permissions(path: &Path) -> Result<(), std::io::Error> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn set_file_permissions(_path: &Path) -> Result<(), std::io::Error> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use sessionmesh_mcp::McpServer;
    use sessionmesh_storage::EventRepository;

    use super::*;

    #[test]
    fn token_is_persistent_valid_and_user_only() {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let first = provision_local_token(directory.path()).expect("token should provision");
        let second = provision_local_token(directory.path()).expect("token should reopen");

        assert_eq!(first, second);
        validate_token(&first).expect("generated token should validate");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(directory.path().join("api-token"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600);
        }
    }

    #[tokio::test]
    async fn codex_fixture_journey_is_idempotent_traceable_and_mcp_retrievable() {
        let directory = tempfile::tempdir().unwrap();
        let codex_home = directory.path().join("codex");
        let rollout_directory = codex_home.join("sessions/2026/07/20");
        fs::create_dir_all(&rollout_directory).unwrap();
        let rollout = rollout_directory.join("rollout-e2e.jsonl");
        let mut native_bytes =
            include_bytes!("../../../fixtures/codex/parser/rollout.jsonl").to_vec();
        native_bytes.extend_from_slice(
            br#"{"timestamp":"2026-07-20T14:30:06Z","type":"event_msg","payload":{"type":"plan_update","text":"Verify the complete local-first journey"}}"#,
        );
        native_bytes.push(b'\n');
        fs::write(&rollout, &native_bytes).unwrap();
        let storage = Storage::open(
            directory.path().join("state/sessionmesh.db"),
            directory.path().join("state/blobs"),
        )
        .await
        .unwrap();
        let state = ApiState::new(storage.clone(), "test-token", Vec::new());
        let discovery = CodexDiscoveryInput {
            explicit_homes: vec![CodexHome {
                readable_path: codex_home.clone(),
                original_path: PathBuf::from("~/.codex"),
            }],
            environment_home: None,
            user_home: directory.path().join("isolated-home"),
        };

        let started = std::time::Instant::now();
        scan_codex_once(&state, &discovery).await;
        let first_count = storage.event_count().await.unwrap();
        scan_codex_once(&state, &discovery).await;
        let elapsed = started.elapsed();

        assert!(first_count > 0);
        assert_eq!(storage.event_count().await.unwrap(), first_count);
        assert_eq!(fs::read(&rollout).unwrap(), native_bytes);
        assert!(elapsed < std::time::Duration::from_secs(5));
        let events = storage.list_canonical_events().await.unwrap();
        for event in &events {
            assert!(event.provenance.source_offset < native_bytes.len() as u64);
        }
        let raw_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM raw_objects")
            .fetch_one(storage.pool())
            .await
            .unwrap();
        assert!(raw_count > 0);
        let untraceable: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM native_events WHERE raw_object_id IS NULL")
                .fetch_one(storage.pool())
                .await
                .unwrap();
        assert_eq!(untraceable, 0);

        reconcile_and_refresh(&state).await.unwrap();
        let global = storage.list_global_sessions().await.unwrap().pop().unwrap();
        assert!(
            !storage
                .list_session_members(&global.id)
                .await
                .unwrap()
                .is_empty()
        );
        assert!(storage.latest_handoff(&global.id).await.unwrap().is_some());
        let mcp = McpServer::new(storage, Some(global.id), false);
        let response = mcp
            .handle(serde_json::json!({
                "jsonrpc":"2.0","id":1,"method":"tools/call",
                "params":{"name":"sessionmesh_get_handoff","arguments":{}}
            }))
            .await
            .unwrap();
        assert_eq!(response["result"]["isError"], false);
        assert_eq!(
            response["result"]["structuredContent"]["delivery"]["ingestion_excluded"],
            true
        );
    }
}
