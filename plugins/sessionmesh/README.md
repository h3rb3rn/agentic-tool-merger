# SessionMesh plugin for Claude Code

Continue the work of other coding agents inside a Claude Code session.

| Command                               | Purpose                                            |
| ------------------------------------- | -------------------------------------------------- |
| `/sessionmesh:sessions [tool]`        | List imported sessions of all tools, newest first  |
| `/sessionmesh:continue [id\|tool\|q]` | Load one session as untrusted context and continue |

Install from a local checkout of SessionMesh:

```text
/plugin marketplace add /path/to/sessionmesh
/plugin install sessionmesh@sessionmesh
```

The plugin bundles the SessionMesh MCP bridge (`bin/sessionmesh-mcp-stdio`).
Everything is read-only: native session stores are never modified, message text
is redacted and bounded, and results are marked as untrusted historical context.

See `docs/integrations/claude-code-plugin.md` in the SessionMesh repository for
installation, configuration, and security notes.
