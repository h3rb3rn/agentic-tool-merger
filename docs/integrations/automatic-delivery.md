# Automatic context delivery

SessionMesh reads native stores without modifying them and delivers derived
shared state over MCP. Start with [First steps](../getting-started/first-steps.md)
for service setup and cross-tool continuation.

## Client setup

Run as the user who runs the clients. Preview the exact registration commands
without launching clients or modifying files:

```bash
scripts/install-sessionmesh-connectors --dry-run codex claude agy opencode grok
scripts/install-sessionmesh-connectors codex claude agy opencode grok
```

Without client arguments, the installer selects installed clients from those
five. Only selected clients must be installed. OpenCode's standard
`~/.opencode/bin/opencode` installation is also recognized outside `PATH`.
Continue is optional: select `continue` explicitly; it requires an existing
`~/.continue/config.yaml` and the jq-style Python `yq` command.

| Client       | Registration                                          | Handoff at startup                                                       |
| ------------ | ----------------------------------------------------- | ------------------------------------------------------------------------ |
| Codex        | `codex mcp add sessionmesh -- <bridge>`               | Append instruction to `$CODEX_HOME/AGENTS.md`, preserving existing rules |
| Claude Code  | `claude mcp add --scope user sessionmesh -- <bridge>` | Upsert `SessionStart` hook, preserving other hooks                       |
| Agy          | `agy mcp add sessionmesh <bridge>`                    | Ask for `sessionmesh_get_handoff` in the first prompt                    |
| OpenCode v2  | `opencode mcp add --global sessionmesh -- <bridge>`   | Ask for `sessionmesh_get_handoff` in the first prompt                    |
| Grok Builder | `grok mcp add --scope user sessionmesh -- <bridge>`   | Ask for `sessionmesh_get_handoff` in the first prompt                    |
| Continue     | YAML `mcpServers` and `rules`                         | Persistent startup rule                                                  |

OpenCode v2 uses `mcp.servers`, whereas older versions used a different
configuration shape. The installer uses the installed CLI and verifies support
for `--global` before making changes. See the official
[OpenCode MCP documentation](https://opencode.ai/docs/mcp-servers/).
Agy and Grok command syntax was also checked against the installed CLI help;
Grok documents its integration in `~/.grok/docs/user-guide/07-mcp-servers.md`.

Configuration backups use the suffix `.sessionmesh-backup` and user-only
permissions. The original backup is retained across repeated installation.
Configuration registration is delegated to client CLIs; native transcripts are
never rewritten. Restart clients after installation. Project or managed
configuration can override a user registration; check the effective server list
from the intended project directory.

MCP availability is distinct from import support. Grok Build now also imports
its authoritative ACP conversation logs; see [Grok Build import](#grok-build-import).
Agy ingestion still covers prompt history rather than complete assistant trajectories.

## Grok Build import

The daemon and remote collector discover
`.grok/sessions/<encoded-cwd>/<session-id>/updates.jsonl` under their configured
profile root. The container's existing read-only `/sources/home` mount covers
this location. The host-facing original path is retained in provenance. A
nonstandard Grok home must be mapped to `.grok` beneath that profile root.

The adapter was verified against Grok Build 1.0.89 and its installed session
format documentation. `updates.jsonl` is the authoritative ACP update log;
`chat_history.jsonl`, summary files, compaction checkpoints, generated exports,
artifacts and tool dumps are not imported as additional transcripts. Discovery
uses exactly two directory levels and does not follow session-directory or log
symlinks. Long encoded paths use Grok's `.cwd` sidecar; other group names are
URL-decoded, preserving literal plus signs and Unicode.

Native `user_message_chunk`, `agent_message_chunk` and `agent_thought_chunk`
updates become user, assistant and unknown/reasoning events respectively.
Each chunk is retained individually, including its original content object,
metadata and byte-exact JSONL evidence. Reasoning remains data. Text, code blocks
and attachments are not shortened or replaced by summaries. Other operational
updates, including tool invocations/results, are outside this conversation-only
adapter surface, even when full tool retention is enabled elsewhere.
Explicit SessionMesh-origin updates marked `ingestion_excluded` are excluded.

Native session IDs are namespaced as `grok:<id>`. Native microsecond timestamps
are preferred, followed by agent milliseconds and older second timestamps;
line order, offsets and deterministic IDs retain tie-break evidence. Missing
or invalid native conversation time, mismatched session identity and malformed
complete JSON records fail explicitly without advancing the cursor. Partial
trailing lines wait for completion. Appends use the shared 4 MiB newline batch
target, allowing one larger record intact. Persisted cursors support restart;
truncation/first-line replacement uses the existing idempotent replay path.

Grok sessions participate in workspace correlation, search, handoffs and the
Web UI under tool family `grok`. Every imported tool stays visible in the left
navigation. Click its heading to load its sessions directly; **Next sessions** /
**Previous sessions** page through that tool’s archive, and text filters apply
to the current session page. Import does not run Grok, invoke models, alter client settings or
write native stores. Existing MCP registration continues to provide outbound
context delivery.

## Transport and permissions

The bridge preserves the calling client's working directory as correlation
evidence. By default it runs `sessionmesh-mcp` inside
`agenttool-merger-sessionmesh-1`. It does not create or start containers.

Configure a different existing container in the environment inherited by the
client:

```bash
export SESSIONMESH_CONTAINER=my-sessionmesh-container
```

For a native installation, point to a built MCP binary and the **same state
home as the daemon**:

```bash
export SESSIONMESH_MCP_BINARY=/absolute/path/to/sessionmesh-mcp
export SESSIONMESH_HOME=/absolute/path/to/sessionmesh-state
```

These settings must remain available when launching each client. Do not point
the MCP process at an empty state home to repair missing context. Restore or
locate the existing state and service first.

Writes now default to disabled. A trusted host can explicitly enable typed
decision and task recording with `SESSIONMESH_MCP_ALLOW_WRITES=true` in the
client's inherited environment. Reads do not require enabling writes. This
replaces the earlier bridge behavior that unconditionally enabled writes.

## Verify the entire path

From the intended project directory:

```bash
/path/to/sessionmesh/scripts/check-sessionmesh-mcp
```

The check negotiates MCP `2025-06-18`, sends the initialized notification,
validates all seven tools, and requests the current handoff. It hides handoff
content and fails on a missing transport, protocol failure, missing tools, or
unavailable context. A failed handoff does not necessarily mean a failed
connection: the workspace may have no imported and linked global session.
The check does not prove that every client's active session has loaded the
registration; inspect its effective MCP list too.

The Claude startup hook uses the same check with `--handoff`. That option prints
private historical context and preserves its delivery origin and ingestion
exclusion metadata. Do not publish the output.

## Trust boundary

Handoffs are observations, not system instructions. Verify current repository,
Git, and test facts locally. Tool outputs cannot promote themselves into
instructions. MCP writes remain typed observations scoped to the current
global session.

## Local audit: 2026-11-19

Read-only inspection of the requested `/home/philipp` installation found:

| Client          | Verified user configuration                      | Result                                                                                                                                                         |
| --------------- | ------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Codex           | `.codex/config.toml`                             | SessionMesh bridge registered                                                                                                                                  |
| Claude Code     | `.claude.json` and `.claude/settings.json`       | Bridge registered; startup hook present                                                                                                                        |
| Agy             | `.gemini/config/mcp_config.json`; `agy mcp list` | Bridge registered and enabled                                                                                                                                  |
| OpenCode v2.2.7 | `.config/opencode/opencode.json`                 | No MCP registration; binary exists under `.opencode/bin`, outside this session's PATH                                                                          |
| Grok Builder    | `.grok/config.toml`                              | No native MCP entry; Grok documents compatibility discovery from Claude configuration, so absence here alone does not prove absence from the effective catalog |

All three existing registrations point to this repository's bridge. Its
configured default container is absent: Docker returned `No such container:
agenttool-merger-sessionmesh-1`. Neither a SessionMesh-named container nor a
SessionMesh-named Docker volume was present. The conventional native state
home was absent too. Other names or external backups were not ruled out.
No native SessionMesh process was visible, and the configured health endpoint
at `192.168.155.225:8787` refused connections. No successful live handoff could
be established.

Client configurations and Git metadata are read-only in this coding session.
Installation was previewed but not applied to the user home. The existing
SessionMesh state/service must be located or restored, then the installer and
connection check must run in a host session with write access. This audit does
not claim those operational steps are complete.
