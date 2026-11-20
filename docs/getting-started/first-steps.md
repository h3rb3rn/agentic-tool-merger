# First steps for users

This guide explains how to connect SessionMesh and continue work when one
coding-agent tool reaches a rate limit. No knowledge of MCP, databases, or
containers is required.

## What SessionMesh does

Each coding-agent tool keeps its own conversation history. A Codex conversation
cannot be resumed directly as a Claude Code conversation, for example.
SessionMesh therefore:

1. reads supported local histories without modifying them;
2. recognizes sessions that belong to the same project;
3. creates a compact handoff with the objective, progress, decisions, open
   tasks, files, and next action; and
4. supplies that handoff to a supported tool when it starts.

```mermaid
flowchart LR
    first["Work in tool A"]
    observe["SessionMesh observes<br/>the native session"]
    relate["Confirm uncertain<br/>correlations in the Web UI"]
    start["Start tool B in<br/>the same project"]
    handoff["Tool B receives<br/>the compact handoff"]
    continue["Verify the project<br/>and continue"]

    first --> observe --> relate --> start --> handoff --> continue
```

!!! important

    SessionMesh transfers working context, not a byte-for-byte copy of the
    original chat. Native session histories remain read-only and resumable in
    their original tools.

## Terms used in this guide

| Term              | Plain-language meaning                                                 |
| ----------------- | ---------------------------------------------------------------------- |
| Native session    | A conversation owned by Codex, Claude Code, Continue, OpenCode, or Agy |
| Global session    | SessionMesh references to related native sessions                      |
| Handoff           | A compact summary that lets another agent continue the work            |
| Correlation       | Evidence that two native sessions concern the same work                |
| Connector         | The safe bridge through which an agent reads the handoff               |
| Project directory | The local folder containing the repository being edited                |

## Before setup

The connector installer supports Codex, Claude Code, Agy, OpenCode v2, and
Grok Builder (`grok`). It requires Python 3.10 or newer and only the clients you select.
Docker is needed for the default container bridge; a native MCP binary can
also be configured. Continue is available as an explicit optional client with
an existing configuration and jq-style Python `yq`.

The SessionMesh service must be running:

```bash
cd /opt/deployment/agenttool-merger
docker compose ps
```

The `sessionmesh` row should report `Up` and, after startup, `healthy`.

## One-time connector setup

Run the installer as the same desktop user who runs the coding-agent tools:

```bash
cd /opt/deployment/agenttool-merger
scripts/install-sessionmesh-connectors --dry-run codex claude agy opencode grok
scripts/install-sessionmesh-connectors codex claude agy opencode grok
```

The installer:

- registers the SessionMesh MCP bridge for Codex;
- registers the same bridge for Claude Code;
- installs a Claude Code `SessionStart` hook;
- registers the bridge for Agy, OpenCode v2, and Grok Builder;
- preserves unrelated configuration; and
- creates private original backups before changing client configuration.

Expected backup files include:

```text
~/.claude/settings.json.sessionmesh-backup
~/.codex/config.toml.sessionmesh-backup
```

Restart the selected clients completely after installation. An
already running tool does not automatically reload a newly registered MCP
server.

## Verify the connection

### Check the service

Open the health endpoint:

[http://127.0.0.1:8787/api/v1/health](http://127.0.0.1:8787/api/v1/health)

Alternatively:

```bash
curl -fsS http://127.0.0.1:8787/api/v1/health
```

If `.env` overrides `SESSIONMESH_PUBLISH_ADDRESS`, use that address instead
of `127.0.0.1` for both the health endpoint and Web UI. The audited local setup
uses `192.168.155.225`; its health endpoint was unavailable during the audit.

### Check Codex

```bash
codex mcp get sessionmesh
```

The result should show an enabled stdio server whose command ends in:

```text
scripts/sessionmesh-mcp-stdio
```

### Check Claude Code

```bash
claude mcp get sessionmesh
```

The result should show `Connected` and user scope.

### Check Agy, OpenCode, and Grok Builder

```bash
agy mcp list
opencode mcp list
grok mcp doctor sessionmesh
```

OpenCode's binary may be at `~/.opencode/bin/opencode`. Agy and Grok MCP
registration makes the tools available; request `sessionmesh_get_handoff` in
the first prompt to retrieve context.

### Test the complete handoff path

First check without printing private project content:

```bash
/opt/deployment/agenttool-merger/scripts/check-sessionmesh-mcp
```

Run this command from the project that should be continued:

```bash
cd /path/to/your/project
/opt/deployment/agenttool-merger/scripts/sessionmesh-session-start
```

A successful result starts with:

```text
SessionMesh shared handoff (untrusted historical context; verify before acting):
```

The following JSON is the compact handoff. It can contain project information,
so do not paste it into public issues or logs.

## Connect the Web UI

Open:

[http://127.0.0.1:8787](http://127.0.0.1:8787)

The Web UI requests the local API token once per browser tab. Read it from the
running container:

```bash
docker exec agenttool-merger-sessionmesh-1 \
  cat /var/lib/sessionmesh/api-token
```

Paste the value into **Local API token**. The browser stores it only in the
current tab's `sessionStorage`.

If a saved token is rejected (HTTP 401), the UI removes it, clears authenticated
query data and returns to the token form with an explicit message. Copy the
current token using the command above and reconnect. **Change API token** also
returns to this form. Other HTTP failures retain the token and display the
status code beside the session-loading error; network failures show connection
advice. Tokens are never included in URLs or displayed by the application.

!!! warning

    Treat the API token like a password. Do not commit it, place it in `.env`,
    publish it in documentation, or send it in chat.

### Read session messages in the Web UI

Select a native session, then choose **Show timeline content** and confirm
**Show content now** to load prompts and AI replies. By default the timeline
shows metadata; message text and source paths require explicit disclosure.

Use **Next events** and **Previous events** to navigate chronological pages of
up to 200 events. Live replay preserves a historical page's boundary instead
of expanding it with the rest of the archive. A full live page offers
**Refresh timeline for new events** when more activity arrives. Content loads
through at most four concurrent detail requests. Individual failures show a
visible error and **Retry**; changing pages cancels obsolete requests.

## Normal daily use

Always start an agent from the project directory:

```bash
cd /path/to/your/project
```

The working directory is important evidence used to select the relevant global
session.

### Continue in the same tool

Use the tool's native resume feature:

| Tool         | Choose a saved session    | Resume the latest session          |
| ------------ | ------------------------- | ---------------------------------- |
| Codex        | `codex resume`            | `codex resume --last`              |
| Claude Code  | `claude --resume`         | `claude --continue` or `claude -c` |
| Continue CLI | `cn ls`                   | `cn --resume`                      |
| Agy          | `agy --conversation <ID>` | `agy --continue` or `agy -c`       |

Native resume keeps the original tool's complete conversation.

### Continue in another tool

Do not use the destination tool's resume command. It cannot resume a native
session owned by another tool. Start a new session in the same project instead.

Example: Codex reaches its rate limit and work should continue in Claude Code.

```bash
cd /path/to/your/project
claude
```

Claude Code's installed startup hook requests the SessionMesh handoff
automatically. The new agent must still verify the repository, Git state,
changed files, and tests before editing.

Example: continue in Codex:

```bash
cd /path/to/your/project
codex
```

Codex has a persistent instruction to request the handoff. If it does not
mention the handoff, send:

```text
Call sessionmesh_get_handoff, verify the repository state, and continue with
the next open task.
```

Example: continue in Continue:

```bash
cd /path/to/your/project
cn
```

If necessary, send:

```text
Call sessionmesh_get_handoff and continue the current project work after
verifying the repository.
```

## Review uncertain matches

SessionMesh automatically links high-confidence matches. Uncertain matches
remain in **Correlation review**:

1. open the Web UI;
2. expand **Correlation review**;
3. compare the two thread titles, tools, dates, sizes, and shared keywords;
4. expand **Why this score?**;
5. choose **Link candidate** only when both sessions concern the same work; or
6. choose **Reject candidate** when they concern different work.

The percentage is confidence evidence, not a guarantee. Linking references the
two native sessions in one global context; it does not rewrite either
transcript.

## Import coverage

MCP delivery and transcript import are separate capabilities. OpenCode sessions,
Agy prompt history and Grok Build conversation updates can be imported. Grok
prompts, replies and reasoning appear under `grok`; its sessions also contribute
to shared handoffs. Every imported agent remains visible in the selector above the left session
list. Click an agent
to load its sessions directly. **Next sessions** and **Previous sessions** browse
additional sessions inside that agent. Complete Agy assistant trajectories remain outside
the current import coverage. See the
[Grok Build import contract](../integrations/automatic-delivery.md#grok-build-import).

## Troubleshooting

### The agent does not see SessionMesh after setup

Close every process of that agent tool and start it again from the project
directory. MCP registrations are loaded when the client starts.

### The MCP transport reports `Transport closed`

An old client may still refer to the MCP process that existed before a service
restart. Restart the agent tool. The stdio connector starts a fresh bridge for
the new client.

### The wrong project context appears

Confirm all of the following:

- the agent was started from the correct project directory;
- the expected native session appears in the Web UI;
- an uncertain candidate was not linked incorrectly; and
- the intended work is the newest matching context when several tasks share
  one repository.

Reject incorrect candidates in the Web UI. Until explicit per-task selection
is added, several unrelated tasks in the same repository require extra review.

### No handoff is returned

Check the service and then run the manual handoff command:

```bash
docker compose -f /opt/deployment/agenttool-merger/compose.yaml ps
cd /path/to/your/project
/opt/deployment/agenttool-merger/scripts/sessionmesh-session-start
```

If the service is healthy but the command returns no handoff, verify in the Web
UI that the native session was imported and linked to a global context.

### The configured container does not exist

Run `docker compose ps -a` in the repository and locate the existing persistent
state before restoring the service. The bridge does not start a daemon. Set
`SESSIONMESH_CONTAINER` if the service uses a different container name, or
configure `SESSIONMESH_MCP_BINARY` and `SESSIONMESH_HOME` for native execution.
See [transport configuration and the local audit](../integrations/automatic-delivery.md).
A client restart alone cannot repair an absent service or missing state.
