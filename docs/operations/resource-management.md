# Resource management and cleanup

SessionMesh permanently retains prompts and AI replies, including code quoted
inside them. It no longer archives every separate tool/file/code version by
default. Native client stores are always read-only.

## Verified growth paths and fixes

| Growth path found in code                                                                               | Change                                                                                                                                  |
| ------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------- |
| Session lists, event pages, MCP workspace resolution and SSE loaded/deserialized all canonical payloads | Incremental SQL session aggregates and indexed metadata pages/workspace lookup; SSE replay pages contain at most 256 summaries          |
| Initial or lagging Codex/Claude imports read all remaining bytes                                        | 4 MiB newline batches with exact cursor continuation; one larger record stays intact                                                    |
| Agy prompt history reread the changed history file                                                      | Append cursor and bounded newline batches                                                                                               |
| OpenCode loaded every changed session's text rows at once                                               | Keyset pages of at most 128 rows and 4 MiB of text, preserving native ranks and continuation across scans                               |
| Correlation cached every text token and stale discovered paths                                          | At most 512 bounded tokens per summary and 2,048 active summaries; deleted source paths leave the stamp cache                           |
| Handoff input grew with the entire linked transcript                                                    | Latest 256 events with a 4 MiB target, plus the latest 128 typed observations; original objective and the full archive remain available |
| Every handoff/snapshot version lived forever                                                            | Latest version per global session by default, configurable from 1 to 100                                                                |
| FTS duplicated and indexed canonical JSON including metadata                                            | Index only textual content; original event remains in the canonical store                                                               |
| Tool dumps were kept in both raw blobs and canonical JSON                                               | Shared conversation retention policy filters new tool bodies and cleans historical ones                                                 |
| Failed blob publications and obsolete derivatives had no collector                                      | Committed deletion queue and streaming orphan collection with a 24-hour grace period                                                    |
| SQLite free pages and container logs could accumulate                                                   | Incremental vacuum for eligible databases, passive WAL checkpoints, rotated container logs                                              |

These are verified code paths, not a measured breakdown of the deleted
production database. No SessionMesh container or state volume was available
for inspection during this session.

## What remains persistent

| Data                                                                                                        | Policy                                                              |
| ----------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------- |
| User prompts and assistant messages, including Unicode and code blocks                                      | Permanent; raw bytes and deterministic canonical identities survive |
| System/lifecycle records, reasoning/unknown kinds, decisions, tasks, graph/audit metadata                   | Retained                                                            |
| Raw records containing both text and tool content                                                           | Retained in full to protect the conversation                        |
| Separate `ToolCall`, `ToolResult`, `FileRead`, `FileWrite`, `Patch`, `ShellCommand`, `CommandResult` events | Not persisted in conversation mode; existing instances are removed  |
| Latest handoff and state snapshot                                                                           | Rebuildable derived state, latest one retained by default           |
| Old handoffs/snapshots, unreferenced blobs, stale publication temporary files                               | Collected automatically                                             |
| Native client session stores and project code repositories                                                  | Never changed by cleanup                                            |

Tool-output-only Claude records previously normalized as empty messages are
checked against their verified raw envelopes before removal. Genuine text,
reasoning, attachments, unknown data, and mixed envelopes are preserved.
Older collectors should be upgraded alongside the daemon for the corrected
classification and bounded ingestion paths.

## Configuration

| TOML field                  | Environment                             | Default | Range                    |
| --------------------------- | --------------------------------------- | ------- | ------------------------ |
| `conversation_only`         | `SESSIONMESH_CONVERSATION_ONLY`         | `true`  | Boolean                  |
| `maintenance_interval_secs` | `SESSIONMESH_MAINTENANCE_INTERVAL_SECS` | `3600`  | 60–86,400 seconds        |
| `handoffs_to_keep`          | `SESSIONMESH_HANDOFFS_TO_KEEP`          | `1`     | 1–100 per global session |

The daemon persists the effective conversation policy in its database. MCP,
remote collectors, direct event inserts and batch imports all use that shared
write boundary, so a replay does not restore discarded tool bodies. Disable
conversation mode **before upgrading** if full tool history is required;
already removed bodies cannot be recovered by changing the setting back.

Maintenance runs at startup and on its own timer, independently of ingestion.
Missed intervals are skipped instead of creating catch-up storms. Errors and
reclaimed counts appear on stderr without transcript content. Discarded tool
history invalidates affected handoffs; the next reconciliation rebuilds them
from surviving evidence, including when no new message arrives.

## Operator commands

The image now includes the `sessionmesh` administration CLI. Inspect counts and
reusable database bytes before deletion:

```bash
docker compose exec sessionmesh sessionmesh maintenance
```

Apply the configured conversation/derived-history policy immediately:

```bash
docker compose exec sessionmesh sessionmesh maintenance --apply
```

Small metadata tombstones also prevent an older parser from restoring legacy
Claude envelopes that were positively identified as disposable tool dumps.

The inspection command opens/migrates storage and applies configured policy
settings, but does not delete events or files. `--apply` does. Back up the
existing database and blob directory before the first upgrade/cleanup if tool
history must remain recoverable. Use a consistent SQLite backup, not a live
copy of the database file alone.

For a legacy database, deleted data initially leaves reusable database pages.
New databases support incremental vacuum; an older database may need one full
rebuild during a maintenance window:

```bash
SESSIONMESH_HOME=/path/to/existing/state sessionmesh maintenance --apply --vacuum
```

This requires enough disk space for SQLite's temporary copy, working database
and WAL. Stop competing writers during the maintenance window. In the
container, provide a disk-backed scratch mount and `SQLITE_TMPDIR` for large
legacy databases: the normal `/tmp` is a bounded 64 MiB tmpfs. Full `VACUUM`
is deliberately manual; it is never run on every import or periodic tick.
See SQLite's [auto-vacuum and incremental-vacuum documentation](https://www.sqlite.org/pragma.html#pragma_auto_vacuum).

Never delete live WAL/SHM files, native histories, or the state volume to
reclaim space. Cleanup does not run `docker system prune` or delete unrelated
images, volumes, repositories, or backups.

## Container budget

Compose defaults to:

- 1 GiB memory and the same combined memory/swap limit;
- one CPU and 128 process IDs;
- a read-only root filesystem, 64 MiB temporary filesystem, and the existing
  persistent SessionMesh state volume;
- JSON log rotation at 10 MiB with three files.

Set `SESSIONMESH_MEMORY_LIMIT` and `SESSIONMESH_CPUS` in the Compose environment
to change the budget. These settings follow the official
[Compose resource controls](https://docs.docker.com/reference/compose-file/services/#mem_limit).
Restart/recreate the service to apply them. Docker/Podman implementations must
enforce the configured cgroup and log settings; Podman was not available for
runtime verification here. Podman log rotation with `max-file` requires
conmon 2.2.0 or newer; verify the effective limits/log settings on the target
engine. See the official [Podman run reference](https://docs.podman.io/en/latest/markdown/podman-run.1.html#log-opt-name-value).

SQLite uses an 8 MiB page-cache target per connection and at most three
connections per storage process. These targets exclude payloads and other
SQLite allocations. Each MCP process opens its own bounded pool. Source stamps and membership
metadata still scale with the number of source files/sessions, rather than
transcript payload bytes. Arbitrarily
large individual messages and rewritten Continue snapshots can exceed ordinary
working-set estimates; their contents are never silently dropped to fit a
budget.

## Reproduce the resource check

Build daemon and CLI, then run the Linux-only synthetic soak in temporary
storage. It never opens the user's native sessions:

```bash
cargo build -p sessionmesh-daemon -p sessionmesh-cli
python3 scripts/resource-soak.py \
  --daemon target/debug/sessionmesh-daemon \
  --cli target/debug/sessionmesh \
  --duration 70
```

The harness imports hundreds of complete Unicode/code-bearing messages and a
large disposable tool dump, appends new conversation messages during repeated
API polling, keeps a slow SSE client open, runs maintenance, checks retained
counts and unchanged source bytes, and reports RSS/disk sizes without private
content. It tolerates at most 64 MiB of additional warm RSS. This short native
check is not a multi-day production/container benchmark.

## Local build-cache finding

The workspace initially contained approximately 13 GiB of generated Cargo artifacts:
7 GiB in `target/debug/deps` and 4.7 GiB in `target/debug/incremental`. These
are build caches, not persistent sessions and not files in the final runtime
image. `.dockerignore` excludes them. Use `cargo clean` in a writable host
session when those local caches are no longer needed. Cleanup in this session removed cache files totaling 12,600,235,657 bytes
(about 11.7 GiB) from
old `target/debug/deps`, `target/debug/build` and incremental caches. About
75 MB of incremental files remained inaccessible because of ownership; no
permissions were bypassed. Existing executable paths and session storage were
not removed. Current checks use a
separate target directory with debug info and incremental compilation disabled
to avoid adding more large debug-version caches.

Retention does not expand adapter coverage. The existing Agy adapter imports
prompt history; complete assistant trajectories still require a supported
export format. Native Agy data is never deleted by this cleanup policy.

## Measured synthetic result — 2026-11-20

The successful 70-second native run used Rust 1.88, debug information disabled,
two Tokio workers, temporary fixture/state directories, concurrent source
appends, repeated API polling and a slow SSE subscriber. Container cgroups were
not part of this native measurement.

| Measurement                                          | Result      |
| ---------------------------------------------------- | ----------- |
| Input source bytes                                   | 27966755    |
| SessionMesh state bytes including database/WAL/blobs | 7600177     |
| Complete conversation events retained                | 728         |
| Separate tool-result events retained                 | 0           |
| Handoffs after cleanup                               | 1           |
| Authenticated API requests                           | 1284        |
| Warm daemon RSS (KiB)                                | 63896       |
| Steady daemon RSS range (KiB)                        | 63912–63928 |

All expected prompts/replies survived, including Unicode and code blocks. The
source hash remained unchanged. These are fixture results, not evidence about
the original deleted production database or a multi-day memory-leak guarantee.
The metadata-only-session regression additionally verifies that sessions with
no usable handoff facts cannot prevent other sessions from reconciling or keep
an otherwise idle daemon rebuilding work indefinitely.

Migration 0005 rebuilds the native-event table to stabilize ingestion IDs and
adds derived indexes/projections. It can temporarily require substantial
additional database/WAL space before cleanup runs. Reserve migration space or
upgrade a consistent backup on a larger disk; do not try to make room by
removing active WAL files. Backfill reads individual canonical payloads rather
than collecting 128 large legacy tool dumps into memory, and skips rebuilding
their FTS content when conversation retention is enabled.

## MCP startup while an initial import is running

The live deployment exposed a SQLite startup contention path: every MCP process
reissued journal/auto-vacuum settings and migration-table DDL when opening an
already current database. During a long import transaction, that could close
stdio with `database is locked` before protocol negotiation.

An ordinary `Storage::open` now verifies applied migration versions/checksums
and projection readiness with WAL reads. A current database inherits its journal
mode and opens without migration or journal-setting writes. New, incomplete or
outdated databases still use the migration path; schema validation is not
skipped. Explicit policy/configuration opens retain their controlled write
semantics. A regression test holds an immediate writer transaction and requires
a second current-schema reader to open within 500 ms; the old code timed out.

The scanner and reconciler also run as independent futures. Initial discovery
and ingestion can therefore continue while already imported sessions receive
handoffs, without waiting for a complete archive scan to return. No busy-timeout
increase or startup retry loop was added.

Long real-world prompts also exposed a mandatory-layer budget failure: the full
latest prompt was copied into both a task and `next_action`, so even dropping
optional fields could not fit the default handoff budget. Derived decisions,
completion/task prose and next actions now use explicit UTF-8-safe excerpts of
at most 2,048 bytes with an ellipsis. Original canonical messages, raw evidence,
and typed observation content are unchanged and remain available through their
provenance. The regression covers long Unicode prompts and recorded tasks.
