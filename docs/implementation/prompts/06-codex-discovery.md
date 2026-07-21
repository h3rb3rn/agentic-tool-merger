# Prompt 06: Codex Discovery

## Objective

Discover Codex installations and session sources safely through
`$CODEX_HOME`, default paths, rollout layout, and the session index.

## Prerequisites

Prompts 02 and 05 are complete.

## Work

- Resolve explicit configuration, `$CODEX_HOME`, and `~/.codex` with documented
  precedence.
- Discover `sessions/YYYY/MM/DD/rollout-*.jsonl` and
  `session_index.jsonl`.
- Report installations, surfaces, source locations, permissions, and
  diagnostics without modifying or locking native files.
- Canonicalize identity while preserving the original display path.
- Tolerate missing, unreadable, stale, and partially populated locations.

## TDD and acceptance

- Create isolated directory fixtures before implementation.
- Cover custom home, standard home, symlinks, duplicates, unreadable paths,
  absent index, malformed index entries, and multiple installations.
- Repeated discovery is deterministic and performs no writes.
- Document discovery and troubleshooting, pass all gates, and select Prompt 07.
