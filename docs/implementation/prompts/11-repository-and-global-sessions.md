# Prompt 11: Repository Identity and Global Sessions

## Objective

Identify repositories and worktrees, create global sessions, link and unlink
native sessions with evidence, and support explicit project context.

## Prerequisites

Prompts 04, 08, 09, and 10 are complete.

## Work

- Capture Git root, normalized remote URL, branch, worktree, HEAD, ancestry
  evidence, and dirty state without changing the repository.
- Implement global-session creation and audited membership changes.
- Read and write `.sessionmesh/current.json` atomically with only global ID,
  objective, and update time; exclude transcripts and secrets.
- Score deterministic correlation features and retain per-feature evidence,
  versions, confidence, and manual overrides.
- Add API and web review flows for create, link, unlink, accept, and reject.

## TDD and acceptance

- Cover repositories without remotes, detached HEAD, multiple worktrees,
  rewritten remotes, dirty state, malformed marker files, and concurrent manual
  decisions.
- Manual rejection prevents silent relinking until explicitly reversed.
- Native sessions and transcripts are never copied into membership records.
- Document identity and scoring, pass all gates, and select Prompt 12.
