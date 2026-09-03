//! Stateless remote ingestion collector.
//!
//! Runs on a dedicated system that is reachable only over the network and
//! that the `SessionMesh` daemon cannot mount a local filesystem from.
//! Reuses the exact discovery and parsing code the daemon uses for local
//! ingestion (`sessionmesh_ingest`), but instead of committing to a local
//! `SessionMesh` database it pushes newly prepared batches to the daemon's
//! network ingestion API.
//!
//! Holds no local state of its own: before every scan of a source it asks
//! the daemon for that source's last committed cursor, so a restarted or
//! freshly reprovisioned collector always resumes from exactly where the
//! daemon last confirmed receipt, without a local database to lose or
//! desynchronize.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::path::{Path, PathBuf};
use std::time::Duration;

use reqwest::{Client, StatusCode};
use sessionmesh_ingest::codex::{
    CodexDiscoveryInput, CodexHome, CodexSourceKind, IncrementalInput, discover,
    prepare_incremental,
};
use sessionmesh_ingest::external::{
    ExternalSource, discover_agy, discover_claude, discover_continue, external_source_id,
    prepare_external,
};
use sessionmesh_ingest::opencode::{
    OpenCodeSource, discover_opencode, opencode_source_id, prepare_opencode,
};
use sessionmesh_ingest_wire::{IngestBatchRequest, IngestCursorResponse};
use sessionmesh_storage::IngestionBatch;

const DEFAULT_SCAN_INTERVAL_MS: u64 = 2_000;
const MAX_RETRY_ATTEMPTS: u32 = 5;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let environment = std::env::vars().collect::<BTreeMap<_, _>>();
    let config = CollectorConfig::from_environment(&environment)?;
    let client = Client::builder().timeout(REQUEST_TIMEOUT).build()?;

    let discovery_input = CodexDiscoveryInput {
        explicit_homes: Vec::new(),
        environment_home: environment.get("CODEX_HOME").map(|path| CodexHome {
            readable_path: PathBuf::from(path),
            original_path: PathBuf::from(path),
        }),
        user_home: config.user_home.clone(),
    };

    let mut scan_interval = tokio::time::interval(Duration::from_millis(config.scan_interval_ms));
    loop {
        scan_interval.tick().await;
        scan_codex(&client, &config, &discovery_input).await;
        for source in external_sources(&config.profile_root, &config.original_profile_root) {
            scan_external(&client, &config, &source).await;
        }
        if let Some(source) = discover_opencode(&config.profile_root, &config.original_profile_root)
        {
            scan_opencode(&client, &config, &source).await;
        }
    }
}

struct CollectorConfig {
    endpoint: String,
    token: String,
    scan_interval_ms: u64,
    user_home: PathBuf,
    profile_root: PathBuf,
    original_profile_root: PathBuf,
}

impl CollectorConfig {
    fn from_environment(environment: &BTreeMap<String, String>) -> Result<Self, Box<dyn Error>> {
        let endpoint = environment
            .get("SESSIONMESH_COLLECTOR_ENDPOINT")
            .ok_or("SESSIONMESH_COLLECTOR_ENDPOINT is required")?
            .trim_end_matches('/')
            .to_owned();
        let allow_insecure = environment
            .get("SESSIONMESH_COLLECTOR_ALLOW_INSECURE")
            .is_some_and(|value| value == "true");
        if !endpoint.starts_with("https://") && !allow_insecure {
            return Err("SESSIONMESH_COLLECTOR_ENDPOINT must use https:// \
                 (set SESSIONMESH_COLLECTOR_ALLOW_INSECURE=true to override \
                 for local testing only)"
                .into());
        }
        let token = environment
            .get("SESSIONMESH_COLLECTOR_TOKEN")
            .ok_or("SESSIONMESH_COLLECTOR_TOKEN is required")?
            .clone();
        let user_home = environment
            .get("HOME")
            .map(PathBuf::from)
            .ok_or("HOME is required")?;
        let profile_root = environment
            .get("SESSIONMESH_PROFILE_ROOT")
            .map_or_else(|| user_home.clone(), PathBuf::from);
        let original_profile_root = environment
            .get("SESSIONMESH_PROFILE_ORIGINAL_ROOT")
            .map_or_else(|| user_home.clone(), PathBuf::from);
        let scan_interval_ms = environment
            .get("SESSIONMESH_WATCH_DEBOUNCE_MS")
            .map(|value| value.parse::<u64>())
            .transpose()
            .map_err(|error| format!("SESSIONMESH_WATCH_DEBOUNCE_MS is invalid: {error}"))?
            .unwrap_or(DEFAULT_SCAN_INTERVAL_MS);
        Ok(Self {
            endpoint,
            token,
            scan_interval_ms,
            user_home,
            profile_root,
            original_profile_root,
        })
    }
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
    sources.extend(discover_agy(
        &profile_root.join(".gemini/antigravity-cli"),
        &original_root.join(".gemini/antigravity-cli"),
    ));
    sources
}

fn now_rfc3339() -> String {
    chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now()).to_rfc3339()
}

async fn scan_codex(
    client: &Client,
    config: &CollectorConfig,
    discovery_input: &CodexDiscoveryInput,
) {
    let report = discover(discovery_input);
    for source in report
        .installations
        .iter()
        .flat_map(|installation| &installation.sources)
        .filter(|source| source.kind == CodexSourceKind::Rollout && source.readable)
    {
        let source_id = format!("codex:{}", source.identity.to_string_lossy());
        let fallback_session_id = source
            .readable_path
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("codex-session")
            .to_owned();
        let prior = match fetch_cursor(client, config, &source_id).await {
            Ok(cursor) => cursor,
            Err(error) => {
                eprintln!("sessionmesh-collector: cursor lookup failed for {source_id}: {error}");
                continue;
            }
        };
        let input = IncrementalInput {
            source_id: source_id.clone(),
            source_path: source.readable_path.clone(),
            original_path: Some(source.original_path.to_string_lossy().into_owned()),
            adapter_version: env!("CARGO_PKG_VERSION").to_owned(),
            fallback_session_id,
            imported_at: now_rfc3339(),
        };
        let prepared = match prepare_incremental(&input, prior.as_ref()).await {
            Ok(prepared) => prepared,
            Err(error) => {
                eprintln!("sessionmesh-collector: prepare failed for {source_id}: {error}");
                continue;
            }
        };
        if prepared.batch.events.is_empty() && prepared.batch.raw_objects.is_empty() {
            continue;
        }
        if let Err(error) = push_batch(client, config, &prepared.batch).await {
            eprintln!("sessionmesh-collector: push failed for {source_id}: {error}");
        }
    }
}

async fn scan_external(client: &Client, config: &CollectorConfig, source: &ExternalSource) {
    let source_id = external_source_id(source);
    let prior = match fetch_cursor(client, config, &source_id).await {
        Ok(cursor) => cursor,
        Err(error) => {
            eprintln!("sessionmesh-collector: cursor lookup failed for {source_id}: {error}");
            return;
        }
    };
    let batch = match prepare_external(source, &now_rfc3339(), prior.as_ref()).await {
        Ok(batch) => batch,
        Err(error) => {
            eprintln!("sessionmesh-collector: prepare failed for {source_id}: {error}");
            return;
        }
    };
    let Some(batch) = batch else {
        return;
    };
    if let Err(error) = push_batch(client, config, &batch).await {
        eprintln!("sessionmesh-collector: push failed for {source_id}: {error}");
    }
}

async fn scan_opencode(client: &Client, config: &CollectorConfig, source: &OpenCodeSource) {
    let source_id = opencode_source_id(source);
    let prior = match fetch_cursor(client, config, &source_id).await {
        Ok(cursor) => cursor,
        Err(error) => {
            eprintln!("sessionmesh-collector: cursor lookup failed for {source_id}: {error}");
            return;
        }
    };
    let batch = match prepare_opencode(source, &now_rfc3339(), prior.as_ref()).await {
        Ok(batch) => batch,
        Err(error) => {
            eprintln!("sessionmesh-collector: prepare failed for {source_id}: {error}");
            return;
        }
    };
    let Some(batch) = batch else {
        return;
    };
    if let Err(error) = push_batch(client, config, &batch).await {
        eprintln!("sessionmesh-collector: push failed for {source_id}: {error}");
    }
}

async fn fetch_cursor(
    client: &Client,
    config: &CollectorConfig,
    source_id: &str,
) -> Result<Option<sessionmesh_storage::IngestionCursor>, CollectorError> {
    let url = format!("{}/api/v1/ingest/cursor", config.endpoint);
    let response = with_retry(|| {
        client
            .get(&url)
            .bearer_auth(&config.token)
            .query(&[("source_id", source_id)])
            .send()
    })
    .await?;
    let body: IngestCursorResponse = response.json().await.map_err(CollectorError::Http)?;
    body.cursor
        .map(|wire| wire.into_cursor(source_id.to_owned()))
        .transpose()
        .map_err(CollectorError::Decode)
}

async fn push_batch(
    client: &Client,
    config: &CollectorConfig,
    batch: &IngestionBatch,
) -> Result<(), CollectorError> {
    let request = IngestBatchRequest::from_batch(batch).map_err(CollectorError::Event)?;
    let url = format!("{}/api/v1/ingest/batch", config.endpoint);
    with_retry(|| {
        client
            .post(&url)
            .bearer_auth(&config.token)
            .json(&request)
            .send()
    })
    .await?;
    Ok(())
}

/// Retries transport failures and 5xx/429 responses with exponential
/// backoff, since a dedicated system's link to the daemon may be flaky or
/// momentarily rate-limited. Never retries 4xx (other than 429): those
/// indicate a request the daemon will never accept, and re-sending it would
/// only spam the audit log with identical rejections.
async fn with_retry<F, Fut>(mut request: F) -> Result<reqwest::Response, CollectorError>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<reqwest::Response, reqwest::Error>>,
{
    let mut attempt = 0_u32;
    loop {
        let outcome = request().await;
        let should_retry = match &outcome {
            Ok(response) => {
                let status = response.status();
                status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS
            }
            Err(error) => error.is_timeout() || error.is_connect(),
        };
        if !should_retry || attempt >= MAX_RETRY_ATTEMPTS {
            return match outcome {
                Ok(response) => response.error_for_status().map_err(CollectorError::Http),
                Err(error) => Err(CollectorError::Http(error)),
            };
        }
        attempt += 1;
        let backoff = Duration::from_millis(200_u64.saturating_mul(1_u64 << attempt.min(10)));
        tokio::time::sleep(backoff).await;
    }
}

#[derive(Debug)]
enum CollectorError {
    Http(reqwest::Error),
    Decode(base64::DecodeError),
    Event(sessionmesh_core::event::EventError),
}

impl Display for CollectorError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Http(error) => write!(formatter, "request failed: {error}"),
            Self::Decode(error) => write!(formatter, "malformed cursor from daemon: {error}"),
            Self::Event(error) => write!(formatter, "event serialization failed: {error}"),
        }
    }
}

impl std::error::Error for CollectorError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_environment() -> BTreeMap<String, String> {
        BTreeMap::from([
            (
                "SESSIONMESH_COLLECTOR_ENDPOINT".to_owned(),
                "https://sessionmesh.internal:8788".to_owned(),
            ),
            (
                "SESSIONMESH_COLLECTOR_TOKEN".to_owned(),
                "test-token".to_owned(),
            ),
            ("HOME".to_owned(), "/home/dedicated-user".to_owned()),
        ])
    }

    #[test]
    fn requires_an_endpoint_and_a_token() {
        let mut environment = base_environment();
        environment.remove("SESSIONMESH_COLLECTOR_ENDPOINT");
        assert!(CollectorConfig::from_environment(&environment).is_err());

        let mut environment = base_environment();
        environment.remove("SESSIONMESH_COLLECTOR_TOKEN");
        assert!(CollectorConfig::from_environment(&environment).is_err());
    }

    #[test]
    fn rejects_a_plain_http_endpoint_unless_explicitly_overridden() {
        let mut environment = base_environment();
        environment.insert(
            "SESSIONMESH_COLLECTOR_ENDPOINT".to_owned(),
            "http://sessionmesh.internal:8788".to_owned(),
        );
        assert!(CollectorConfig::from_environment(&environment).is_err());

        environment.insert(
            "SESSIONMESH_COLLECTOR_ALLOW_INSECURE".to_owned(),
            "true".to_owned(),
        );
        assert!(CollectorConfig::from_environment(&environment).is_ok());
    }

    #[test]
    fn defaults_profile_roots_to_home_and_trims_a_trailing_slash() {
        let mut environment = base_environment();
        environment.insert(
            "SESSIONMESH_COLLECTOR_ENDPOINT".to_owned(),
            "https://sessionmesh.internal:8788/".to_owned(),
        );
        let config = CollectorConfig::from_environment(&environment)
            .expect("valid environment should resolve");

        assert_eq!(config.endpoint, "https://sessionmesh.internal:8788");
        assert_eq!(config.profile_root, PathBuf::from("/home/dedicated-user"));
        assert_eq!(
            config.original_profile_root,
            PathBuf::from("/home/dedicated-user")
        );
        assert_eq!(config.scan_interval_ms, DEFAULT_SCAN_INTERVAL_MS);
    }

    #[test]
    fn rejects_a_malformed_scan_interval() {
        let mut environment = base_environment();
        environment.insert(
            "SESSIONMESH_WATCH_DEBOUNCE_MS".to_owned(),
            "not-a-number".to_owned(),
        );
        assert!(CollectorConfig::from_environment(&environment).is_err());
    }
}
