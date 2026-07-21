# Prompt 07: Codex Parser

## Objective

Parse Codex rollouts into immutable raw records and canonical events while
preserving enough source detail to reprocess every record.

## Prerequisites

Prompts 03, 05, and 06 are complete.

## Work

- Parse session metadata, user and assistant messages, tool calls and results,
  Git/CWD state, compaction, and supported lifecycle events.
- Preserve unknown event types as traceable native events.
- Isolate errors to the smallest source record and continue when safe.
- Redact nothing in immutable raw storage; mark sensitive fields for later
  controlled processing.
- Keep parsing pure and independent from filesystem watching and database
  transactions.

## TDD and acceptance

- Add sanitized real-shape fixtures and expected canonical output first.
- Cover multiline and Unicode payloads, missing optional fields, malformed
  records, unknown types, tool-call pairing, timestamps, and stable IDs.
- Every normalized event resolves to its raw source path and byte location.
- One bad record does not suppress valid subsequent records.
- Document mappings and limitations, run all gates, and select Prompt 08.
