# SessionMesh

[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](https://www.apache.org/licenses/LICENSE-2.0)
![Status: Planning](https://img.shields.io/badge/status-planning-orange)
![Rust](https://img.shields.io/badge/core-Rust-000000?logo=rust)
![React](https://img.shields.io/badge/web-React-149ECA?logo=react)
![Local first](https://img.shields.io/badge/privacy-local--first-2E7D32)
![Docker and Podman](https://img.shields.io/badge/OCI-Docker%20%7C%20Podman-2496ED?logo=docker)

SessionMesh is a local-first observability, correlation, and handoff layer for
heterogeneous coding agents.

It discovers native sessions created by tools such as Codex, Claude Code,
Continue, OpenCode, Gemini, Qwen Code, GitHub Copilot CLI, Kiro, Cline, and
Goose. It imports their data without modifying the native stores, normalizes
events with complete provenance, relates sessions through a global-session
graph, and delivers a compact current state to another agent.

## Product promise

The first release focuses on four capabilities:

1. reliable incremental import of Codex sessions;
2. traceable normalization of native events;
3. cross-tool management of global sessions; and
4. compact handoff delivery through MCP.

The [complete project plan](architecture/project-plan.md) is the canonical
product specification. The [implementation status](development/implementation-status.md)
identifies the next executable task.

## Design principles

- Local first and explicit remote opt-in
- Immutable raw imports and read-only native stores
- Deterministic, idempotent normalization
- Global sessions as reference graphs
- Progressive disclosure instead of transcript flooding
- Evidence-based correlation with human review
- Root-cause fixes and sustainable engineering

## System flow

```mermaid
flowchart LR
    native["Native agent sessions"]
    ingest["Read-only discovery<br/>and ingestion"]
    canonical["Canonical event store"]
    graph["Global-session graph"]
    handoff["Current handoff"]
    integrations["MCP · Hooks · CLI · ACP"]
    agents["Coding agents"]

    native --> ingest --> canonical --> graph --> handoff --> integrations --> agents
```

SessionMesh runs as a [native or containerized service](architecture/deployment.md).
