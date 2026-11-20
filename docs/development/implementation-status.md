# Implementation Status

This page is the canonical restart and execution ledger.

## Current state

- **Current milestone:** Milestone 3 multi-tool correlation implemented
- **Active prompt:** None
- **Next prompt:** None; MVP prompts 00–14 are complete
- **Last completed prompt:** 14 — MVP Hardening
- **Known blockers:** User home and source Git metadata are read-only in the current coding environment; intermittent initial-import writer contention remains an open operational issue

The running integration foundation now discovers Claude Code, Continue,
OpenCode, and Agy sessions from a read-only agent home, correlates sessions by
normalized workspace, bounded temporal proximity, and lexical content evidence,
persists uncertain candidates for review, refreshes deterministic handoffs,
and provides MCP connectors for Codex, Claude Code, Agy, OpenCode v2, Grok
Builder, and optional Continue. Live delivery requires the service described
in the maintenance audit below. Native transcript synchronization remains intentionally excluded:
shared knowledge is delivered as provenance-backed context without mutating
tool-owned stores.

The Correlation Review presents a bounded, searchable queue. Candidate scores
are attached to paired thread context cards and can be expanded into workspace,
time, similarity, and shared-keyword evidence instead of being shown beside
opaque native IDs alone.

The user documentation now includes a beginner-oriented path from service and
connector setup through connection verification, Web UI authentication, native
resume, and cross-tool continuation after a rate limit.

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

- The daemon currently polls discovery rather than using native watcher
  notifications; unchanged files are skipped by size and modification time.
- Historical limitation (resolved by resource maintenance below): API/SSE
  previously loaded payload-bearing events before pagination.
- Reconciliation, Claude Code, Agy and OpenCode ingestion are incremental;
  Continue still processes one changed JSON snapshot at a time.
- OpenCode imports text parts from its allowlisted SQLite session tables. Agy
  currently imports stable prompt history; protobuf assistant trajectories
  await a supported schema or export surface.
- Podman was not installed on the validation host. The image and Compose model
  remain OCI-compatible, while Docker is the exercised runtime.

Post-MVP performance work (2026-10-09, ADR-010): a production-scale instance
(236,000 events) consumed about 100 % CPU and 14.6 GiB of memory because every
interval reprocessed the whole history. Reconciliation, handoff refresh, Claude
Code ingestion, OpenCode ingestion, and the scan loop are now proportional to
changed data. Verification: Clippy with warnings denied and the full Rust
workspace test suite pass; new tests cover the watermark, idle and per-session
reconciliation, Claude append/partial-line/replacement, OpenCode session-scoped
replay, and scan memory. Real-world resource figures after rollout are not yet
measured.

Release readiness: the local-first MVP satisfies Prompts 00–14. The first
public commit may be staged after the final sensitive-data audit. Nothing may
be pushed automatically.

Next roadmap decision: add supported Agy trajectory decoding or optimize the
Codex watcher and large-store query path using representative sanitized
fixtures.

## MCP connector maintenance — 2026-11-19

The user requested a project review focused on MCP integration for Claude Code,
Codex, Agy, OpenCode, and Grok Builder. The compact SessionMesh tools were not
exposed in this session, so a handoff could not be retrieved or a durable MCP
observation recorded. Repository and client facts were verified locally.

The previous `Next prompt` named an Agy roadmap task without a corresponding
implementation prompt. It now correctly states that no ordered MVP prompt is
pending. Supported Agy assistant trajectory decoding remains a roadmap task.

Verified operational blockers and evidence:

- Codex, Claude, and Agy already reference the bridge. OpenCode v2.2.7 lacks an
  entry; Grok has no native entry but supports compatibility discovery.
- The bridge's default Docker container is absent, as are SessionMesh-named
  volumes and the conventional native state home. A direct MCP check failed
  with `No such container` and transport closure. Existing state may be stored
  elsewhere; no empty replacement database or service was created.
- `/home/philipp` is read-only. Client installation was previewed without
  changing user configuration.
- `.git` is read-only: creating `fix/mcp-client-integrations` failed while
  locking its ref. Work remains uncommitted on the existing non-main branch;
  no staging, commits, or pushes were attempted.

Implemented selective installation for the five requested clients, optional
Continue, private original backups, preservation of startup instructions and
unrelated Claude hooks, OpenCode v2 CLI preflight, configurable container/native
bridges, explicit write opt-in, and a bounded protocol/handoff check. Startup
injection now retains delivery origin metadata and exposes unavailable context
as failure rather than silently succeeding. See the
[local integration audit](../integrations/automatic-delivery.md).

Condition for live completion: locate or restore the existing SessionMesh
service and state, apply the prepared installation in a writable host session,
restart clients, and retrieve a handoff from the intended workspace. Durable
MCP task/decision recording can resume after that connection is restored.

Validation for connector maintenance:

- Eight isolated Python regression tests pass, including original backups,
  idempotent hooks/instructions, per-client command syntax, dry-run behavior,
  workspace/permission forwarding, missing-context failure, lifecycle ordering,
  and private handoff output suppression.
- Codex MCP registration was also repeated successfully against an isolated
  `/tmp` configuration using the installed CLI; the second add safely replaces
  the entry. The real user configuration was not modified.
- Six Rust MCP tests and MCP doc tests pass with Rust 1.88. Cargo's normal cache
  is read-only; the successful run used a dedicated cache under `/tmp`.
- Twelve Node repository/schema/security contract tests and twelve web tests
  pass. ESLint, TypeScript, and the production web build pass.
- Strict MkDocs, Compose configuration validation, shell syntax checks,
  `git diff --check`, and Prettier on changed Markdown/JavaScript files pass.
- The aggregate `npm run check` stops at pre-existing formatting differences in
  `docs/api/ingest.md`, `docs/architecture/deployment.md`,
  `docs/development/configuration.md`, and `docs/security/threat-model.md`.
  Those unrelated files were left unchanged; its later checks were run
  separately as recorded above.
- The live MCP check correctly fails because the configured container does not
  exist. No live-client success is claimed.

## Resource-management maintenance — 2026-11-20

The user authorized permanent prompts and AI replies, removal of redundant code
versions, scheduled cleanup, and bounded RAM/disk use. No SessionMesh MCP tools
were exposed; durable decisions and verified work are recorded here and in
ADR-011. The existing native home and `.git` remain read-only, so changes stay
uncommitted on the pre-existing non-main branch. Earlier connector work is
preserved.

Verified causes include whole-payload API/SSE and MCP workspace-resolution loads, unbounded correlation
vocabulary and snapshot history, full-JSON FTS duplication, whole-history Agy
and OpenCode imports, uncollected tool dumps/orphan blobs, and unbounded
container logs/resources. The local Cargo target also held about 13 GiB of
build artifacts, separate from session persistence.

A synthetic soak reproduced an additional Codex bug: resumed imports lost the
native session ID and created fallback-session/global-context entries. A
focused regression test failed (`fallback` versus `session-1`) before the
incremental parser was changed to seed native header identity/workspace. The
initial soak caught the duplicate context through its expected retained-handoff
count; that failure was investigated instead of relaxing the assertion.

Implemented shared persistent conversation retention, protected raw references,
legacy Claude tool-envelope cleanup, recoverable blob GC, bounded snapshot
retention, cleanup revision-triggered handoff rebuilding, exact-order metadata
pages, incremental session aggregates, bounded active context/cache, bounded
Codex/Claude/Agy reads, OpenCode continuation pages, administration commands,
container budgets, rotated logs, and the synthetic soak harness. Migration 0005
preserves existing conversations and stable IDs; tool-body deletion is performed
by the configured maintenance policy after upgrade, not by schema migration.
See the resource-management operations guide for backup, opt-out, and large
legacy database compaction requirements.

Validation and final measurements are recorded below after completion. A live
production/container rollout remains unavailable because the former service and
state volume are absent. No real native session data or unrelated Docker
storage has been deleted.

Cache cleanup removed generated dependency/build/incremental cache files totaling
12,600,235,657 apparent bytes (about 11.7 GiB). About 75 MB of inaccessible
incremental files remained due to ownership; permissions were not bypassed.
`target` fell to approximately 1.5 GiB including the isolated validation builds.
Native histories, existing executable paths, `.git`, images and volumes were
not removed. Disk usage also reflects concurrent host work and hard links, so
these cache-file counts are not an exact physical-volume reclamation measure.

Final validation for resource maintenance:

- All 156 Rust workspace tests and doc tests pass. Formatting and workspace
  Clippy with warnings denied pass. Tests cover exact chronology including
  nanoseconds/time zones and wide timestamp ranges, legacy schema upgrade and
  stable keys through vacuum, protected messages/shared raw data, idempotent
  cleanup/replay, legacy tombstones, bounded OpenCode/Agy/Codex processing,
  native header identity, metadata-only scope resolution, and context refresh
  after cleanup without new messages.
- The 70-second synthetic native run completed 1,284 authenticated API requests
  with active imports and a slow SSE subscriber. Warm RSS was 63,896 KiB;
  steady RSS was 63,912–63,928 KiB. All 728 expected conversation events survived,
  including Unicode/code content; tool-result bodies were absent and one latest
  handoff remained. Source hashes were unchanged. State including WAL/blobs was
  7,600,177 bytes for 27,966,755 bytes of synthetic source.
- Full npm formatting/lint/TypeScript/schema/security contracts, twelve web
  tests, the connector regression suite and production build pass. The earlier
  four Markdown formatting findings are resolved in documents updated for this
  resource-management change.
- Strict MkDocs, Compose configuration validation and `git diff --check` pass.
  Compose resolves to 1 GiB RAM/combined swap, one CPU, 128 PIDs, read-only root,
  64 MiB temporary space and three rotated 10 MiB logs.
- No multi-day production soak, image deployment, or Podman runtime validation
  is claimed. The old service and state volume remain absent. The operator must
  locate/restore state, allow migration space, and deploy the updated image to
  apply the new runtime and cleanup policy to real sessions.

## Publication and service startup — 2026-11-20

The user explicitly authorized committing, pushing a feature branch, and
starting the container in parallel. The source checkout's `.git` remains
read-only, so publication uses a separate writable checkout synchronized with
`origin/main` (`b1d5e69`, including the previous performance PR). The source
checkout's Git history/branch/index are not modified.

Compose build arguments now match the configured runtime UID/GID, preserving
ownership of a new private state volume. Rust image compilation uses two jobs
so build-time parallelism is bounded too. Docker Buildx activity data uses a
writable temporary directory in this restricted coding environment.

Publication verification: all 156 Rust tests/doc tests, strict Clippy,
formatting, the full npm check pipeline and strict MkDocs pass in the separate
checkout. The sensitive-data pattern scan found no credentials/private keys;
ignored local settings/runtime files are excluded from the commit.

An additional registry audit found existing dependency advisories. Compatible
transitive updates to Seroval 1.8.2 and seroval-plugins 1.8.0 remove the runtime
findings: `npm audit --omit=dev` reports zero. `npm audit` still reports 35
build/development dependency findings (2 critical, 29 high, 3 moderate, 1 low).
The deployed image serves static browser assets through Rust and contains no
Node runtime, Vite dev server or Vitest process. This is not a claim that every
reported advisory is exploitable or harmless.

Open maintenance task: resolve the remaining development/build dependency
advisories with compatible updates or a separately verified major-version
upgrade; rerun npm audit and the full web toolchain. No audit error was hidden
or reclassified as a passing full dependency audit.

The previously absent service/image/state volume are being recreated under the
user's startup authorization, using read-only native sources. Live deployment
verification is recorded after startup below.

Live startup follow-up: the first container reached healthy status with the
configured limits, user-only state, and read-only native mount, but the MCP
probe reproduced `database is locked` while ingestion held a writer. The cause
was schema/journal initialization on every ordinary storage open. A focused
writer-held regression timed out before the fix and now passes: current-schema
readers validate checksums/readiness through WAL without requesting a write
lock. Explicit migrations/policy writes still use their existing validation.
The archive scanner and reconciler now run independently so initial import
does not postpone all handoff generation. All 157 Rust tests/doc tests and
strict Clippy pass. A follow-up image build and live verification are underway;
no state volume is removed during recreation.

The real archive additionally reproduced `BudgetTooSmall` for a long prompt
copied into mandatory task/next-action fields. The regression failed with the
original budget error, then passed after adding bounded UTF-8-safe excerpts to
derived prose. Full messages and recorded observations are unchanged. All 158
Rust tests/doc tests and strict Clippy pass with both startup fixes. The image
is being rebuilt again to include this real-data context case; the existing
state volume and cursors are preserved across recreation.

### Deployed verification

The image `sha256:61c9015426a4255dc670dae99b7ddd9cdcde533df4b084c65128b7bca421a904`
was built and Compose recreated `agenttool-merger-sessionmesh-1` successfully.
The container is healthy; `/api/v1/health` returns `ok` and the web application
returns HTTP 200 at `http://192.168.155.225:8787/`. The state volume is retained
across recreation; native sources remain mounted read-only. Inspect confirms
UID/GID 1000, read-only root, 1 GiB memory/combined swap, one CPU and 128 PIDs.
An observed import-time sample used approximately 178 MiB; this is not a
long-running production soak result.

Inspection during active import counted 19,276 retained conversation events,
zero separate tool/code events and 194 generated handoffs. No maintenance
`--apply`, volume deletion or native-store cleanup was performed during this
startup verification.

MCP negotiation, all five tools and workspace handoff delivery pass the final
`scripts/check-sessionmesh-mcp` probe. Earlier probes returned
`no handoff for current session` until initial reconciliation reached this
workspace. Reconciliation logs also contain intermittent `database is locked`
errors during concurrent archive import. Open operational task: investigate
writer contention under sustained real-archive import and verify a longer soak.
The successful final MCP probe does not erase the observed contention or prove
multi-day resource stability.

The deployed source passes all 158 Rust tests/doc tests, strict Clippy, the full
npm check pipeline, formatting and strict MkDocs in the publication checkout.
The feature branch is published separately from `main`; no merge is performed.

## GUI content loading — 2026-11-20

The user reported missing GUI content. Authenticated production API probes
return sessions, events and canonical details successfully. A real headless
Chromium run reproduced 1,466 timeline cards despite the API page size of 200.
SSE replay expanded the selected page, and the subscription restarted whenever
its data changed. Revealing all content then launched hundreds of parallel
detail requests and reproduced browser `Failed to fetch` errors. Default
metadata-only display also lacked an explanation of how to reveal messages.

Focused regressions failed before the fix for bounded replay/pagination,
request concurrency/cancellation and visible content-error recovery. The GUI
now preserves API cursor boundaries, exposes previous/next event pages, keeps
live pages at most 200 cards and maintains one stream per selected page/filter.
Payload requests use four slots with cancellation and explicit retry controls.
Raw disclosure continues to require user action.

Validation: all 17 web tests and twelve repository/schema/security tests pass,
alongside formatting, ESLint, TypeScript, the production web build and strict
MkDocs. Rust behavior is unchanged; the OCI release compilation succeeds.
The first test run failed all four newly added behavioral regressions before
the implementation was changed; a fifth test additionally bounds a live tail.

The rebuilt image
`sha256:fd27328e5754cf0c43be2033a9431f290c2a504d95ae8a4580faa919b8f6fe93`
is deployed and healthy. A production headless Chromium check authenticated
without printing credentials or session text, loaded all 200 content blocks,
verified prompts/replies are visible, navigated to the next page and back, and
measured a peak of four simultaneous detail requests. JavaScript errors and
failed detail requests were both zero. MCP negotiation, its five tools and the
workspace handoff check also pass. The existing state volume is preserved.
The changes are published on `fix/gui-timeline-loading` without merging main.
SessionMesh decision recording was attempted but the connected MCP process
refused writes; this repository ledger preserves the confirmed result.

## GUI authentication recovery — 2026-11-20

The user reported `Sessions are temporarily unavailable` after deployment.
The running container is healthy. A current-token probe returns HTTP 200 with
200 session summaries; an invalid token returns HTTP 401 `unauthorized`.
The browser previously discarded HTTP status and rendered the same generic
message for invalid credentials and server/network outages. This reproduces
the reported message but does not establish which token the user's tab holds.

Two focused tests failed before implementation: HTTP status/callback behavior
and replacing a rejected saved token in the full application. The UI now clears
the rejected token and query cache, explains the authentication failure, shows
the current-token retrieval command and permits reconnection. A token-specific
guard ignores late failures from an older client. Non-401 failures preserve the
token. An additional test covers stream rejection without an authentication
retry loop. The exposed SessionMesh MCP transport was closed at session start;
verified state and changes are persisted in this ledger.

Validation passes: twenty web tests, twelve repository/schema/security tests,
formatting, lint, TypeScript, production build and strict MkDocs. The deployed
image is
`sha256:0d431135e26d59d3845ca8c79afb70c3a8d575d3832af1caef022d3f54060bd3`;
Compose and HTTP health are healthy. A real Chromium test seeded a rejected
saved token, verified automatic login recovery and cache/token removal,
reconnected using the current token, and verified 200 session buttons. Manual
token reset also passes. An intercepted HTTP 500 preserved the token, displayed
its status and recovered through Retry after the interception was removed.
There were zero JavaScript errors. Credentials/session text were not logged.
Changes are published on `fix/gui-auth-recovery`; the state volume is preserved.

## Grok Build import — 2026-11-20

The user explicitly requested Grok Build implementation. Previous integration
provided only outbound MCP context. Installed Grok Build 1.0.89 documentation
and local file-shape inspection identify `updates.jsonl` as the authoritative
conversation log; `chat_history.jsonl` reflects model context/compaction.
There are 156 native update logs totaling approximately 615 MB at inspection.
No private conversation or credential contents are copied into fixtures/docs.
The exposed SessionMesh MCP transport is still closed; state is verified locally.

Implemented exact-depth, read-only discovery in daemon and collector, bounded
append parsing of user/assistant/thought chunks, URL-encoded and `.cwd` workspace
mapping, native-time precedence, deterministic provenance and incremental
cursors. Nonconversation operational/tool updates are not another persistent
code history. Unknown native attachment content is preserved in the original
content object and raw evidence. Native stores remain read-only.

Focused integration tests failed before the adapter existed. They now cover
mixed conversation/tool records, Unicode/code preservation, partial appends,
stable identity, original path/byte offset, long-CWD sidecars, explicit malformed
records, bounded batches/oversized replies and replacement. A daemon integration
test verifies discovery through global-session handoff without source changes.
A GUI regression failed before adding archive cursor navigation to reach Grok
sessions beyond the first 200 records. Runtime deployment verification follows.

A read-only scan of the real archive found 979 user chunks, 3,154 assistant
chunks and 11,126 thought chunks, plus one malformed complete JSON line at byte
2,359,716 in one source. That source is intentionally not advanced through the
corrupt batch, and its native data remains unchanged. Open operational task:
resolve or explicitly quarantine that damaged native record through a verified,
lossless error-reporting design; do not silently skip it or rewrite the source.
Other sources continue independently. A GUI regression also caught duplicate
presentation of normalized text and its original Grok content envelope. The
presentation now shows text once while retaining expandable canonical evidence;
persisted event identities/content are unchanged.

Final validation: all 163 Rust workspace tests/doc tests, formatting and strict
Clippy pass. All 22 web tests and twelve repository/schema/security tests pass,
alongside ESLint, TypeScript, production build and strict MkDocs. The feature
branch was synchronized with remote main `32bcb95`; rebasing preserved the exact
tested file tree. The updated image
`sha256:0a9b330a55827fffe423b81a56d1c1e42ed5095b109f85aee61c23f469685394`
is deployed and healthy with its existing state volume and resource limits.

Authenticated production API inspection counted 152 Grok sessions and 15,046
retained conversation chunks. A canonical assistant-detail probe verified text,
workspace and original update-log provenance without logging contents. Headless
Chromium navigated to a later session page, selected a Grok conversation and
loaded all 76 visible event contents, including conversation messages, with zero
JavaScript errors. Fresh MCP negotiation, all five tools and the current
workspace handoff pass. The malformed native source remains blocked and is not
reported as a successful complete archive import. A post-recreation sample used
92.65 MiB of the 1 GiB limit; it is not a long-running memory-soak result.
Changes are published on `feat/grok-build-import`; main is not modified.

## Persistent agent navigation — 2026-11-20

The user reported OpenCode disappearing and explicitly requested all agent tools
in the left navigation without switching pages. Production API verification
found all 40 OpenCode sessions intact. The first 200-session page contained
Codex/Agy/Claude/Continue and 21 Grok sessions; the UI incorrectly derived the
agent tree from that page. Archive paging fixed reachability but did not satisfy
persistent tool visibility.

A focused API regression failed before the new inventory route existed, and a
GUI regression failed because later tools were absent. The storage layer now
aggregates family/session counts without loading events or enumerating complete
session summaries. The authenticated inventory keeps all agent headings visible
in a compact selector above the independently scrolling session list. Selecting
an agent resets session cursors
and loads that family's sessions through the existing server filter. Paging
controls live within the active branch; cross-tool search also updates scope.
Visible correlation cards retrieve individual metadata summaries for peers
outside the chosen agent, keeping paired context available without loading
other full session pages.
The inventory refreshes every 30 seconds. Payload paging and four-request detail
limits remain in effect. No session data is removed or re-imported for this fix.
The exposed MCP transport was closed at the session-start handoff attempt;
confirmed state and changes are recorded here.

Final validation: all 164 Rust workspace tests/doc tests, formatting and strict
Clippy pass. All 24 web tests, twelve repository/schema/security tests, lint,
TypeScript, production build and strict MkDocs pass. The deployed image is
`sha256:6a7f37fde83b06885253bbdf1548c5796fbaf80c182b4a349e3c6e4ea1c4cf30`.
The container is healthy and its existing state volume is preserved.

The real Chromium check found all six imported agents (Agy, Claude Code, Codex,
Continue, Grok and OpenCode) in the selector without paging. All entries fit
inside both the desktop and mobile viewport above the session list. Selecting
OpenCode displayed its 40 sessions and loaded conversation contents; switching
to Grok retained all agent entries, with 152 Grok sessions available. Every
session-page request was scoped to an agent. There were no speculative event
payload requests before disclosure and zero JavaScript errors. Health and a
fresh MCP five-tool/workspace-handoff check pass. The change is published on
`fix/all-tools-navigation` without modifying main.

## Web FAQ and cross-client continuation — 2026-11-20

The user requested a Web UI FAQ with per-agent native resume and SessionMesh
context commands, including continuation in another client after a rate limit.
All six clients' syntax was checked against installed CLI help: Codex 0.170.0,
Claude Code 2.1.328, Agy 1.5.13, OpenCode 2.2.7, Grok Build 1.0.89 and Continue
CLI 1.5.43. OpenCode help required temporary XDG directories because its default
log path is read-only; real client settings, histories and credentials were not
changed. No model sessions were started to validate documentation commands.

The static FAQ explains native-history ownership, new-session handoffs,
workspace/membership verification, search provenance, provider-quota boundaries
and coverage gaps. It uses the actual MCP read schema and documents Continue's
boolean resume flag and OpenCode's existing-ID requirement. It includes
copyable shell commands and clearly labelled in-agent prompts for each client.
Header and login links use FAQ hash routes/section anchors. The prior timeline
stays mounted when visited and is restored on return; a direct FAQ visit avoids
private archive requests even with a saved login. Clipboard denial or HTTP
secure-context restrictions produce explicit text-selection/manual-copy guidance.

Focused tests failed before implementation for the absent FAQ/navigation.
Coverage now verifies all client guides, rate-limit workflow, native/cross-client
semantics, copy selection, public/no-fetch routing and preserved timeline state.
The exposed SessionMesh MCP transport was closed at the initial handoff attempt;
confirmed facts and changes are persisted in this ledger and canonical docs.

Validation passes: all 31 web tests and twelve repository/schema/security tests,
formatting, ESLint, TypeScript, production build and strict MkDocs. Rust and API
behavior are unchanged. The deployed image is
`sha256:67ed385bd375c0173c0a3f7e38b317f54c4f342d37eb72109f67e00e5b298623`;
Compose and HTTP health are healthy and the state volume is preserved.

A real Chromium check loaded all six client guides without authentication and
made no private API requests, including after reloading a direct FAQ link with
a saved login. HTTP-origin command selection/manual-copy guidance, agent section
anchors and mobile width checks pass. After authenticated OpenCode navigation,
returning from the FAQ preserved the native session, filter, sort and grouping
without another session-page fetch. JavaScript errors were zero. No conversation
or credentials were printed. Fresh MCP negotiation, five tools and workspace
handoff delivery pass. Changes are published on `feat/web-faq-session-handoffs`
without modifying main.
