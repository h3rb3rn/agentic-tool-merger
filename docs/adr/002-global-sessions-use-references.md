# ADR-002: Global Sessions Use References

- **Status:** Accepted
- **Date:** 2026-07-20

## Context

Related work can span several native sessions and tools. Copying transcripts
into a merged session would obscure origin, duplicate sensitive content, and
conflict with native session ownership.

## Decision

A global session is a graph that references native sessions. Membership stores
confidence, evidence, correlation version, and manual overrides. Original and
normalized events remain attached to their native session.

## Consequences

Every global view is traceable and reversible. Queries must traverse
membership, and manual linking or unlinking requires an audit record.
