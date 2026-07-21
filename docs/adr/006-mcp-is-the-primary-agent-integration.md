# ADR-006: MCP Is the Primary Agent Integration

- **Status:** Accepted
- **Date:** 2026-07-20

## Context

Agents need progressive access to current state, search, and constrained
updates without receiving entire transcripts. ACP addresses agent/client
session transport rather than shared memory semantics.

## Decision

Use MCP resources and strictly typed tools as the primary context, search, and
controlled-write integration. Use ACP where native session creation, loading,
or resumption is required.

## Consequences

Handoffs are progressively disclosed. Hooks, wrappers, files, and manual copy
remain fallbacks. MCP writes require authentication, validation, and auditing.
