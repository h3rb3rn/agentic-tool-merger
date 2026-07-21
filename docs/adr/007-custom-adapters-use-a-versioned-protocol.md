# ADR-007: Custom Adapters Use a Versioned Protocol

- **Status:** Accepted
- **Date:** 2026-07-20

## Context

Unknown and enterprise tools require extensibility without linking every
adapter into the daemon. Arbitrary scripts create security and compatibility
risks.

## Decision

Support declarative profiles first, versioned NDJSON process adapters second,
and native Rust adapters only for important or performance-critical tools.
Arbitrary executable adapters require explicit approval and restricted
execution.

## Consequences

Adapters can use multiple languages while maintaining a stable contract.
Protocol negotiation, validation, health reporting, fixtures, and capability
declaration are mandatory.
