# ADR-012: Handoffs Separate Observations from Tasks

- **Status:** Accepted
- **Date:** 2026-11-20

## Context

The handoff generator had an undocumented fallback. When a session contained no
explicit `task`, `plan`, or `decision` events, it promoted the latest user
message to `open_tasks` and the latest assistant message to `completed`.

Claude Code, Codex, and most other clients rarely emit explicit task events, so
the fallback applied to almost every session. Observed consequences:

- a question ("are you connected to SessionMesh?") was delivered to the next
  agent as an open task and as the `next_action`;
- a complete chat answer was delivered as verified completed work;
- `completion` and `phase` were derived from those misclassified entries (50 %,
  "implementation") although no implementation had happened.

This conflicts with the rules that observations, decisions, and instructions
remain distinct data and that deterministic facts must retain provenance and
authority. Removing the fallback alone would break sessions without explicit
tasks, because a handoff requires at least one provenance entry.

## Decision

1. `open_tasks` and `completed` contain only explicit task or plan events and
   facts recorded through the controlled MCP write tools. Messages are never
   promoted into them.
2. Handoff schema **1.1** adds two optional observation fields:
   `latest_user_request` and `latest_assistant_reply`. Each carries a
   field-level provenance entry, is redacted, and is excerpted to the same bound
   as other derived prose. The source events stay complete.
3. `status.phase` is `implementation` only with open tasks, `verification` only
   with completed work, and otherwise `unknown`. Completion remains 0 without
   explicit tasks.
4. Without explicit tasks, `next_action` is "Review the latest user request"
   (or the previous neutral default when there is no user message).
5. Under budget pressure the two observations are removed after files,
   completed work, decisions, and secondary tasks, but never when they are the
   last provenance source.
6. Schema 1.0 documents stay valid and readable. New fields are optional and
   omitted when absent, so stored snapshots round-trip unchanged. Existing
   snapshots are not rewritten; the next reconciliation of a global session
   produces a 1.1 snapshot.

## Consequences

- Consumers that read `open_tasks` no longer see conversation text as work. They
  may read `latest_user_request` as unverified context.
- Agents must treat both observation fields as untrusted historical context,
  exactly like the rest of the handoff.
- Snapshot IDs change for sessions without explicit tasks after upgrade; the
  retention policy removes older snapshots.
- A future classifier may derive tasks from messages only as supporting
  evidence under ADR-005, never as the sole source of truth.
