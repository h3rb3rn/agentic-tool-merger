# Prompt 01: Architecture Baseline

## Objective

Turn the accepted architecture into machine-checkable boundaries, schemas, and
fixtures before domain implementation begins.

## Prerequisites

Prompt 00 is complete.

## Work

- Verify ADR-001 through ADR-007 against the actual workspace and correct only
  factual bootstrap details without changing their decisions.
- Create initial versioned JSON Schemas for canonical events, tool profiles,
  and handoffs.
- Establish fixture directories, manifest conventions, provenance metadata,
  expected-output files, and secret-safe fixture rules.
- Define crate dependency direction and prevent circular dependencies.
- Document schema compatibility, versioning, fixture review, and ADR
  supersession.

Do not add persistence or native-tool parsing.

## TDD and acceptance

- Start with failing schema validation tests for valid, invalid, unknown-field,
  and unsupported-version examples.
- All schemas validate their examples and reject incompatible versions with
  actionable errors.
- Dependency-boundary checks, all workspace gates, and strict MkDocs build pass.
- Record the result and set Prompt 02 next.
