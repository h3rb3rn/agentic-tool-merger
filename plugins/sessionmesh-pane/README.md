# SessionMesh pane for Claude Code

A pane beside the chat that lists the newest SessionMesh sessions of every
imported coding agent (Codex, Claude Code, OpenCode, Grok Build, Agy, Continue).
Filter buttons narrow the list to one tool. Pressing a session puts
`/sessionmesh:continue <id>` into the prompt box without sending it, so you stay
in control of what is loaded.

It needs the `sessionmesh` plugin from this repository, because it reads the
list through that plugin's MCP server (`sessionmesh_list_sessions`, read-only).
It refreshes at session start, after each turn, and on the Refresh button. Open
it again with `/sessionmesh-pane`. When loading fails, the pane footer shows the
error instead of an empty list.

Install from a local checkout of SessionMesh:

```text
/plugin marketplace add /path/to/sessionmesh
/plugin install sessionmesh-pane@sessionmesh
```

The pane is a hooks module, so Claude Code must allow plugin hooks modules. The
engine writes the editor typings into `.claude-plugin/types/` when it loads the
mod; they are generated and not part of the repository.
