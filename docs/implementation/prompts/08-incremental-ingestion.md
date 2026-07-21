# Prompt 08: Incremental Ingestion

## Objective

Import growing Codex JSONL files exactly once using durable offsets,
partial-line buffering, rotation detection, watching, debounce, and bounded
retry.

## Prerequisites

Prompts 04, 06, and 07 are complete.

## Work

- Identify source generations using stable filesystem metadata and content
  evidence rather than path alone.
- Read only bytes after the committed cursor.
- Persist incomplete final lines without parsing or advancing beyond confirmed
  records.
- Handle append, truncate, rename, move, replacement, duplicate notifications,
  daemon restart, and files discovered during watcher downtime.
- Preserve native sequence and timestamp metadata, assign durable ingestion
  sequence, and expose the deterministic multi-signal ordering key defined by
  ADR-009.
- Retry only classified transient failures with bounded backoff and visible
  diagnostics.

## TDD and acceptance

- Write deterministic filesystem integration tests before watcher logic.
- Reproduce append, crash-before-commit, crash-after-commit, partial line,
  rotation, replacement at the same path, rename, and event storms.
- Repeated scans create no duplicates and no data loss.
- Identical timestamps, clock skew, and late arrivals have deterministic,
  explainable order without changing source-local chronology.
- A newly appended Codex event becomes queryable within seconds.
- Document cursor semantics and recovery, pass all gates, and select Prompt 09.
