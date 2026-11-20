# MCP integration

SessionMesh implements the MCP `2025-06-18` stdio lifecycle, resources, and
tools using newline-delimited JSON-RPC 2.0. Stdio is deliberate: the MCP host
launches the local process, while remote HTTP authorization remains outside the
MVP.

The protocol structure follows the official
[MCP base protocol](https://modelcontextprotocol.io/specification/2025-06-18/basic/index),
[resource model](https://modelcontextprotocol.io/specification/2025-06-18/server/resources),
and [tool model](https://modelcontextprotocol.io/specification/2025-06-18/server/tools).

```mermaid
sequenceDiagram
    participant Agent as Coding agent
    participant MCP as sessionmesh-mcp
    participant Store as Local SQLite
    Agent->>MCP: initialize
    MCP-->>Agent: resources + tools capabilities
    Agent->>MCP: resources/read current handoff
    MCP->>Store: Latest immutable snapshot
    Store-->>MCP: Compact handoff
    MCP-->>Agent: Context + delivery metadata
    Agent->>MCP: tools/call search
    MCP-->>Agent: Paginated provenance summaries
```

## Resources

| URI                             | Content                         |
| ------------------------------- | ------------------------------- |
| `sessionmesh://current`         | Current global context and refs |
| `sessionmesh://handoff/current` | Compact latest handoff          |

Handoff delivery metadata contains origin, schema version, snapshot ID,
generation time, staleness, and `ingestion_excluded: true`. Consumers can
therefore avoid importing SessionMesh-derived content as a native source.

## Tools

| Tool                          | Effect                                 |
| ----------------------------- | -------------------------------------- |
| `sessionmesh_get_current`     | Read current context                   |
| `sessionmesh_get_handoff`     | Read compact handoff                   |
| `sessionmesh_search`          | Fetch event provenance on demand       |
| `sessionmesh_list_sessions`   | List native sessions, newest first     |
| `sessionmesh_get_session`     | Read one session's redacted messages   |
| `sessionmesh_record_decision` | Record typed user-observation decision |
| `sessionmesh_record_task`     | Record typed user-observation task     |

Handoff schema 1.1 reports `latest_user_request` and `latest_assistant_reply`
as unverified observations. They are not tasks or completed work; only
`open_tasks` and `completed` carry explicit task state (ADR-012).

`sessionmesh_list_sessions` returns metadata only (tool, surface, timestamps,
last activity, event count, redacted title) with an optional `tool_family` filter and opaque
cursor paging. `sessionmesh_get_session` returns the newest redacted user and
assistant messages of one native session (at most 50, each at most 4 KiB,
chronological, with `truncated`) and marks the result `untrusted` and
`ingestion_excluded`. Both are read-only and need no write authorization. They
let a client continue a session that another tool created without copying or
modifying its native transcript.

Every input schema rejects additional properties. Search is limited to 100
items and uses opaque `v1:search:` cursors. Duplicate write `request_id`
values are idempotent; reusing an ID with different data fails.

Recorded content is classified as `user_observation` with
`instruction: false`. Prompt-like or injection-like text remains untrusted
data and never enters a system-instruction channel.

## Write authorization

Reads are enabled for a process launched in the user's local context. Writes
are denied unless the trusted MCP host explicitly sets:

```text
SESSIONMESH_MCP_ALLOW_WRITES=true
```

This is process-scoped stdio authorization, consistent with the MCP rule that
stdio credentials come from the environment rather than the HTTP OAuth flow.
All records retain origin and an idempotency key.

## Codex installation

Build the binary:

```bash
cargo build --release --package sessionmesh-mcp
```

Add a narrowly scoped server to the Codex MCP configuration. Substitute
absolute paths and enable writes only when wanted:

```toml
[mcp_servers.sessionmesh]
command = "/absolute/path/to/sessionmesh-mcp"

[mcp_servers.sessionmesh.env]
SESSIONMESH_HOME = "/home/user/.local/share/sessionmesh"
SESSIONMESH_PROJECT_ROOT = "/absolute/path/to/project"
SESSIONMESH_MCP_ALLOW_WRITES = "false"
```

SessionMesh does not rewrite unrelated Codex settings. The project marker
provides automatic current-context discovery at the beginning of a fresh
session; the agent initially reads the compact resource and loads details only
through search.

## Multi-client bridge

The [connector installer](../integrations/automatic-delivery.md) supports Codex,
Claude Code, Agy, OpenCode v2, and Grok Builder, plus optional Continue.
The shared bridge accepts `SESSIONMESH_CONTAINER` for container deployment or
`SESSIONMESH_MCP_BINARY` with `SESSIONMESH_HOME` for native execution.
It preserves the client workspace and defaults `SESSIONMESH_MCP_ALLOW_WRITES`
to `false`; set it explicitly in the trusted host environment to allow writes.
Run `scripts/check-sessionmesh-mcp` from the project to verify negotiation,
the tool catalog, and handoff delivery without printing private context.
