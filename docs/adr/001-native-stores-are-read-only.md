# ADR-001: Native Stores Are Read-Only

- **Status:** Accepted
- **Date:** 2026-07-20

## Context

Native session stores belong to their agent tools and may contain undocumented
formats, complete transcripts, tool output, and secrets. Mutating them risks
corruption and can prevent a tool from resuming its own session.

## Decision

SessionMesh opens native stores read-only. It writes imported bytes, metadata,
cursors, normalized events, and derived state only to SessionMesh-owned
storage.

## Consequences

Native tools retain ownership and resumability. SessionMesh cannot implement
cross-tool resume by rewriting native data. Adapters and tests must verify that
collection performs no source writes.
