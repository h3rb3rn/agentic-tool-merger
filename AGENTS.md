# SessionMesh Agent Instructions

This file is the mandatory entry point for every agent working in this
repository. These instructions apply to the entire repository.

## Restart Protocol

Before changing code or documentation:

1. Read this file completely.
2. Read `docs/architecture/project-plan.md`.
3. Read all accepted records in `docs/adr/`.
4. Read `docs/development/implementation-status.md`.
5. Read the implementation prompt named as `Next prompt` in that status file.
6. Inspect the repository and verify that the recorded state still matches it.

Do not rely on conversation history as the only source of project knowledge.
Persist decisions, progress, and changes in the repository.

## Language

English is the default and required language for:

- source code, identifiers, and code comments;
- repository documentation and diagrams;
- schemas, configuration descriptions, and UI text;
- tests, fixtures, errors, and log messages;
- ADRs, change notes, and commit messages.

The user may communicate in another language. That does not change the language
of repository artifacts unless the user explicitly requests a localized
artifact.

## Product Definition

SessionMesh is a local-first observability, correlation, and handoff layer for
heterogeneous coding agents. It imports native sessions without modifying them,
normalizes their events with complete provenance, links related native sessions
through a global-session graph, and provides compact handoffs through MCP and
other controlled integrations.

The first release must be exceptionally reliable at:

1. incrementally importing Codex sessions;
2. normalizing native events with traceable provenance;
3. managing cross-tool global sessions; and
4. delivering compact handoffs through MCP.

## Non-Negotiable Architecture Rules

- Native agent stores are read-only.
- Raw imported data is immutable and is never replaced by a summary.
- Global sessions reference native sessions; they do not copy or destructively
  merge their transcripts.
- Normalized event IDs are deterministic. Repeated scans must be idempotent.
- Every normalized fact retains provenance to its native source.
- SQLite in WAL mode is the initial database.
- Progressive disclosure is the default handoff strategy.
- Deterministic correlation precedes semantic correlation.
- LLM output is supporting evidence, never the sole source of truth.
- MCP is the primary agent-memory and controlled-write integration layer.
- ACP is used for agent/client session transport where appropriate.
- Session contents remain local unless the user explicitly enables a remote
  service.
- Generated handoffs must not be re-imported as native session input.
- SessionMesh actively watches configured sources and maintains a shared
  derived context without requiring an agent to initiate synchronization.
- Synchronization writes to tools only through documented MCP, ACP, hook, CLI,
  or API integration points; native transcript stores remain read-only.
- Cross-source chronology never relies on timestamps alone. Preserve native
  order and use deterministic tie-break evidence.

## Engineering Method

Use test-driven development for behavioral changes:

1. Write a focused failing test that expresses the required behavior.
2. Implement the smallest sustainable design that makes it pass.
3. Refactor for clarity while keeping the suite green.
4. Add integration or end-to-end coverage where component boundaries matter.

Fix causes, not symptoms. Investigate why a failure exists before changing the
implementation. Do not add retries, fallbacks, special cases, ignored errors,
or test exceptions merely to make behavior appear functional.

A temporary mitigation requires explicit user approval and must include:

- a documented limitation and operational impact;
- a tracked follow-up task;
- tests that define the temporary behavior; and
- an ADR when architecture or compatibility is affected.

Sustainability, correctness, security, provenance, and maintainability take
priority over delivery speed.

## Code Quality

- Prefer clear names, cohesive modules, explicit types, and straightforward
  control flow over clever or compressed code.
- Keep formatting readable; never remove whitespace merely to reduce size.
- Treat code as the developer's primary documentation.
- Add documentation comments to public APIs and all relevant functions.
- Comments must explain purpose, invariants, security boundaries, non-obvious
  behavior, or why a function matters. Do not restate obvious syntax.
- Keep parsing, persistence, correlation, presentation, and transport concerns
  separated.
- Preserve errors and provenance instead of silently discarding them.
- Avoid speculative abstractions, but maintain the adapter and schema
  boundaries defined by the architecture.

Run formatting, linting, unit tests, integration tests, schema validation, and
documentation checks appropriate to the changed area before marking work
complete.

## Documentation and Change Control

Material for MkDocs is the canonical documentation system. Every functional,
architectural, operational, or configuration change must update the relevant
page under `docs/` in the same change.

Also:

- record architectural decisions in `docs/adr/`;
- record user-visible and operational changes in `docs/changes/changelog.md`;
- keep examples, interfaces, and configuration documentation synchronized with
  code;
- document rationale, security implications, compatibility, and migration
  effects when relevant; and
- ensure `mkdocs build --strict` succeeds.

## Prompt Execution Protocol

Implementation prompts live in `docs/implementation/prompts/` and are executed
in numeric order. `docs/development/implementation-status.md` is the canonical
execution ledger.

- Only one prompt may be `in_progress`.
- Do not start a later prompt while a predecessor is incomplete.
- Minimal interface seams needed by the active prompt are allowed; implementing
  later behavior early is not.
- A prompt is complete only after its acceptance criteria, tests,
  documentation, and change record are complete.
- Update the execution ledger after every meaningful implementation session.
- If blocked, record the verified cause, evidence, attempted sustainable
  solutions, and exact condition required to continue.
- If repository state conflicts with the ledger, investigate and correct the
  ledger before implementation continues.

## Initial Commit Gate

The repository remains uncommitted while the ordered MVP prompts are still in
progress. Prepare the initial public commit only after Prompt 14 is complete,
all quality and security gates pass, generated/runtime artifacts are excluded,
and a final sensitive-data audit reports no publishable findings. Initialize,
stage, or commit Git history earlier only if the user explicitly supersedes
this gate.

## Security Baseline

- Open native stores read-only.
- Default network binding is `127.0.0.1`.
- Protect state-changing HTTP operations with local authentication and CSRF
  controls as applicable.
- Store the SessionMesh database with user-only permissions (`0600`).
- Redact secrets before embeddings or LLM processing.
- Keep observations, decisions, and instructions as distinct data.
- Never promote tool output to system instructions automatically.
- Require explicit approval for arbitrary custom scripts.
- Restrict adapter-process privileges.
- Audit manual session links and overrides.

## Deployment Contract

SessionMesh must run both as a native service and as an OCI container under
Docker or Podman.

- Native installations follow platform conventions and support an explicit
  `SESSIONMESH_HOME`.
- Containers use `/var/lib/sessionmesh` as `SESSIONMESH_HOME`; mount a
  persistent volume there.
- Mount native agent-session sources separately under `/sources/<tool>` and
  read-only by default.
- Never assume that the container user's `~` is the host user's home.
- Use the same image, configuration model, health endpoint, and documented
  mount contract for Docker and Podman.
- Run as a non-root user, publish only the configured localhost port by
  default, and document UID/GID and SELinux considerations.
- Container deployments may use a small native host connector for hooks, local
  sockets, CLI integrations, and sources that cannot be safely mounted.

## Scope Boundaries

Milestones 0–2 define the current executable MVP scope. Milestones 3–7 remain
documented roadmap items until the Codex vertical slice, global sessions, and
MCP handoff are stable.

Do not implement the explicitly deferred first-release exclusions listed in
`docs/architecture/project-plan.md` without a new approved decision.
