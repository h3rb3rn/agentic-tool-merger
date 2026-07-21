# Prompt 05: Adapter Contract

## Objective

Define the stable adapter SDK for discovery, full and incremental scans,
cursors, health, capabilities, and versioned process-adapter messages.

## Prerequisites

Prompts 03 and 04 are complete.

## Work

- Define typed Rust interfaces and error categories for discovery and scanning.
- Specify cursor ownership, cancellation, backpressure, partial success, and
  unsupported-capability behavior.
- Define and validate the NDJSON process protocol for requests, sessions,
  events, cursors, diagnostics, and protocol negotiation.
- Ensure adapters cannot mutate native sources through the SDK.
- Document declarative, process, and native adapter levels.

## TDD and acceptance

- Add contract tests reusable by every adapter.
- Test malformed NDJSON, incompatible versions, interrupted output, duplicate
  events, partial success, cancellation, and untrusted diagnostics.
- No adapter error can erase a committed cursor or be promoted to an
  instruction.
- Publish protocol examples, run all gates, update the ledger, and select
  Prompt 06.
