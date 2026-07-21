# ADR-003: SQLite Is the Initial Database

- **Status:** Accepted
- **Date:** 2026-07-20

## Context

SessionMesh is initially a single-user local daemon that needs transactions,
incremental ingestion, full-text search, simple deployment, and low resource
use.

## Decision

Use SQLite in WAL mode with FTS5. Store large payloads in a
content-addressed filesystem blob store. A vector extension is optional.

## Consequences

The application ships without an external database service. Repository
interfaces must preserve a future path to a server database without weakening
SQLite transaction or concurrency tests.
