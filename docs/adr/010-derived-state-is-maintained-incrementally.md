# ADR-010: Derived State Is Maintained Incrementally

- **Status:** Accepted
- **Date:** 2026-10-09

## Context

A production instance with about 236,000 events and 319 native sessions ran at
roughly 100 % CPU, 14.6 GiB resident memory, and 113 GB of cumulative block
writes. The daemon repeated work proportional to the **entire history** on every
interval:

- reconciliation loaded every canonical event with payload, re-tokenized all
  text, and regenerated the handoff of every global session;
- Claude Code transcripts were reread, reparsed, and replayed into storage in
  full whenever they grew by a single line;
- the OpenCode importer replayed every text part on any database change; and
- every known file was checked against a database cursor on each scan tick.

## Decision

Derived state and imports are maintained incrementally, and cost must be
proportional to what changed:

1. **Change detection by watermark.** Storage exposes
   `native_sessions_changed_since(rowid)`. The `native_events` rowid advances
   only when an event is genuinely inserted, so idempotent rescans never look
   like activity. Local scans and remote collectors are detected uniformly.
2. **Per-session reconciliation.** The reconciler keeps compact session
   summaries in memory, reloads only changed sessions (one at a time), and
   refreshes handoffs only for the global sessions that contain them. A restart
   rebuilds everything once from immutable events.
3. **Append-only transcripts use cursors.** Claude Code ingestion resumes at the
   last complete line and its next sequence. A shrunken file or a different
   first line triggers a full, idempotent reread.
4. **OpenCode replays whole changed sessions.** Event sequences are ranks within
   a session, so a session with any part updated since the stored
   `time_updated` high-water mark is replayed; unchanged sessions are skipped.
5. **Unchanged files are not examined.** The scan remembers each file's size and
   modification time in memory and skips unchanged files.

## Consequences

- Idle cost is one indexed `MAX(rowid)` lookup per reconcile interval and one
  `stat` per known file per scan tick.
- Memory scales with the number of sessions (terms and times), not with event
  payload volume.
- Event and raw-object identities are unchanged, so existing databases remain
  valid. Old Claude and OpenCode cursors use a different identity or watermark
  meaning and trigger exactly one idempotent full reread after upgrade.
- Claude lines are imported only once their terminating newline exists.
- The OpenCode cursor's `byte_offset` now stores a `time_updated` high-water
  mark. No schema migration is required.
- Handoff snapshots remain immutable and content-addressed; their number now
  grows only with real changes to a session.
- Continue and Agy snapshot imports still reread a changed file in full; they
  are small, rewritten documents.
