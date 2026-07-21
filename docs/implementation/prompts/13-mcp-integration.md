# Prompt 13: MCP Integration

## Objective

Expose current context, handoffs, search, decisions, and tasks through MCP with
strict schemas, controlled writes, and Codex installation guidance.

## Prerequisites

Prompts 11 and 12 are complete.

## Work

- Implement resources for current global-session context and current handoff.
- Implement typed tools equivalent to `get_current`, `get_handoff`, `search`,
  `record_decision`, and `record_task`.
- Design names and wire schemas consistently with the full planned
  `sessionmesh_*` interface.
- Authenticate and audit writes; validate global-session scope and distinguish
  user observations from instructions.
- Add Codex MCP configuration generation or documented installation without
  mutating unrelated user settings.
- Publish the current derived context automatically at session start where the
  tool supports discovery, and mark delivered handoffs with origin and version
  metadata so they cannot create ingestion loops.

## TDD and acceptance

- Add protocol conformance and end-to-end client tests first.
- Cover missing current session, stale handoff, invalid schema, unauthorized
  write, duplicate request, prompt-injection content, cancellation, and search
  pagination.
- A fresh Codex session can retrieve the compact handoff and fetch details only
  on demand.
- Document resources, tools, permissions, and installation; pass all gates and
  select Prompt 14.
