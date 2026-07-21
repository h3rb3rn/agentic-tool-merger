# Prompt 00: Workspace Bootstrap

## Objective

Create a reproducible Rust and React monorepo foundation without implementing
product behavior.

## Prerequisites

None. Confirm this prompt is the ledger's `Next prompt`, then mark it
`in_progress`.

## Work

- Create the Cargo workspace and the crates defined by the project plan.
- Pin a stable Rust toolchain and configure `rustfmt`, Clippy, unit tests, and
  workspace-wide dependency policy.
- Create `apps/web` with React, TypeScript, Vite, strict TypeScript, and the
  planned TanStack dependencies. Add React Flow and Monaco only when first used.
- Add shared, cross-platform development commands for formatting, linting,
  testing, building, and documentation.
- Add `.gitignore`, editor defaults, a minimal English `README.md`, and license
  metadata consistent with Apache-2.0.
- Add a production-oriented multi-stage OCI build usable by Docker and Podman.
  Run as non-root, set `SESSIONMESH_HOME=/var/lib/sessionmesh`, declare the
  persistent state mount, and provide compose examples with agent sources
  mounted read-only under `/sources/<tool>`.
- Document native startup, Docker, Podman, UID/GID ownership, health checks,
  localhost publishing, and SELinux volume-label examples.
- Keep every crate and application compilable with the smallest meaningful
  placeholder surface; do not implement sessions, adapters, or storage.

## TDD and acceptance

- Add a workspace smoke test or equivalent package-level tests before adding
  nontrivial bootstrap helpers.
- `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace`, web lint/typecheck/test/build, and
  `mkdocs build --strict` pass.
- A clean checkout can run the documented development commands.
- Docker and Podman can run the same image contract with persistent state, and
  restarting the container retains a bootstrap state file without writing to a
  mounted native source fixture.
- Update the architecture documentation, changelog, and ledger. Set Prompt 01
  as next only after all gates pass.
