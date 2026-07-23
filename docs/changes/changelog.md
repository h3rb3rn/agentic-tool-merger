# Changelog

All notable functional, architectural, configuration, security, and operational
changes to SessionMesh are recorded here.

## Unreleased

### Added

- Beginner-oriented setup and usage guide covering service checks, connector
  installation, Web UI authentication, native resume, cross-tool rate-limit
  handoff, correlation review, manual fallback, and troubleshooting.
- Read-only OpenCode SQLite ingestion restricted to allowlisted session-content
  tables and Agy history ingestion that excludes OAuth and credential state.
- Thread-title session navigation with native metadata and bounded fallbacks,
  plus topic, date, and normalized-size sorting and grouping.
- Hierarchical, collapsible tool and session-group navigation with counts,
  active-branch expansion, local filtering, and a bounded scroll region.
- Responsive sidebar constraints verified in Chromium at desktop and mobile
  widths, preventing long global-session labels from widening the page.
- Bounded, searchable Correlation Review with paired thread context cards,
  expandable score explanations, and deterministic shared-keyword evidence.
- Explainable cross-tool content correlation with persisted workspace,
  temporal, and lexical evidence, configurable automatic linking, and an
  authenticated accept/reject review interface.
- Authenticated cross-tool keyword search with tool-family, native-session,
  normalized content excerpt, and provenance-path results.
- Human-readable timeline content mode for messages, commands, and results,
  with one explicit sensitive-data acknowledgement and canonical JSON details.
- Configurable, interface-specific OCI publication through
  `SESSIONMESH_PUBLISH_ADDRESS`, including LAN exposure guidance.
- Automatic cross-tool correlation and handoff refresh, read-only Claude Code
  and Continue adapters, and startup delivery connectors for Codex, Claude
  Code, and Continue.
- Read-only host agent-home mounting for OCI discovery, with Codex resolved
  below the shared profile root and persistent state kept separately.
- Persistent branch protection instructions requiring purpose-specific
  branches and pull-request integration instead of direct pushes to `main`.
- Persistent repository instructions for language, TDD, Clean Code,
  root-cause engineering, documentation, and security.
- Material for MkDocs documentation foundation.
- Canonical SessionMesh architecture and implementation plan.
- Initial architecture decision records.
- Ordered implementation prompts and restart-safe execution ledger for
  Milestones 0–2.
- Native and Docker/Podman deployment contract with persistent
  `SESSIONMESH_HOME` and read-only source mounts.
- Automatic synchronization of derived context through supported integration
  surfaces while native transcripts remain read-only.
- Deterministic chronology based on native order, timestamps, source offsets,
  ingestion sequence, and stable event identity.
- Rust 1.88 workspace with the planned crate boundaries, a React/Vite
  application shell, reproducible lockfiles, shared quality gates, and a
  non-root OCI multi-stage build.
- Strict Draft 2020-12 schemas for canonical events, tool profiles, and
  handoffs, including validated examples and automated crate-dependency
  boundary checks.
- Typed configuration resolution with source provenance, deterministic
  precedence, safe path expansion, secure local defaults, container source
  mapping, and field-specific validation.
- Versioned canonical Rust event types with RFC 3339 timestamp preservation,
  unknown-native-event retention, deterministic SHA-256 IDs, stable canonical
  JSON, and tamper detection.
- Safe environment template, expanded secret and local-state ignore rules,
  documented primary variables, and an automated repository-safety contract
  for the canonical GitHub repository.
- Transactional SQLite WAL storage with forward-only migrations, immutable raw
  metadata, idempotent canonical events, atomic cursors, FTS5, and verified
  content-addressed blobs.
- Public security policy covering private vulnerability reporting, repository
  hygiene, runtime defaults, and secret-rotation expectations.
- Versioned read-only adapter SDK with explicit capabilities, cursor ownership,
  cooperative cancellation, bounded backpressure, partial completion, typed
  errors, and strict negotiated NDJSON process messages.
- Deterministic Codex discovery with explicit, environment, and default home
  precedence; container-to-host provenance mappings; dated rollout and session
  index detection; symlink deduplication; permission metadata; and non-fatal
  diagnostics for missing, malformed, or unreadable sources.
- Pure Codex rollout parsing for session metadata, user and assistant messages,
  tool calls and results, lifecycle, CWD/Git state, compaction, plans, errors,
  and loss-preserving unknown events, with record-level failure isolation and
  sensitive-field markers.
- Cursor-driven Codex ingestion with stable file generations, confirmed
  offsets, native sequence persistence, partial-line recovery, append,
  truncate, replacement and rename handling, atomic commits, event-storm
  coalescing, bounded retry classification, and deterministic global ordering
  keys.
- Versioned Axum REST and SSE read API with constant-time local bearer
  authentication, safe payload-free projections, stable filters and cursors,
  deterministic chronology, resumable event IDs, bounded live buffering, and
  sanitized error envelopes.
- Responsive, keyboard-accessible native-session timeline with typed TanStack
  Query access, event filtering, tool activity grouping, route-safe selection,
  idempotent authenticated SSE reconciliation, explicit sensitive-detail
  disclosure, and loading, empty, error, reconnect, and stale-data states.
- Read-only Git repository and worktree identity, normalized remotes, detached
  and dirty-state handling, strict atomic project markers, versioned
  deterministic correlation evidence, reference-only global sessions,
  persistent manual rejections, and transactional membership audit.
- Reproducible handoff generation from deterministic event facts with
  field-level provenance, progressive token budgeting, secret redaction,
  immutable snapshot persistence, optional timeout-bounded OpenAI-compatible
  local extraction, and deterministic precedence over model suggestions.
- MCP 2025-06-18 stdio resources and strictly typed tools for current context,
  compact handoffs, bounded event search, and idempotent decision/task
  observations, with process-scoped write authorization, stale/origin
  metadata, cancellation-safe request handling, and documented Codex setup.
- Daemon-owned active Codex discovery and incremental polling with automatic
  SSE publication, explicit container-local `CODEX_HOME`, raw-to-canonical
  foreign-key traceability, an end-to-end fixture journey, an MVP threat model,
  and bounded performance and scaling baselines.
