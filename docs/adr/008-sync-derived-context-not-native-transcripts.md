# ADR-008: Synchronize Derived Context, Not Native Transcripts

- **Status:** Accepted
- **Date:** 2026-07-20

## Context

Users should be able to switch agent tools without manually copying context.
Native transcript formats are tool-owned, often undocumented, and not mutually
compatible. Writing merged histories into those stores risks corruption,
duplicate ingestion, and broken native resume behavior.

## Decision

SessionMesh actively discovers and watches configured native sources. It
maintains a canonical event log, global-session state, and current handoff
without requiring an agent action.

Outbound synchronization sends derived context through the best documented
integration supported by each surface:

1. MCP resources or tools;
2. session-start or resume hooks;
3. ACP session operations;
4. documented local APIs;
5. a SessionMesh wrapper; or
6. generated handoff files.

Native transcript and session stores remain read-only. SessionMesh does not
promise byte-for-byte transcript replication across tools.

## Consequences

An agent receives the same objective, decisions, tasks, repository state, and
relevant history while keeping its native session semantics. Connectors declare
their inbound and outbound capabilities. Generated context carries an origin
marker so collectors can prevent synchronization loops.

Some tools require one-time integration configuration or a small native host
connector. A tool without a supported injection surface can still be observed,
but automatic outbound context delivery may be limited to a generated file.
