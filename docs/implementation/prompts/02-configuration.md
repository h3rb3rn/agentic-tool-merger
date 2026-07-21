# Prompt 02: Configuration

## Objective

Implement typed, validated configuration with deterministic precedence:
defaults → user file → environment → CLI.

## Prerequisites

Prompts 00 and 01 are complete.

## Work

- Define configuration types for daemon binding, database and blob paths,
  ingestion, redaction, model endpoints, token budgets, and correlation
  thresholds.
- Support `SESSIONMESH_HOME`, with platform-native defaults outside containers
  and `/var/lib/sessionmesh` in the documented OCI runtime.
- Expand `~` and documented environment variables without executing shell
  syntax.
- Return field-specific validation errors and retain the source layer for
  diagnostics.
- Keep network access disabled unless explicitly enabled and bind to
  `127.0.0.1` by default.
- Document configuration locations, precedence, environment names, security
  defaults, and examples.

## TDD and acceptance

- Write precedence, missing-home, invalid-value, path-expansion, and
  security-default tests first. Cover container source mappings where the
  preserved host-origin path differs from the readable container path.
- Tests use isolated temporary homes and never read the developer's real
  configuration.
- CLI overrides win deterministically; invalid higher-precedence values do not
  silently fall back.
- All gates pass; update the ledger and select Prompt 03.
