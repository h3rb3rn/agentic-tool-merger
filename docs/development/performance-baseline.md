# MVP performance baseline

The baseline is a reproducible guardrail, not a production benchmark. It uses
the sanitized Codex rollout fixture on the current development host and inside
the pinned OCI toolchain.

## Measured journey

```mermaid
flowchart LR
    discover --> ingest --> rescan["Duplicate rescan"] --> query
    query --> handoff --> mcp
```

The automated end-to-end test records a five-second upper bound for discovery,
initial import, and duplicate rescan of the small fixture. It also asserts:

- duplicate rescan adds zero canonical events;
- every ingested event references an immutable raw object;
- native bytes are unchanged;
- handoff generation fits 4,000 approximate tokens; and
- MCP retrieves the stored compact handoff.

## Initial reference values

The following values were captured on 2026-07-20 from the release OCI image
with one sanitized eight-record Codex rollout. They are smoke-test references,
not capacity claims.

| Measure                              | Observed or bounded value |
| ------------------------------------ | ------------------------: |
| Native E2E including handoff and MCP |                    0.08 s |
| Discovery + import + rescan guard    |                     < 5 s |
| OCI steady-state memory              |                 19.01 MiB |
| SQLite database after import         |             262,144 bytes |
| Content-addressed blob directory     |               1,279 bytes |
| Local authenticated session query    |                  1.024 ms |
| REST list page                       |               ≤ 200 items |
| MCP search page                      |               ≤ 100 items |
| SSE client buffer                    |       128 event summaries |
| Handoff model timeout                |                      15 s |
| Default handoff budget               |  4,000 approximate tokens |

Database and blob growth are linear in unique native records. Duplicate scans
reuse deterministic event identities and content-addressed blobs. Release CI
should repeat these measurements with larger, redistributable fixtures before
performance claims or capacity targets are published.

## Known scaling limits

Ingestion and reconciliation cost is proportional to changed data (see
[ADR-010](../adr/010-derived-state-is-maintained-incrementally.md)): a rowid
watermark selects changed sessions, Claude Code transcripts resume from a byte
offset, OpenCode replays only updated sessions, and unchanged files are skipped
by size and modification time. Idle cost is one indexed lookup per reconcile
interval plus one `stat` per known file per scan tick.

Remaining limits:

- The daemon polls instead of using native watcher notifications.
- The correlator keeps one compact summary (terms and times) per session in
  memory; payloads are not retained.
- Candidate matching compares a new session against every existing member.
- The API still loads canonical events before sorting large result sets, and
  SQLite FTS5 stores a full copy of each canonical event.
- Continue and Agy snapshot files are reread in full when they change.
- Handoff snapshots are immutable and are not pruned.
