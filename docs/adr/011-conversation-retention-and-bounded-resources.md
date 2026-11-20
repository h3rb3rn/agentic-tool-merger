# ADR-011: Conversation Retention and Bounded Resources

- **Status:** Accepted
- **Date:** 2026-11-20
- **Authorization:** User requested permanent prompts and AI replies, disposal of redundant code versions, cleanup jobs, and bounded container resources.

## Context

ADR-010 reduced repeated reconciliation but left several independent growth
paths: whole-transcript API/SSE materialization, unlimited correlation terms,
whole-file imports, permanent handoff/snapshot history, redundant full-JSON FTS
content, and unrestricted container logs/resources. The original raw-store
policy retained every tool dump even when only conversation history was wanted.

## Decision

Default the daemon to conversation-oriented retention. Preserve user and
assistant messages, including code inside those messages, and their byte-exact
raw evidence. Also preserve system, lifecycle, reasoning/unknown records,
structured decisions/tasks, membership, and audit evidence. Never edit native
sources. Retained canonical identities and raw content remain immutable.

Separate tool calls/results, file reads/writes, patches, shell commands, and
command results are disposable in this mode. Reject their persistence at the
shared database write boundary and remove existing instances during maintenance.
A raw object shared with a protected event remains intact. Small metadata
tombstones prevent legacy parser replays from restoring positively identified
Claude tool-only envelopes. Legacy Claude
empty-message records are disposable only when their verified raw content
contains exclusively structured tool-use/result blocks. Text, reasoning, and
attachments are protected.

Keep the latest derived handoff/snapshot per global session by default. Cleanup
invalidates contexts referencing discarded tool history and advances a revision
so the daemon rebuilds them without needing a new message. Typed user decisions
and tasks are not deleted when they leave the active context window.

Bound API/SSE pages, ingestion batches, correlation vocabulary/cache, and active
handoff inputs. Preserve native order and exact chronology in indexed metadata.
Use a persistent SQLite policy shared by every writer, serialized blob
publication/collection, a durable deletion queue, and age-protected orphan
collection. Run cleanup at startup and periodically. Do not periodically perform
full database rewrites or global Docker pruning.

## Compatibility and migration

This supersedes the original plan's retain-every-tool-record policy and the
unbounded snapshot-history consequence of ADR-010. It does not permit rewriting
raw evidence or replacing conversation history with summaries.

Schema migration 0005 preserves all existing canonical/raw data. It adds
persistent policy and exact-order projections, rebuilds the derived FTS index
using content fields, and gives native events a stable integer ingestion key.
Existing event IDs and source offsets do not change. The bounded projection
backfill completes before accepting traffic and resumes after interruption.

The upgraded daemon then enables its configured policy; the first maintenance
pass removes eligible tool history. Back up the existing database and blobs
before upgrading if those tool bodies must remain recoverable. Setting
`conversation_only=false` before startup retains full tool history, but cannot
restore bodies already removed. This intentionally changes timeline/detail and
search availability for discarded tool events.

## Consequences

Conversation storage continues growing with real prompts and AI replies. A fixed
upper disk limit would contradict permanent, complete message retention. Only
derived state, disposable tool bodies, orphan files and logs are cleaned.
Unknown or mixed native records can still contain tool/code data; preserving
messages takes precedence over aggressive deletion.

Memory depends on bounded windows, source/membership metadata, and the largest
individual native record.
Individual messages are never silently truncated to satisfy a batch size.
Continue's rewritten JSON snapshots still cost one snapshot in memory; the
currently requested Codex, Claude, Agy and OpenCode paths are paged/batched.
Container memory limits are enforceable guardrails, not a proof of a particular
long-running production RSS. Performance claims require representative soak
measurements.
