# Prompt 03: Canonical Event Model

## Objective

Implement versioned Rust domain types that faithfully represent canonical
events, workspace context, tool identity, payloads, and provenance.

## Prerequisites

Prompt 01 is complete; consume configuration only where required by tests.

## Work

- Model every canonical event kind listed in the project plan.
- Represent unknown future native kinds without discarding raw data.
- Define deterministic event-ID input and canonical serialization rules.
- Keep observations, decisions, tasks, and instructions semantically distinct.
- Generate or verify JSON Schema from the Rust types and document compatibility.

## TDD and acceptance

- Begin with fixture round-trip and deterministic-hash tests.
- Cover field-order independence where promised, timestamp precision, Unicode,
  optional workspace data, unknown native events, and schema-version rejection.
- Equal canonical inputs produce equal IDs; semantically relevant differences
  produce different IDs.
- Rust types, published schema, and examples stay synchronized.
- Run all gates, update documentation and changelog, then select Prompt 04.
