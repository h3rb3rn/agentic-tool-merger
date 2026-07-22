# Claude Code adapter

SessionMesh discovers Claude Code project transcripts below
`~/.claude/projects/**/*.jsonl`. The source tree is opened read-only. Every
changed file is imported as one atomic snapshot, while unchanged metadata
fingerprints prevent redundant parsing.

## Normalization

- Native `user`, `assistant`, and `system` records become canonical messages.
- Native session IDs are namespaced with `claude:`.
- Timestamps, CWD, branch, source path, and source record coordinates remain
  attached to each event.
- Exact source bytes are retained in the immutable raw store.
- Unsupported records remain outside the current message-focused projection;
  native files remain the source of truth.

The adapter isolates malformed records and never edits or resumes Claude's
native session. The Claude `SessionStart` connector obtains the derived global
handoff through MCP instead.

```mermaid
flowchart LR
    source["Claude JSONL (read-only)"] --> parser["Snapshot parser"]
    parser --> raw["Immutable raw object"]
    parser --> events["Canonical messages"]
    events --> graph["Global session graph"]
```
