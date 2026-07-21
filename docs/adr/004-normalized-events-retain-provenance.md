# ADR-004: Normalized Events Retain Provenance

- **Status:** Accepted
- **Date:** 2026-07-20

## Context

Normalization can lose native detail, and parsers evolve. Users must be able to
verify every derived fact against its imported source.

## Decision

Every normalized event stores source identity, source offset or equivalent
location, adapter version, and deterministic event identity. Raw objects remain
available and immutable.

## Consequences

Schema and repository APIs treat provenance as required data. Reprocessing can
produce versioned interpretations without deleting the original import.
