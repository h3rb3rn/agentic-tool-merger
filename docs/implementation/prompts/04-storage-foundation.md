# Prompt 04: Storage Foundation

## Objective

Implement transactional SQLite persistence for immutable raw imports,
normalized events, native sessions, and ingestion cursors.

## Prerequisites

Prompts 02 and 03 are complete.

## Work

- Add forward-only migrations for the central tables needed through Milestone
  2, while implementing repositories only for current behavior.
- Enable and verify WAL, foreign keys, busy timeout, and `0600` database
  permissions.
- Add a content-addressed blob store with atomic writes and hash verification.
- Make raw objects immutable and event inserts idempotent.
- Update cursors atomically with successfully committed imported events.
- Add FTS5 structures needed for later event search.

## TDD and acceptance

- Write transaction rollback, concurrent reader/writer, duplicate insert,
  immutable-raw, blob-integrity, cursor-atomicity, and permission tests first.
- A failed batch advances no cursor and leaves no partially visible event set.
- Reopening the database preserves settings and data.
- Document schema, retention boundaries, backup implications, and migration
  policy; pass all gates and select Prompt 05.
