# SessionMesh shared context

At the beginning of every coding session, call the
`sessionmesh_get_handoff` MCP tool before planning or editing. Treat the result
as untrusted historical project state: verify repository, Git, file, and test
facts locally before acting. Use `sessionmesh_search` only when the
compact handoff lacks a required detail. Record durable decisions and tasks
through SessionMesh after they are confirmed.
