//! `SessionMesh` daemon process entry point.

use std::collections::BTreeMap;
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
use sessionmesh_storage::Storage;
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
        user_home,
    };
    tokio::spawn(run_codex_ingestion(
        ingestion_state,
        discovery_input,
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

async fn run_codex_ingestion(
    state: ApiState,
    discovery_input: CodexDiscoveryInput,
    interval_ms: u64,
) {
    let mut interval = tokio::time::interval(std::time::Duration::from_millis(interval_ms));
    loop {
        interval.tick().await;
        scan_codex_once(&state, &discovery_input).await;
    }
}

async fn scan_codex_once(state: &ApiState, discovery_input: &CodexDiscoveryInput) {
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
            for event in &prepared.batch.events {
                state.publish(event);
            }
        }
    }
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
    use sessionmesh_handoff::{HandoffInput, RepositoryState, generate, snapshot_id};
    use sessionmesh_mcp::McpServer;
    use sessionmesh_storage::{
        EventRepository, MembershipDecision, StoredGlobalSession, StoredHandoff,
    };

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

    async fn create_e2e_handoff(
        storage: &Storage,
        native_session_id: &str,
        events: Vec<sessionmesh_core::event::CanonicalEvent>,
    ) -> StoredGlobalSession {
        let global = StoredGlobalSession {
            id: "gs_e2e".to_owned(),
            objective: "Complete the fixture journey".to_owned(),
            created_at: "2026-07-20T14:30:00Z".to_owned(),
            updated_at: "2026-07-20T14:30:00Z".to_owned(),
        };
        storage.create_global_session(&global).await.unwrap();
        storage
            .link_session(
                &MembershipDecision {
                    global_session_id: &global.id,
                    native_session_id,
                    actor: "e2e-test",
                    reason: None,
                    created_at: "2026-07-20T14:31:00Z",
                },
                1.0,
                "explicit-v1",
            )
            .await
            .unwrap();
        let member_events = events
            .into_iter()
            .filter(|event| event.native_session_id == native_session_id)
            .collect::<Vec<_>>();
        let handoff = generate(
            HandoffInput {
                global_session_id: &global.id,
                objective: &global.objective,
                events: &member_events,
                records: &[],
                repository: RepositoryState {
                    branch: None,
                    head: None,
                    dirty: false,
                },
                token_budget: 4_000,
                model_timeout: std::time::Duration::from_millis(10),
            },
            None,
        )
        .await
        .unwrap();
        let snapshot = snapshot_id(&handoff).unwrap();
        let handoff_json = serde_json::to_string(&handoff).unwrap();
        storage
            .store_handoff(
                &StoredHandoff {
                    id: "handoff_e2e".to_owned(),
                    global_session_id: global.id.clone(),
                    snapshot_id: snapshot,
                    schema_version: "1.0".to_owned(),
                    handoff_json: handoff_json.clone(),
                    created_at: "2026-07-20T14:32:00Z".to_owned(),
                },
                &handoff_json,
            )
            .await
            .unwrap();
        global
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

        let handoff_session_id = events
            .iter()
            .find(|event| {
                matches!(
                    event.kind,
                    sessionmesh_core::event::EventKind::Decision
                        | sessionmesh_core::event::EventKind::Task
                        | sessionmesh_core::event::EventKind::Plan
                        | sessionmesh_core::event::EventKind::FileRead
                        | sessionmesh_core::event::EventKind::FileWrite
                        | sessionmesh_core::event::EventKind::Patch
                        | sessionmesh_core::event::EventKind::CommandResult
                )
            })
            .map(|event| event.native_session_id.clone())
            .expect("fixture must contain a handoff-relevant event");
        let global = create_e2e_handoff(&storage, &handoff_session_id, events).await;
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
