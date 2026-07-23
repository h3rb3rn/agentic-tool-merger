# Implementation Status

This page is the canonical restart and execution ledger.

## Current state

- **Current milestone:** Milestone 3 multi-tool correlation implemented
- **Active prompt:** None
- **Next prompt:** Decode supported Agy assistant trajectory exports
- **Last completed prompt:** 14 — MVP Hardening
- **Known blockers:** None

The running integration foundation now discovers Claude Code, Continue,
OpenCode, and Agy sessions from a read-only agent home, correlates sessions by
normalized workspace, bounded temporal proximity, and lexical content evidence,
persists uncertain candidates for review, refreshes deterministic handoffs,
and delivers them to Codex, Claude Code, and Continue through MCP and startup
connectors. Native transcript synchronization remains intentionally excluded:
shared knowledge is delivered as provenance-backed context without mutating
tool-owned stores.

The Correlation Review presents a bounded, searchable queue. Candidate scores
are attached to paired thread context cards and can be expanded into workspace,
time, similarity, and shared-keyword evidence instead of being shown beside
opaque native IDs alone.

## Prompt ledger

| Prompt                            | State       | Dependencies | Result                                                 |
| --------------------------------- | ----------- | ------------ | ------------------------------------------------------ |
| 00 Workspace Bootstrap            | `completed` | None         | Rust/React and OCI foundation validated                |
| 01 Architecture Baseline          | `completed` | 00           | Versioned schemas, fixtures, and dependency checks     |
| 02 Configuration                  | `completed` | 01           | Typed, layered, validated configuration                |
| 03 Canonical Model                | `completed` | 01           | Versioned types and deterministic event identity       |
| 04 Storage Foundation             | `completed` | 02, 03       | Transactional WAL, immutable raw, blobs, and cursors   |
| 05 Adapter Contract               | `completed` | 03, 04       | Read-only SDK and negotiated NDJSON protocol           |
| 06 Codex Discovery                | `completed` | 02, 05       | Deterministic homes, rollouts, index, and diagnostics  |
| 07 Codex Parser                   | `completed` | 03, 05, 06   | Pure traceable rollout normalization                   |
| 08 Incremental Ingestion          | `completed` | 04, 06, 07   | Atomic append, recovery, rotation, and ordering        |
| 09 REST API                       | `completed` | 04, 08       | Authenticated REST, pagination, and resumable SSE      |
| 10 Web Timeline                   | `completed` | 09           | Accessible live timeline and explicit raw disclosure   |
| 11 Repository and Global Sessions | `completed` | 04, 08, 09   | Git identity, correlation, references, and audit       |
| 12 Handoff Engine                 | `completed` | 03, 11       | Provenance-backed deterministic compact state          |
| 13 MCP Integration                | `completed` | 11, 12       | Typed read/write tools with strict trust boundaries    |
| 14 MVP Hardening                  | `completed` | 00–13        | Native and OCI journey, threat model, and release gate |

## Session handoff

Prompt 00 established the planned Rust crate boundaries, React/Vite application,
English repository conventions, shared quality commands, Apache-2.0 licensing,
Material for MkDocs, and the Docker/Podman-compatible OCI foundation.

Validation completed:

- Rust 1.88 formatting, Clippy with warnings denied, unit tests, and doc tests
  passed through the pinned Rust container.
- Prettier, ESLint, TypeScript, Vitest, and the Vite production build passed.
- The npm audit reported zero vulnerabilities after updating affected Vite and
  Vitest patch versions.
- `mkdocs build --strict` passed.
- The OCI multi-stage image built successfully.
- Compose configuration resolved the persistent home and read-only Codex mount.
- The runtime user was UID 10001, state survived container replacement, and a
  write attempt against the native-source fixture failed as required.
- Docker was validated directly. Podman is not installed in this environment;
  compatibility is defined by the shared OCI and Compose contract.

No implementation commit exists because the supplied `.git` mount is not an
initialized Git repository.

Prompt 01 added Draft 2020-12 contracts for canonical events, tool profiles,
and handoffs, with compatible and incompatible examples. Automated tests
validate strict schema versions, timestamp formats, identifiers, required
provenance, profile capabilities, and handoff structure. A second test enforces
the documented Rust workspace dependency direction. Fixture sanitation,
compatibility, and review rules are documented with architecture diagrams.

Additional validation completed:

- Seven Node contract tests passed: six schema cases and one dependency-boundary
  case.
- The React component test, TypeScript checks, ESLint, Prettier, and Vite build
  passed.
- The npm audit reported zero vulnerabilities after selecting patched AJV,
  Vite, and Vitest versions.
- `mkdocs build --strict` passed with all architecture and ADR pages in
  navigation.

Prompt 02 implemented pure, typed configuration resolution with
Default → User TOML → Environment → CLI precedence. Every effective value
retains its source layer. Invalid higher-precedence input fails explicitly and
never falls back.

Configuration validation now covers:

- native and container `SESSIONMESH_HOME` behavior;
- local-only bind and model-endpoint defaults;
- explicit network opt-in;
- database and blob paths derived from the final home;
- safe `~`, `$NAME`, and `${NAME}` expansion without shell execution;
- runtime-readable and original host source paths;
- debounce, token-budget, port, model, and correlation ranges;
- strict unknown-field rejection; and
- field- and source-specific diagnostics.

Validation completed:

- Thirteen Rust tests passed with formatting and Clippy warnings denied.
- Eight configuration/canonical/profile/handoff schema cases and the workspace
  dependency-boundary test passed.
- Web lint, typecheck, component test, and production build passed.
- The npm audit reported zero vulnerabilities.
- `mkdocs build --strict` passed.

Prompt 03 introduced the versioned canonical-event Rust model. It preserves
RFC 3339 timestamp text and precision, supports every planned canonical kind,
retains unknown native events, uses lexically ordered payload maps, and rejects
empty identity fields or invalid provenance confidence.

Deterministic SHA-256 identity includes semantic event data and stable native
source coordinates. Runtime paths, adapter version, ingestion order, and
ordering confidence are intentionally excluded so native and container imports
of the same source remain identical. Parsed events recompute and verify their
ID, detecting tampering or stale fixtures.

Validation completed:

- Twenty-three Rust tests passed with formatting and Clippy warnings denied.
- Tests cover round trips, Unicode, payload ordering, semantic differences,
  mutable import metadata, invalid timestamps, unsupported versions, unknown
  kinds, tampered IDs, and published-fixture parity.
- The canonical JSON Schema now requires explicit timestamp precision.
- All schema, dependency-boundary, web, npm-audit, and strict MkDocs gates
  passed.

Prompt 04 added the initial forward-only SQLite migration and repository
contracts for raw objects, canonical events, and ingestion cursors. SQLite uses
WAL, foreign keys, a bounded busy timeout, FTS5, and user-only permissions.
Content-addressed blobs are published through unique temporary files and atomic
rename, verified on write and read, and reused only after integrity validation.

Raw bytes and provenance metadata are immutable under a stable object identity.
Normalized events are idempotent. Batch ingestion commits raw metadata, events,
and cursor progress together, so failures expose neither partial event sets nor
advanced cursors.

Validation completed:

- Eleven storage tests passed, including rollback, duplicate insert,
  provenance conflicts, blob tamper detection, concurrent blob publication,
  cursor atomicity, WAL settings, permissions, concurrent reads, and reopen
  persistence.
- Twenty-three core tests and all workspace doc tests passed.
- Rust formatting and Clippy with warnings denied passed.
- Eleven repository contract tests, the web test/build pipeline, npm audit, and
  strict MkDocs build passed.
- Storage retention, backup, restore, and forward-only migration policy are
  documented.
- Secret hygiene now includes `.env.example`, expanded ignore rules, automated
  policy tests, README configuration variables, and `SECURITY.md`.

Prompt 05 established the stable adapter SDK for discovery, health, full and
incremental scans, capabilities, cooperative cancellation, bounded output, and
partial completion. Native source descriptors expose paths only for reading.
Cursor proposals carry adapter, source, generation, byte offset, and partial
record ownership and are never committed by adapters.

The process-adapter protocol now negotiates version `1.0` before emitting typed
NDJSON messages. Strict decoding preserves a valid prefix for review while
rejecting malformed JSON, incompatible versions, missing negotiation,
unterminated output, unknown fields, duplicate events, foreign cursors, and
diagnostic instruction fields. Diagnostics remain untrusted display
observations.

Validation completed:

- Ten adapter contract tests passed.
- All 44 Rust workspace tests and all doc tests passed with formatting and
  Clippy warnings denied.
- Repository contract tests, web lint, TypeScript, Vitest, production build,
  npm audit, and strict MkDocs build passed.
- Adapter levels, ownership, backpressure, cursor semantics, protocol examples,
  and stream failure behavior are documented.
- The initial public commit is gated until Prompt 14 and the final
  sensitive-data audit are complete.

Next action: execute Prompt 06 by testing Codex installation and rollout
discovery across explicit `$CODEX_HOME`, default native paths, container source
mappings, session indexes, missing paths, and permission failures.

Prompt 06 implemented deterministic, read-only Codex discovery. Explicit
SessionMesh homes take precedence over an isolated `CODEX_HOME` input, which
takes precedence over `~/.codex`. Multiple explicitly configured installations
are supported for native and container source mappings.

Discovery recognizes the optional `session_index.jsonl` and exactly dated
`sessions/YYYY/MM/DD/rollout-*.jsonl` paths. Canonical filesystem identities
deduplicate symlink aliases while original host-facing paths remain intact for
provenance. Results include the CLI surface, source kinds, permission metadata,
readability, and valid or malformed session-index counts.

Missing, stale, unreadable, and partially populated homes remain non-fatal
diagnostics. Malformed index records do not suppress valid rollouts. Discovery
uses read-only opens, performs no native writes, and produces equal results for
repeated scans of unchanged input.

Validation completed:

- Ten isolated Codex discovery tests passed, covering precedence, default and
  custom homes, multiple installations, symlink deduplication, absent and
  malformed indexes, stale paths, permission failures, determinism, and native
  file preservation.
- All 54 Rust workspace tests and doc tests passed with formatting and Clippy
  warnings denied.
- Repository contract tests, web lint, TypeScript, Vitest, production build,
  npm audit, and strict MkDocs build passed.
- Discovery flow, container mapping, recognized layouts, metadata, and
  troubleshooting diagnostics are documented.

Next action: execute Prompt 07 by writing sanitized Codex rollout fixtures and
parser tests for metadata, messages, tool calls and results, Git/CWD,
compaction, malformed records, and unknown event preservation.

Prompt 07 added a pure Codex rollout parser and sanitized real-shape fixtures.
It maps session metadata, user, assistant, and system data, tool calls and
results, lifecycle events, CWD/Git state, compaction, plans, and errors into
canonical events. Unsupported native records are retained as `unknown`.

Each physical JSONL record retains its exact bytes, line number, and byte
offset. Canonical provenance resolves directly to those coordinates. Tool
results record source-local `call_id` pairing, timestamps retain their observed
precision, and sensitive native fields are marked for later controlled
redaction without altering raw evidence.

Malformed, invalid, missing-timestamp, and incomplete records are isolated.
Safe later records continue to normalize, while incomplete final bytes are left
for incremental ingestion.

Validation completed:

- Eight parser tests and ten discovery tests passed.
- All 62 Rust workspace tests and doc tests passed with formatting and Clippy
  warnings denied.
- Sanitized fixtures cover multiline Unicode, assistant output, tool pairing,
  malformed JSON, compaction, unknown types, sensitive markers, timestamp
  precision, stable IDs, and byte-exact raw reconstruction.
- Repository contract tests, web lint, TypeScript, Vitest, production build,
  and strict MkDocs build passed.
- The Codex mapping table, provenance behavior, redaction boundary, and current
  limitations are documented.

Next action: execute Prompt 08 by implementing cursor-driven file ingestion,
partial-line buffering, generation changes, rename handling, debounced watcher
events, and atomic storage commits.

Prompt 08 implemented cursor-driven Codex ingestion. Source generations combine
filesystem identity with bounded first-record content evidence, surviving
append and rename while detecting replacement. Cursors now persist confirmed
byte boundaries, the next native sequence, and incomplete final bytes.

Only complete newline-terminated records advance the confirmed boundary.
Partial bytes survive restart and are verified against the source before
parsing. Raw records, canonical events, and cursor progress commit atomically;
pre-commit crashes replay stable identities and post-commit scans read only new
content.

Truncate, replacement, rename, duplicate notification, and event-storm paths
are deterministic. Retry applies only to classified transient I/O failures with
bounded capped backoff. ADR-009 ordering keys normalize timestamp instants and
retain native sequence, generation, offset, ingestion sequence, and event ID.

Validation completed:

- Seven incremental integration tests cover append, duplicate scans, partial
  lines across restart, crash boundaries, truncate, replacement, rename,
  coalescing, bounded retry, and deterministic ordering.
- All 70 Rust workspace tests and doc tests passed with formatting and Clippy
  warnings denied.
- Repository boundary and security tests, web lint, TypeScript, Vitest,
  production build, and strict MkDocs build passed.
- Cursor semantics, recovery, watcher coalescing, retry classification,
  generation identity, and global ordering are documented.

Next action: execute Prompt 09 by implementing the versioned Axum health,
tools, native-session, and event read APIs with validated pagination,
filtering, stable ordering, and safe errors.

Prompt 09 implemented the versioned Axum API and a production daemon entry
point. Health is public; tool discovery state, native sessions, events, and
streaming require a persistent randomly generated bearer token stored with
user-only permissions. The daemon retains the secure loopback default, while
container deployment requires an explicit network opt-in and publishes only to
the host loopback interface.

List endpoints return stable envelopes, opaque validated cursors, deterministic
ordering, and payload-free event summaries. Errors never expose parser input or
storage details. Server-sent events use canonical event IDs, replay from
storage after `Last-Event-ID`, deduplicate naturally, and enforce bounded
per-client buffering.

Validation completed:

- Six API contract and storage-backed tests cover authentication, stable
  pagination, invalid cursors and filters, empty results, secret exclusion,
  corrupt stored data, SSE recovery, and lagging clients.
- The daemon token persistence and `0600` permission test passed.
- All 77 Rust workspace tests and doc tests passed with formatting and Clippy
  warnings denied.
- Web checks, npm audit, strict MkDocs, and Compose validation passed.
- REST/SSE contracts, authentication, recovery, privacy, container binding,
  and backpressure are documented.

Prompt 10 added a responsive and keyboard-accessible React timeline. Typed
TanStack Query clients load payload-free projections, authenticated SSE
reconciles live events idempotently, tool calls and results remain paired, and
raw or canonical details require an explicit sensitive-data disclosure.

Prompt 11 added read-only repository/worktree identity, normalized remotes,
strict project markers, deterministic correlation evidence, reference-only
global sessions, reversible membership, persistent rejection, and an audit
trail. The web UI can create, link, unlink, and reject relationships.

Prompt 12 added deterministic handoffs with field-level provenance, secret
redaction, progressive token budgeting, immutable snapshots, and an optional
timeout-bounded OpenAI-compatible local extractor. Deterministic facts remain
authoritative when model output is absent, malformed, late, or conflicting.

Prompt 13 added a 2025-06-18 MCP stdio server. Current-context and handoff
resources, bounded search, and idempotent decision/task observations use strict
schemas. Writes require process-level authorization and remain untrusted
observations rather than instructions.

Prompt 14 connected automatic Codex discovery and incremental polling to the
daemon, restored raw-to-canonical foreign-key traceability, and added an
end-to-end discovery → ingestion → rescan → global session → handoff → MCP
test. The threat model covers HTTP, MCP, raw disclosure, paths, adapters, model
endpoints, native stores, and ingestion loops.

Final validation completed:

- Rust formatting, Clippy with warnings denied, 96 tests, and all doc tests
  passed with Rust 1.88.
- Repository schema, dependency, and secret-hygiene contracts passed.
- ESLint, TypeScript, Vitest, Vite production build, Prettier, and npm audit
  passed.
- `mkdocs build --strict` and `docker compose config` passed.
- The non-root OCI image served health, API, and browser assets; automatically
  imported a read-only Codex mount; retained state across restart; and kept the
  source hash unchanged.
- The measured small-fixture container used approximately 19 MiB RSS, a
  262,144-byte SQLite database, 1,279 blob bytes, and a 1.024 ms local session
  list request. See the performance baseline for scope and limitations.

Known limitations:

- The daemon currently polls Codex discovery rather than using native watcher
  notifications.
- Large-store API sorting still loads canonical event projections before
  pagination.
- OpenCode imports text parts from its allowlisted SQLite session tables. Agy
  currently imports stable prompt history; protobuf assistant trajectories
  await a supported schema or export surface.
- Podman was not installed on the validation host. The image and Compose model
  remain OCI-compatible, while Docker is the exercised runtime.

Release readiness: the local-first MVP satisfies Prompts 00–14. The first
public commit may be staged after the final sensitive-data audit. Nothing may
be pushed automatically.

Next roadmap decision: add supported Agy trajectory decoding or optimize the
Codex watcher and large-store query path using representative sanitized
fixtures.
