//! Versioned, authenticated local REST and streaming API.

use std::collections::BTreeMap;
use std::convert::Infallible;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use sessionmesh_core::event::{CanonicalEvent, EventKind};
use sessionmesh_handoff::{
    HANDOFF_SCHEMA_VERSION, Handoff, HandoffInput, LocalExtractor, RecordedFact, RepositoryState,
    generate, snapshot_id,
};
use sessionmesh_storage::{
    MembershipAuditEntry, MembershipDecision, Storage, StorageError, StoredGlobalSession,
    StoredHandoff, StoredNativeSession, StoredSessionMember,
};
use tokio::sync::broadcast;

const DEFAULT_PAGE_SIZE: usize = 50;
const MAX_PAGE_SIZE: usize = 200;

/// Shared local API state.
#[derive(Clone)]
pub struct ApiState {
    storage: Storage,
    auth_token: Arc<str>,
    tools: Arc<Vec<ToolStatus>>,
    events: broadcast::Sender<EventSummary>,
    extractor: Option<Arc<dyn LocalExtractor>>,
    handoff_token_budget: usize,
}

impl ApiState {
    /// Creates state with a bounded live-event channel.
    #[must_use]
    pub fn new(storage: Storage, auth_token: impl Into<Arc<str>>, tools: Vec<ToolStatus>) -> Self {
        let (events, _) = broadcast::channel(128);
        Self {
            storage,
            auth_token: auth_token.into(),
            tools: Arc::new(tools),
            events,
            extractor: None,
            handoff_token_budget: 4_000,
        }
    }

    /// Enables optional local-model extraction and configures the output
    /// budget. Deterministic operation remains available without this.
    #[must_use]
    pub fn with_handoff(
        mut self,
        extractor: Option<Arc<dyn LocalExtractor>>,
        token_budget: usize,
    ) -> Self {
        self.extractor = extractor;
        self.handoff_token_budget = token_budget;
        self
    }

    /// Publishes a safe event summary to currently connected clients.
    ///
    /// A lack of subscribers is normal. Lagging subscribers recover through
    /// their last deterministic event ID.
    pub fn publish(&self, event: &CanonicalEvent) {
        let _ignored = self.events.send(EventSummary::from(event));
    }

    /// Returns the shared storage handle for daemon-owned ingestion tasks.
    #[must_use]
    pub fn storage(&self) -> &Storage {
        &self.storage
    }
}

/// Builds the `/api/v1` router.
pub fn router(state: ApiState) -> Router {
    Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v1/tools", get(tools))
        .route("/api/v1/tools/discover", post(discover_tools))
        .route("/api/v1/native-sessions", get(native_sessions))
        .route("/api/v1/native-sessions/{id}", get(native_session))
        .route("/api/v1/events", get(events))
        .route("/api/v1/events/{id}", get(event_detail))
        .route("/api/v1/events/stream", get(event_stream))
        .route(
            "/api/v1/global-sessions",
            get(global_sessions).post(create_global_session),
        )
        .route("/api/v1/global-sessions/{id}", get(global_session))
        .route("/api/v1/global-sessions/{id}/members", post(link_member))
        .route(
            "/api/v1/global-sessions/{id}/members/{native_id}",
            axum::routing::delete(unlink_member),
        )
        .route("/api/v1/correlations/accept", post(accept_correlation))
        .route("/api/v1/correlations/reject", post(reject_correlation))
        .route(
            "/api/v1/handoffs/{global_id}",
            get(get_handoff).post(refresh_handoff),
        )
        .with_state(state)
}

/// Health response.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct HealthResponse {
    /// Stable service status.
    pub status: &'static str,
    /// API contract version.
    pub api_version: &'static str,
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        api_version: "v1",
    })
}

/// Safe tool discovery status.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ToolStatus {
    /// Tool family.
    pub family: String,
    /// Tool surface.
    pub surface: String,
    /// Whether a readable installation was found.
    pub detected: bool,
    /// Number of readable native sources.
    pub source_count: usize,
    /// Non-secret diagnostic code.
    pub diagnostic_code: Option<String>,
}

async fn tools(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<Vec<ToolStatus>>, ApiError> {
    authorize(&state, &headers)?;
    Ok(Json(state.tools.as_ref().clone()))
}

async fn discover_tools(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<Vec<ToolStatus>>), ApiError> {
    authorize(&state, &headers)?;
    Ok((StatusCode::ACCEPTED, Json(state.tools.as_ref().clone())))
}

/// Opaque stable page response.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Page<T> {
    /// Safe projected items.
    pub items: Vec<T>,
    /// Cursor for the next page.
    pub next_cursor: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct SessionQuery {
    cursor: Option<String>,
    limit: Option<usize>,
    tool_family: Option<String>,
}

async fn native_sessions(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Query(query): Query<SessionQuery>,
) -> Result<Json<Page<SessionSummary>>, ApiError> {
    authorize(&state, &headers)?;
    let limit = page_size(query.limit)?;
    let after = decode_cursor(query.cursor.as_deref(), "session")?;
    let sessions = state.storage.list_native_sessions().await?;
    if let Some(cursor) = &after
        && !sessions.iter().any(|session| session.id == *cursor)
    {
        return Err(ApiError::InvalidCursor);
    }
    Ok(Json(page_sessions(
        sessions,
        after.as_deref(),
        limit,
        query.tool_family.as_deref(),
    )))
}

/// Secret-free native session summary.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SessionSummary {
    /// Native identity.
    pub id: String,
    /// Tool family.
    pub tool_family: String,
    /// Tool surface.
    pub surface: String,
    /// Start timestamp if observed.
    pub started_at: Option<String>,
    /// End timestamp if observed.
    pub ended_at: Option<String>,
}

impl From<StoredNativeSession> for SessionSummary {
    fn from(session: StoredNativeSession) -> Self {
        Self {
            id: session.id,
            tool_family: session.tool_family,
            surface: session.surface,
            started_at: session.started_at,
            ended_at: session.ended_at,
        }
    }
}

fn page_sessions(
    sessions: Vec<StoredNativeSession>,
    after: Option<&str>,
    limit: usize,
    family: Option<&str>,
) -> Page<SessionSummary> {
    let mut matching = sessions
        .into_iter()
        .filter(|session| after.is_none_or(|cursor| session.id.as_str() > cursor))
        .filter(|session| family.is_none_or(|family| session.tool_family == family))
        .take(limit + 1)
        .collect::<Vec<_>>();
    let has_more = matching.len() > limit;
    matching.truncate(limit);
    let next_cursor = has_more
        .then(|| {
            matching
                .last()
                .map(|session| encode_cursor("session", &session.id))
        })
        .flatten();
    Page {
        items: matching.into_iter().map(SessionSummary::from).collect(),
        next_cursor,
    }
}

async fn native_session(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<SessionSummary>, ApiError> {
    authorize(&state, &headers)?;
    state
        .storage
        .get_native_session(&id)
        .await?
        .map(SessionSummary::from)
        .map(Json)
        .ok_or(ApiError::NotFound)
}

#[derive(Debug, Default, Deserialize)]
struct EventQuery {
    cursor: Option<String>,
    limit: Option<usize>,
    native_session_id: Option<String>,
    kind: Option<String>,
}

async fn events(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Query(query): Query<EventQuery>,
) -> Result<Json<Page<EventSummary>>, ApiError> {
    authorize(&state, &headers)?;
    let limit = page_size(query.limit)?;
    let after = decode_cursor(query.cursor.as_deref(), "event")?;
    let kind = query.kind.as_deref().map(parse_kind).transpose()?;
    let events = sorted_events(state.storage.list_canonical_events().await?)?;
    if let Some(cursor) = &after
        && !events.iter().any(|event| event.event_id.as_str() == cursor)
    {
        return Err(ApiError::InvalidCursor);
    }
    Ok(Json(page_events(
        events,
        after.as_deref(),
        limit,
        query.native_session_id.as_deref(),
        kind,
    )))
}

async fn event_detail(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<CanonicalEvent>, ApiError> {
    authorize(&state, &headers)?;
    state
        .storage
        .get_canonical_event(&id)
        .await?
        .map(Json)
        .ok_or(ApiError::NotFound)
}

/// Global session projection with reference-only membership and audit data.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct GlobalSessionDetail {
    /// Global session metadata.
    pub session: GlobalSessionSummary,
    /// Native-session references.
    pub members: Vec<StoredMemberSummary>,
    /// Immutable manual-decision audit.
    pub audit: Vec<AuditSummary>,
}

/// Safe global session summary.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct GlobalSessionSummary {
    /// Global session ID.
    pub id: String,
    /// Human objective.
    pub objective: String,
    /// Creation timestamp.
    pub created_at: String,
    /// Update timestamp.
    pub updated_at: String,
}

impl From<StoredGlobalSession> for GlobalSessionSummary {
    fn from(session: StoredGlobalSession) -> Self {
        Self {
            id: session.id,
            objective: session.objective,
            created_at: session.created_at,
            updated_at: session.updated_at,
        }
    }
}

/// Membership response that cannot contain transcript data.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct StoredMemberSummary {
    /// Native session reference.
    pub native_session_id: String,
    /// Correlation confidence.
    pub confidence: f64,
    /// Scoring version.
    pub correlation_version: String,
    /// Manual state.
    pub manual_state: Option<String>,
}

impl From<StoredSessionMember> for StoredMemberSummary {
    fn from(member: StoredSessionMember) -> Self {
        Self {
            native_session_id: member.native_session_id,
            confidence: member.confidence,
            correlation_version: member.correlation_version,
            manual_state: member.manual_state,
        }
    }
}

/// Safe audit response.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AuditSummary {
    /// Audit identity.
    pub id: i64,
    /// Native session reference.
    pub native_session_id: String,
    /// Decision.
    pub action: String,
    /// Local actor class.
    pub actor: String,
    /// Optional rationale.
    pub reason: Option<String>,
    /// Decision timestamp.
    pub created_at: String,
}

impl From<MembershipAuditEntry> for AuditSummary {
    fn from(entry: MembershipAuditEntry) -> Self {
        Self {
            id: entry.id,
            native_session_id: entry.native_session_id,
            action: entry.action,
            actor: entry.actor,
            reason: entry.reason,
            created_at: entry.created_at,
        }
    }
}

#[derive(Debug, Deserialize)]
struct CreateGlobalSessionRequest {
    objective: String,
}

#[derive(Debug, Deserialize)]
struct MembershipRequest {
    native_session_id: String,
    confidence: Option<f64>,
    correlation_version: Option<String>,
    reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CorrelationDecisionRequest {
    global_session_id: String,
    native_session_id: String,
    reason: Option<String>,
}

async fn global_sessions(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<Vec<GlobalSessionSummary>>, ApiError> {
    authorize(&state, &headers)?;
    Ok(Json(
        state
            .storage
            .list_global_sessions()
            .await?
            .into_iter()
            .map(GlobalSessionSummary::from)
            .collect(),
    ))
}

async fn create_global_session(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(request): Json<CreateGlobalSessionRequest>,
) -> Result<(StatusCode, Json<GlobalSessionSummary>), ApiError> {
    authorize(&state, &headers)?;
    if request.objective.trim().is_empty() {
        return Err(ApiError::InvalidInput);
    }
    let now = now_rfc3339();
    let session = StoredGlobalSession {
        id: random_global_session_id()?,
        objective: request.objective.trim().to_owned(),
        created_at: now.clone(),
        updated_at: now,
    };
    state.storage.create_global_session(&session).await?;
    Ok((
        StatusCode::CREATED,
        Json(GlobalSessionSummary::from(session)),
    ))
}

async fn global_session(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<GlobalSessionDetail>, ApiError> {
    authorize(&state, &headers)?;
    let session = state
        .storage
        .list_global_sessions()
        .await?
        .into_iter()
        .find(|session| session.id == id)
        .ok_or(ApiError::NotFound)?;
    let members = state
        .storage
        .list_session_members(&id)
        .await?
        .into_iter()
        .map(StoredMemberSummary::from)
        .collect();
    let audit = state
        .storage
        .membership_audit(&id)
        .await?
        .into_iter()
        .map(AuditSummary::from)
        .collect();
    Ok(Json(GlobalSessionDetail {
        session: GlobalSessionSummary::from(session),
        members,
        audit,
    }))
}

async fn link_member(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<MembershipRequest>,
) -> Result<StatusCode, ApiError> {
    authorize(&state, &headers)?;
    state
        .storage
        .link_session(
            &MembershipDecision {
                global_session_id: &id,
                native_session_id: &request.native_session_id,
                actor: "authenticated-local-user",
                reason: request.reason.as_deref(),
                created_at: &now_rfc3339(),
            },
            request.confidence.unwrap_or(1.0),
            request
                .correlation_version
                .as_deref()
                .unwrap_or("manual-v1"),
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn unlink_member(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path((id, native_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    authorize(&state, &headers)?;
    state
        .storage
        .unlink_session(&MembershipDecision {
            global_session_id: &id,
            native_session_id: &native_id,
            actor: "authenticated-local-user",
            reason: None,
            created_at: &now_rfc3339(),
        })
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn accept_correlation(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(request): Json<CorrelationDecisionRequest>,
) -> Result<StatusCode, ApiError> {
    authorize(&state, &headers)?;
    state
        .storage
        .link_session(
            &MembershipDecision {
                global_session_id: &request.global_session_id,
                native_session_id: &request.native_session_id,
                actor: "authenticated-local-user",
                reason: request.reason.as_deref(),
                created_at: &now_rfc3339(),
            },
            1.0,
            "manual-v1",
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn reject_correlation(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(request): Json<CorrelationDecisionRequest>,
) -> Result<StatusCode, ApiError> {
    authorize(&state, &headers)?;
    state
        .storage
        .reject_membership(&MembershipDecision {
            global_session_id: &request.global_session_id,
            native_session_id: &request.native_session_id,
            actor: "authenticated-local-user",
            reason: request.reason.as_deref(),
            created_at: &now_rfc3339(),
        })
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

fn random_global_session_id() -> Result<String, ApiError> {
    let mut random = [0_u8; 16];
    getrandom::fill(&mut random).map_err(|_| ApiError::Internal)?;
    let mut id = String::from("gs_");
    for byte in random {
        use std::fmt::Write as _;
        write!(&mut id, "{byte:02x}").map_err(|_| ApiError::Internal)?;
    }
    Ok(id)
}

fn now_rfc3339() -> String {
    chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now()).to_rfc3339()
}

async fn get_handoff(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(global_id): Path<String>,
) -> Result<Json<Handoff>, ApiError> {
    authorize(&state, &headers)?;
    let stored = state
        .storage
        .latest_handoff(&global_id)
        .await?
        .ok_or(ApiError::NotFound)?;
    let handoff = serde_json::from_str(&stored.handoff_json).map_err(|_| ApiError::Internal)?;
    Ok(Json(handoff))
}

async fn refresh_handoff(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Path(global_id): Path<String>,
) -> Result<Json<Handoff>, ApiError> {
    authorize(&state, &headers)?;
    let session = state
        .storage
        .list_global_sessions()
        .await?
        .into_iter()
        .find(|session| session.id == global_id)
        .ok_or(ApiError::NotFound)?;
    let member_ids = state
        .storage
        .list_session_members(&global_id)
        .await?
        .into_iter()
        .map(|member| member.native_session_id)
        .collect::<std::collections::BTreeSet<_>>();
    let events = state
        .storage
        .list_canonical_events()
        .await?
        .into_iter()
        .filter(|event| member_ids.contains(&event.native_session_id))
        .collect::<Vec<_>>();
    let records = state
        .storage
        .list_session_records(&global_id)
        .await?
        .into_iter()
        .map(|record| RecordedFact {
            provenance_id: record.id,
            kind: record.record_type,
            content: record.content,
        })
        .collect::<Vec<_>>();
    let handoff = generate(
        HandoffInput {
            global_session_id: &global_id,
            objective: &session.objective,
            events: &events,
            records: &records,
            repository: RepositoryState {
                branch: events
                    .iter()
                    .rev()
                    .find_map(|event| event.workspace.as_ref()?.branch.clone()),
                head: events
                    .iter()
                    .rev()
                    .find_map(|event| event.workspace.as_ref()?.head.clone()),
                dirty: events
                    .iter()
                    .rev()
                    .find_map(|event| {
                        event
                            .payload
                            .get("dirty")
                            .and_then(serde_json::Value::as_bool)
                    })
                    .unwrap_or(false),
            },
            token_budget: state.handoff_token_budget,
            model_timeout: std::time::Duration::from_secs(15),
        },
        state.extractor.as_deref(),
    )
    .await
    .map_err(|_| ApiError::InvalidInput)?;
    let snapshot_id = snapshot_id(&handoff).map_err(|_| ApiError::Internal)?;
    let handoff_json = serde_json::to_string(&handoff).map_err(|_| ApiError::Internal)?;
    state
        .storage
        .store_handoff(
            &StoredHandoff {
                id: format!("handoff_{}", snapshot_id.trim_start_matches("snapshot_")),
                global_session_id: global_id,
                snapshot_id,
                schema_version: HANDOFF_SCHEMA_VERSION.to_owned(),
                handoff_json: handoff_json.clone(),
                created_at: now_rfc3339(),
            },
            &handoff_json,
        )
        .await?;
    Ok(Json(handoff))
}

/// Event projection that intentionally excludes native payloads.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct EventSummary {
    /// Deterministic event ID.
    pub event_id: String,
    /// Native session ID.
    pub native_session_id: String,
    /// Source-local sequence.
    pub sequence: u64,
    /// Original timestamp.
    pub timestamp: String,
    /// Canonical kind.
    pub kind: String,
    /// CWD when observed.
    pub cwd: Option<String>,
    /// Branch when observed.
    pub branch: Option<String>,
}

impl From<&CanonicalEvent> for EventSummary {
    fn from(event: &CanonicalEvent) -> Self {
        Self {
            event_id: event.event_id.as_str().to_owned(),
            native_session_id: event.native_session_id.clone(),
            sequence: event.sequence,
            timestamp: event.timestamp.as_str().to_owned(),
            kind: kind_name(event.kind).to_owned(),
            cwd: event
                .workspace
                .as_ref()
                .and_then(|workspace| workspace.cwd.clone()),
            branch: event
                .workspace
                .as_ref()
                .and_then(|workspace| workspace.branch.clone()),
        }
    }
}

fn page_events(
    events: Vec<CanonicalEvent>,
    after: Option<&str>,
    limit: usize,
    session_id: Option<&str>,
    kind: Option<EventKind>,
) -> Page<EventSummary> {
    let start = after
        .and_then(|id| {
            events
                .iter()
                .position(|event| event.event_id.as_str() == id)
        })
        .map_or(0, |position| position + 1);
    let mut matching = events
        .into_iter()
        .skip(start)
        .filter(|event| session_id.is_none_or(|id| event.native_session_id == id))
        .filter(|event| kind.is_none_or(|kind| event.kind == kind))
        .take(limit + 1)
        .collect::<Vec<_>>();
    let has_more = matching.len() > limit;
    matching.truncate(limit);
    let next_cursor = has_more
        .then(|| {
            matching
                .last()
                .map(|event| encode_cursor("event", event.event_id.as_str()))
        })
        .flatten();
    Page {
        items: matching.iter().map(EventSummary::from).collect(),
        next_cursor,
    }
}

fn sorted_events(mut events: Vec<CanonicalEvent>) -> Result<Vec<CanonicalEvent>, ApiError> {
    let mut keys = BTreeMap::new();
    for event in &events {
        let timestamp = chrono::DateTime::parse_from_rfc3339(event.timestamp.as_str())
            .map_err(|_| ApiError::Internal)?;
        let nanos = timestamp.timestamp_nanos_opt().ok_or(ApiError::Internal)?;
        keys.insert(
            event.event_id.as_str().to_owned(),
            (
                nanos,
                event.sequence,
                event.provenance.source_generation.clone(),
                event.provenance.source_offset,
                event.provenance.ingestion_sequence,
                event.event_id.as_str().to_owned(),
            ),
        );
    }
    events.sort_by_key(|event| keys.get(event.event_id.as_str()).cloned());
    Ok(events)
}

async fn event_stream(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Sse<impl futures_core::Stream<Item = Result<Event, Infallible>>>, ApiError> {
    authorize(&state, &headers)?;
    let last_id = headers
        .get("last-event-id")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let history = sorted_events(state.storage.list_canonical_events().await?)?;
    if let Some(id) = &last_id
        && !history.iter().any(|event| event.event_id.as_str() == id)
    {
        return Err(ApiError::InvalidCursor);
    }
    let mut receiver = state.events.subscribe();
    let stream = async_stream::stream! {
        for event in page_events(history, last_id.as_deref(), usize::MAX - 1, None, None).items {
            yield Ok(sse_event(&event));
        }
        loop {
            match receiver.recv().await {
                Ok(event) => yield Ok(sse_event(&event)),
                Err(broadcast::error::RecvError::Lagged(_)) => break,
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    };
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

fn sse_event(summary: &EventSummary) -> Event {
    Event::default()
        .id(summary.event_id.clone())
        .event("canonical_event")
        .json_data(summary)
        .unwrap_or_else(|_| Event::default().event("serialization_error"))
}

fn authorize(state: &ApiState, headers: &HeaderMap) -> Result<(), ApiError> {
    let supplied = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .ok_or(ApiError::Unauthorized)?;
    constant_time_equal(supplied.as_bytes(), state.auth_token.as_bytes())
        .then_some(())
        .ok_or(ApiError::Unauthorized)
}

fn constant_time_equal(left: &[u8], right: &[u8]) -> bool {
    let mut difference = left.len() ^ right.len();
    for index in 0..left.len().max(right.len()) {
        difference |= usize::from(
            left.get(index).copied().unwrap_or_default()
                ^ right.get(index).copied().unwrap_or_default(),
        );
    }
    difference == 0
}

fn page_size(limit: Option<usize>) -> Result<usize, ApiError> {
    match limit.unwrap_or(DEFAULT_PAGE_SIZE) {
        0 => Err(ApiError::InvalidLimit),
        limit if limit > MAX_PAGE_SIZE => Err(ApiError::InvalidLimit),
        limit => Ok(limit),
    }
}

fn encode_cursor(scope: &str, value: &str) -> String {
    format!("v1:{scope}:{value}")
}

fn decode_cursor(cursor: Option<&str>, scope: &str) -> Result<Option<String>, ApiError> {
    let Some(cursor) = cursor else {
        return Ok(None);
    };
    let prefix = format!("v1:{scope}:");
    cursor
        .strip_prefix(&prefix)
        .filter(|value| !value.is_empty())
        .map(|value| Some(value.to_owned()))
        .ok_or(ApiError::InvalidCursor)
}

fn parse_kind(kind: &str) -> Result<EventKind, ApiError> {
    serde_json::from_value(serde_json::Value::String(kind.to_owned()))
        .map_err(|_| ApiError::InvalidFilter)
}

fn kind_name(kind: EventKind) -> &'static str {
    match kind {
        EventKind::UserMessage => "user_message",
        EventKind::AssistantMessage => "assistant_message",
        EventKind::SystemMessage => "system_message",
        EventKind::ToolCall => "tool_call",
        EventKind::ToolResult => "tool_result",
        EventKind::FileRead => "file_read",
        EventKind::FileWrite => "file_write",
        EventKind::Patch => "patch",
        EventKind::ShellCommand => "shell_command",
        EventKind::CommandResult => "command_result",
        EventKind::GitState => "git_state",
        EventKind::Plan => "plan",
        EventKind::Task => "task",
        EventKind::Decision => "decision",
        EventKind::Checkpoint => "checkpoint",
        EventKind::Compaction => "compaction",
        EventKind::Error => "error",
        EventKind::SubagentSpawn => "subagent_spawn",
        EventKind::SessionStart => "session_start",
        EventKind::SessionEnd => "session_end",
        EventKind::Unknown => "unknown",
    }
}

/// Stable API error envelope.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ErrorEnvelope {
    /// Machine-readable code.
    pub code: &'static str,
    /// Safe message with no internal source content.
    pub message: &'static str,
}

#[derive(Debug)]
enum ApiError {
    Unauthorized,
    InvalidCursor,
    InvalidLimit,
    InvalidFilter,
    NotFound,
    InvalidInput,
    Conflict,
    Internal,
}

impl From<StorageError> for ApiError {
    fn from(error: StorageError) -> Self {
        match error {
            StorageError::InvalidInput {
                field: "membership",
                ..
            }
            | StorageError::ImmutableConflict { .. } => Self::Conflict,
            StorageError::InvalidInput { .. } => Self::InvalidInput,
            _ => Self::Internal,
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code, message) = match self {
            Self::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "a valid local bearer token is required",
            ),
            Self::InvalidCursor => (
                StatusCode::BAD_REQUEST,
                "invalid_cursor",
                "the pagination or resume cursor is invalid",
            ),
            Self::InvalidLimit => (
                StatusCode::BAD_REQUEST,
                "invalid_limit",
                "limit must be between 1 and 200",
            ),
            Self::InvalidFilter => (
                StatusCode::BAD_REQUEST,
                "invalid_filter",
                "the requested filter is unsupported",
            ),
            Self::NotFound => (StatusCode::NOT_FOUND, "not_found", "resource was not found"),
            Self::InvalidInput => (
                StatusCode::BAD_REQUEST,
                "invalid_input",
                "the request body is invalid",
            ),
            Self::Conflict => (
                StatusCode::CONFLICT,
                "conflict",
                "the requested state conflicts with a prior decision",
            ),
            Self::Internal => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                "the request could not be completed",
            ),
        };
        (status, Json(ErrorEnvelope { code, message })).into_response()
    }
}

pub use sessionmesh_core::bootstrap_stage;

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use sessionmesh_core::event::{
        EventDraft, EventProvenance, EventTimestamp, TimestampPrecision, ToolIdentity,
    };
    use sessionmesh_storage::EventRepository;
    use tempfile::TempDir;
    use tower::ServiceExt;

    use super::*;

    async fn state() -> (TempDir, ApiState) {
        let directory = tempfile::tempdir().expect("temporary directory should exist");
        let storage = Storage::open(
            directory.path().join("sessionmesh.db"),
            directory.path().join("blobs"),
        )
        .await
        .expect("storage should open");
        let state = ApiState::new(
            storage,
            Arc::<str>::from("local-test-token"),
            vec![ToolStatus {
                family: "codex".to_owned(),
                surface: "cli".to_owned(),
                detected: true,
                source_count: 1,
                diagnostic_code: None,
            }],
        );
        (directory, state)
    }

    fn event(session: &str, sequence: u64, timestamp: &str) -> CanonicalEvent {
        CanonicalEvent::from_draft(EventDraft {
            tool: ToolIdentity {
                family: "codex".to_owned(),
                surface: "cli".to_owned(),
                profile: "default".to_owned(),
            },
            native_session_id: session.to_owned(),
            sequence,
            timestamp: EventTimestamp::parse(timestamp).expect("timestamp should parse"),
            timestamp_precision: TimestampPrecision::Second,
            kind: if sequence == 0 {
                EventKind::SessionStart
            } else {
                EventKind::AssistantMessage
            },
            workspace: None,
            payload: BTreeMap::from([
                ("text".to_owned(), "safe summary must exclude this".into()),
                ("api_key".to_owned(), "synthetic-secret-value".into()),
            ]),
            provenance: EventProvenance {
                source_path: "/sources/codex/rollout.jsonl".to_owned(),
                original_path: Some("~/.codex/rollout.jsonl".to_owned()),
                source_offset: sequence,
                source_generation: format!("generation-{session}"),
                adapter_version: "0.1.0".to_owned(),
                ingestion_sequence: sequence,
                ordering_confidence: Some(1.0),
            },
        })
        .expect("event should build")
    }

    fn request(uri: &str) -> Request<Body> {
        Request::builder()
            .uri(uri)
            .header("authorization", "Bearer local-test-token")
            .body(Body::empty())
            .expect("request should build")
    }

    fn json_request(method: &str, uri: &str, body: &serde_json::Value) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .header("authorization", "Bearer local-test-token")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .expect("request should build")
    }

    async fn json(response: Response) -> serde_json::Value {
        let body = response
            .into_body()
            .collect()
            .await
            .expect("body should collect")
            .to_bytes();
        serde_json::from_slice(&body).expect("body should be JSON")
    }

    #[tokio::test]
    async fn health_is_public_but_data_requires_authentication() {
        let (_directory, state) = state().await;
        let app = router(state);
        let health = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let unauthorized = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/tools")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(health.status(), StatusCode::OK);
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(json(unauthorized).await["code"], "unauthorized");
    }

    #[tokio::test]
    async fn session_pages_are_stable_filtered_and_cursor_validated() {
        let (_directory, state) = state().await;
        state
            .storage
            .store_event(&event("session-b", 0, "2026-07-20T14:30:00Z"))
            .await
            .unwrap();
        state
            .storage
            .store_event(&event("session-a", 0, "2026-07-20T14:31:00Z"))
            .await
            .unwrap();
        let app = router(state);
        let first = app
            .clone()
            .oneshot(request("/api/v1/native-sessions?limit=1&tool_family=codex"))
            .await
            .unwrap();
        let first_json = json(first).await;
        let cursor = first_json["next_cursor"].as_str().unwrap();
        let second = app
            .clone()
            .oneshot(request(&format!(
                "/api/v1/native-sessions?limit=1&cursor={cursor}"
            )))
            .await
            .unwrap();
        let invalid = app
            .oneshot(request("/api/v1/native-sessions?cursor=wrong"))
            .await
            .unwrap();

        assert_eq!(first_json["items"][0]["id"], "session-a");
        assert_eq!(json(second).await["items"][0]["id"], "session-b");
        assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn event_list_excludes_raw_payload_and_orders_equal_timestamps() {
        let (_directory, state) = state().await;
        state
            .storage
            .store_event(&event("session-a", 1, "2026-07-20T14:30:00Z"))
            .await
            .unwrap();
        state
            .storage
            .store_event(&event("session-a", 0, "2026-07-20T14:30:00Z"))
            .await
            .unwrap();
        let response = router(state)
            .oneshot(request("/api/v1/events?native_session_id=session-a"))
            .await
            .unwrap();
        let body = json(response).await;
        let encoded = body.to_string();

        assert_eq!(body["items"][0]["sequence"], 0);
        assert_eq!(body["items"][1]["sequence"], 1);
        assert!(!encoded.contains("synthetic-secret-value"));
        assert!(!encoded.contains("api_key"));
        assert!(body["items"][0].get("payload").is_none());
    }

    #[tokio::test]
    async fn event_detail_requires_auth_and_returns_only_the_selected_payload() {
        let (_directory, state) = state().await;
        let stored = event("session-a", 1, "2026-07-20T14:30:00Z");
        state.storage.store_event(&stored).await.unwrap();
        let path = format!("/api/v1/events/{}", stored.event_id.as_str());
        let app = router(state);
        let unauthorized = app
            .clone()
            .oneshot(Request::builder().uri(&path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let detail = app.oneshot(request(&path)).await.unwrap();
        let body = json(detail).await;

        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(body["event_id"], stored.event_id.as_str());
        assert_eq!(body["payload"]["api_key"], "synthetic-secret-value");
    }

    #[tokio::test]
    async fn empty_results_and_invalid_filters_have_stable_envelopes() {
        let (_directory, state) = state().await;
        let app = router(state);
        let empty = app
            .clone()
            .oneshot(request("/api/v1/events"))
            .await
            .unwrap();
        let invalid = app
            .oneshot(request("/api/v1/events?kind=not-a-kind"))
            .await
            .unwrap();

        assert!(json(empty).await["items"].as_array().unwrap().is_empty());
        assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
        assert_eq!(json(invalid).await["code"], "invalid_filter");
    }

    #[tokio::test]
    async fn corrupt_stored_event_returns_safe_internal_error() {
        let (_directory, state) = state().await;
        state
            .storage
            .store_event(&event("session-a", 0, "2026-07-20T14:30:00Z"))
            .await
            .unwrap();
        sqlx::query("UPDATE native_events SET canonical_json = 'secret parser failure'")
            .execute(state.storage.pool())
            .await
            .unwrap();
        let response = router(state)
            .oneshot(request("/api/v1/events"))
            .await
            .unwrap();
        let body = json(response).await;

        assert_eq!(body["code"], "internal_error");
        assert!(!body.to_string().contains("secret parser failure"));
    }

    #[tokio::test]
    async fn sse_rejects_unknown_resume_id_and_bounds_slow_clients() {
        let (_directory, state) = state().await;
        let stored = event("session-a", 0, "2026-07-20T14:30:00Z");
        state.storage.store_event(&stored).await.unwrap();
        let app = router(state.clone());
        let resumed = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/events/stream")
                    .header("authorization", "Bearer local-test-token")
                    .header("last-event-id", stored.event_id.as_str())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let invalid = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/events/stream")
                    .header("authorization", "Bearer local-test-token")
                    .header("last-event-id", "sha256:missing")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resumed.status(), StatusCode::OK);
        assert_eq!(
            resumed.headers().get("content-type").unwrap(),
            "text/event-stream"
        );
        assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);

        let mut slow = state.events.subscribe();
        for sequence in 0..140 {
            state.publish(&event("session-a", sequence, "2026-07-20T14:30:00Z"));
        }
        assert!(matches!(
            slow.recv().await,
            Err(broadcast::error::RecvError::Lagged(_))
        ));
    }

    #[tokio::test]
    async fn global_membership_mutations_are_authenticated_reversible_and_audited() {
        let (_directory, state) = state().await;
        state
            .storage
            .store_event(&event("native-a", 0, "2026-07-20T14:30:00Z"))
            .await
            .unwrap();
        let app = router(state);
        let created = app
            .clone()
            .oneshot(json_request(
                "POST",
                "/api/v1/global-sessions",
                &serde_json::json!({"objective": "Build correlation"}),
            ))
            .await
            .unwrap();
        assert_eq!(created.status(), StatusCode::CREATED);
        let global_id = json(created).await["id"].as_str().unwrap().to_owned();
        let link_path = format!("/api/v1/global-sessions/{global_id}/members");
        let linked = app
            .clone()
            .oneshot(json_request(
                "POST",
                &link_path,
                &serde_json::json!({"native_session_id": "native-a"}),
            ))
            .await
            .unwrap();
        assert_eq!(linked.status(), StatusCode::NO_CONTENT);
        let rejected = app
            .clone()
            .oneshot(json_request(
                "POST",
                "/api/v1/correlations/reject",
                &serde_json::json!({
                    "global_session_id": global_id,
                    "native_session_id": "native-a",
                    "reason": "different objective"
                }),
            ))
            .await
            .unwrap();
        assert_eq!(rejected.status(), StatusCode::NO_CONTENT);
        let blocked = app
            .clone()
            .oneshot(json_request(
                "POST",
                "/api/v1/correlations/accept",
                &serde_json::json!({
                    "global_session_id": global_id,
                    "native_session_id": "native-a"
                }),
            ))
            .await
            .unwrap();
        assert_eq!(blocked.status(), StatusCode::CONFLICT);
        let detail = app
            .oneshot(request(&format!("/api/v1/global-sessions/{global_id}")))
            .await
            .unwrap();
        let body = json(detail).await;
        assert!(body["members"].as_array().unwrap().is_empty());
        assert_eq!(body["audit"].as_array().unwrap().len(), 2);
        assert!(!body.to_string().contains("payload"));
    }
}
