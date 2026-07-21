# Prompt 14: MVP Hardening

## Objective

Prove that Milestones 0–2 satisfy their definitions of done under realistic
failure, restart, performance, privacy, and security conditions.

## Prerequisites

Prompts 00–13 are complete.

## Work

- Build an end-to-end fixture journey from Codex discovery through incremental
  ingestion, REST/SSE, browser timeline, global linking, handoff, and MCP.
- Threat-model local HTTP, MCP writes, raw views, path handling, native-store
  access, adapter processes, model endpoints, and ingestion loops.
- Measure import latency, memory, database and blob growth, query latency, and
  handoff generation on representative fixtures.
- Run crash/restart, corrupt-input, migration, backup/restore, and permission
  scenarios.
- Run the same end-to-end journey natively and through the OCI image under
  Docker or Podman. Verify persistent volume restart, non-root ownership,
  health checks, localhost publishing, and read-only source mounts.
- Remove root causes of instability; do not relax tests or add unbounded
  retries.

## Acceptance

- Repeated scans are duplicate-free.
- Growing files resume from their committed offsets.
- Partial lines, parser errors, rotations, and restarts lose no confirmed data.
- Raw and canonical events are mutually traceable.
- A new Codex session appears in the browser within seconds.
- Global-session membership is reversible and audited.
- MCP provides a compact handoff with progressive detail retrieval.
- Native stores remain byte-for-byte unchanged during the test suite.
- All code, web, schema, security, and strict documentation gates pass.
- Record performance baselines, known limitations, release readiness, and the
  next roadmap decision in the ledger and changelog.
