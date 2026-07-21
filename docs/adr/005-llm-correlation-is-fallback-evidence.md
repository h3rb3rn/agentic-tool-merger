# ADR-005: LLM Correlation Is Fallback Evidence

- **Status:** Accepted
- **Date:** 2026-07-20

## Context

Semantic models can relate sessions with different wording, but their output is
probabilistic, may expose sensitive content, and cannot reliably explain
identity on its own.

## Decision

Run explicit and deterministic correlation first. Invoke an explicitly
configured local model only for ambiguous cases. Store model output as
additional evidence, never as the sole source of truth.

## Consequences

The system remains useful without a model. Secret redaction precedes model
input, uncertain results enter a review queue, and manual decisions override
automated candidates.
