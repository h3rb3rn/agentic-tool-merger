# FAQ: sessions and agent handoffs

The Web UI provides the same help through **FAQ** in its header and login screen.
Open `/#faq` directly or use an agent anchor such as `/#faq-agent-opencode`.
The page is static and accessible without authentication. Opening a direct FAQ
link does not load the session archive, even with a saved login. Returning to
an already opened timeline preserves its selected session and navigation state.

## Native resume and shared context

Native history belongs to its client. Cross-client continuation starts a new
native session with a SessionMesh handoff; native transcripts remain unchanged.
Run commands on the host from the same project checkout or worktree:

```bash
cd /path/to/project
```

Replace placeholders before executing commands. Obtain native IDs from the
original client's history. SessionMesh global IDs (`gs_…`) and imported IDs with
SessionMesh namespace prefixes are not native resume arguments. Use the native
picker or a known ID when the latest session might belong to a different task.

## Commands for each client

| Client       | Native picker or list                 | Latest native session | Known native ID                             | Inspect MCP setup                                                |
| ------------ | ------------------------------------- | --------------------- | ------------------------------------------- | ---------------------------------------------------------------- |
| Codex        | `codex resume`                        | `codex resume --last` | `codex resume NATIVE_SESSION_ID`            | `codex mcp get sessionmesh`                                      |
| Claude Code  | `claude --resume`                     | `claude --continue`   | `claude --resume NATIVE_SESSION_ID`         | `claude mcp get sessionmesh`                                     |
| Agy          | Use Agy's native conversation history | `agy --continue`      | `agy --conversation NATIVE_CONVERSATION_ID` | `agy mcp list`                                                   |
| OpenCode v2  | `opencode session list`               | `opencode --continue` | `opencode --session NATIVE_SESSION_ID`      | `opencode mcp list`                                              |
| Grok Build   | `grok sessions list`                  | `grok --continue`     | `grok --resume NATIVE_SESSION_ID`           | `grok mcp doctor sessionmesh`                                    |
| Continue CLI | `cn ls`                               | `cn --resume`         | Choose through `cn ls`                      | Shared transport check below, then confirm tools inside Continue |

Additional details:

- `codex resume --all` includes other working directories in its picker.
- OpenCode's `--session` can create a new session for an unknown ID; use an
  existing ID from its native list. Its binary may be `~/.opencode/bin/opencode`.
- Grok accepts a saved session title with `--resume` too.
- The verified Continue `--resume` is a boolean flag, without an ID argument.
  Continue IDE uses its own history UI and MCP configuration; CLI/IDE histories
  can differ. Claude's CLI/editor/desktop surfaces can also have separate histories.
- SessionMesh currently imports Agy prompt history, not full assistant
  trajectories. Inspect Agy itself for any response missing from shared context.

The commands were checked against installed CLI help on 2026-11-20: Codex
0.170.0, Claude Code 2.1.328, Agy 1.5.13, OpenCode 2.2.7, Grok Build 1.0.89 and
Continue CLI 1.5.43. Check native help after upgrading.

## Continue another client's work after a rate limit

1. Keep the same checkout/worktree and note unfinished tasks, branch, pending
   tests and uncommitted changes.
2. Confirm the source conversation is imported. Review uncertain links and
   verify that memberships refer to the intended task.
3. Choose a client/provider/model with available quota and start a new session
   in the same project using one of the commands below.
4. Inspect current membership and the shared handoff, then verify repository
   facts before editing.
5. To return later, resume the original native session and retrieve the latest
   handoff to account for changes made in the other client.

The source agent does not need to answer another prompt once its conversation
has already been imported. Provider quotas and account access remain controlled
by each provider.

Each command below runs independently and requires the destination's MCP
connector. Run only the command for your chosen client:

### Codex

```bash
codex 'Call sessionmesh_get_current and sessionmesh_get_handoff. Verify the source sessions, workspace, repository state and open tasks, then continue the next open task.'
```

### Claude Code

```bash
claude 'Call sessionmesh_get_current and sessionmesh_get_handoff. Verify the source sessions, workspace, repository state and open tasks, then continue the next open task.'
```

### Agy

```bash
agy --prompt-interactive 'Call sessionmesh_get_current and sessionmesh_get_handoff. Verify the source sessions, workspace, repository state and open tasks, then continue the next open task.'
```

### OpenCode

```bash
opencode --prompt 'Call sessionmesh_get_current and sessionmesh_get_handoff. Verify the source sessions, workspace, repository state and open tasks, then continue the next open task.'
```

### Grok Build

```bash
grok 'Call sessionmesh_get_current and sessionmesh_get_handoff. Verify the source sessions, workspace, repository state and open tasks, then continue the next open task.'
```

### Continue CLI

```bash
cn 'Call sessionmesh_get_current and sessionmesh_get_handoff. Verify the source sessions, workspace, repository state and open tasks, then continue the next open task.'
```

For example, continue in Claude after Codex reaches a limit. Later return with
`codex resume --last` (or the exact native ID) and request an updated shared
handoff before working in that older conversation.

## Request context inside an agent

These are agent prompts, not terminal commands:

```text
Call sessionmesh_get_current and sessionmesh_get_handoff. Verify the source
sessions, workspace, repository state and open tasks before continuing.
```

```text
Call sessionmesh_search with {"query":"unique task keywords","limit":20}.
Identify the relevant native_session_id and source provenance, and check that
they match this project and task.
```

MCP scope comes from the client working directory and SessionMesh project
context. The Web UI agent selector browses sessions; verify actual current
memberships with `sessionmesh_get_current`, especially for multiple tasks in
one repository. Search returns event references/provenance. Read matching
conversation text through **Search session content** / **Show timeline content**
in the Web UI or an authorized source read. The search schema has `query`,
`limit`, and `cursor`; it does not accept an agent/session selection parameter.
Read tools use default connector permissions. Recording tasks or decisions
requires separately authorized MCP writes.

## Connection checks and manual delivery

From the project directory, check negotiation, the five tools and workspace
handoff without printing private context:

```bash
/opt/deployment/agenttool-merger/scripts/check-sessionmesh-mcp
```

For a manual handoff, retrieve historical context and provide it to the
receiving agent for verification. This command prints private project context:

```bash
/opt/deployment/agenttool-merger/scripts/sessionmesh-session-start
```

Restart an affected CLI after a container restart to replace an old closed MCP
stdio process. If context is absent, check import progress, supported source
formats, parse errors, the project marker and global-session memberships.
A compact handoff is a bounded window; original imported conversation records
remain available separately.

See [connector setup](../integrations/automatic-delivery.md) for registration.
The Web FAQ includes the existing installer commands; it never executes them.
Copy uses the clipboard when permitted, or selects the command and explicitly
instructs manual copy on HTTP origins and denied clipboard access.
