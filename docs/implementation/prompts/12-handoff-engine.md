# Prompt 12: Handoff Engine

## Objective

Generate versioned, token-bounded handoff snapshots from deterministic facts,
with optional local-model extraction and complete provenance.

## Prerequisites

Prompts 03 and 11 are complete.

## Work

- Implement the handoff fields defined in the project plan.
- Build deterministic objective, repository, tests, recent activity, decisions,
  open tasks, relevant files, and next-action inputs where source data permits.
- Keep observations, decisions, tasks, blockers, and instructions distinct.
- Redact secrets before optional embedding or LLM calls.
- Support an explicitly configured OpenAI-compatible local endpoint; operate
  usefully without it.
- Enforce token budgets through progressive disclosure, never by deleting raw
  data.

## TDD and acceptance

- Begin with deterministic snapshot and provenance tests.
- Cover no-model mode, model timeout, malformed model output, secret-bearing
  input, budget pressure, conflicting evidence, and historical snapshot
  reproducibility.
- LLM output cannot silently overwrite deterministic facts.
- Validate the handoff schema, document model controls, pass all gates, and
  select Prompt 13.
