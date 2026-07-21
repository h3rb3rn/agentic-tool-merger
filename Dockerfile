# syntax=docker/dockerfile:1

FROM node:20-bookworm-slim AS web-builder
WORKDIR /build
COPY package.json package-lock.json ./
COPY apps/web/package.json apps/web/package.json
RUN npm ci
COPY apps/web apps/web
RUN npm run build

FROM rust:1.88-bookworm AS rust-builder
WORKDIR /build
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates crates
COPY fixtures fixtures
COPY migrations migrations
RUN cargo build --locked --release --package sessionmesh-daemon --package sessionmesh-mcp

FROM debian:bookworm-slim AS runtime
ARG SESSIONMESH_UID=10001
ARG SESSIONMESH_GID=10001
RUN apt-get update \
    && apt-get install --yes --no-install-recommends curl \
    && rm -rf /var/lib/apt/lists/*
RUN groupadd --gid "${SESSIONMESH_GID}" sessionmesh \
    && useradd --uid "${SESSIONMESH_UID}" --gid "${SESSIONMESH_GID}" \
      --home-dir /var/lib/sessionmesh --no-create-home sessionmesh
COPY --from=rust-builder /build/target/release/sessionmesh-daemon /usr/local/bin/sessionmesh-daemon
COPY --from=rust-builder /build/target/release/sessionmesh-mcp /usr/local/bin/sessionmesh-mcp
COPY --from=web-builder /build/apps/web/dist /usr/share/sessionmesh/web
ENV SESSIONMESH_HOME=/var/lib/sessionmesh
RUN install -d -m 0700 -o sessionmesh -g sessionmesh /var/lib/sessionmesh
VOLUME ["/var/lib/sessionmesh"]
USER sessionmesh:sessionmesh
HEALTHCHECK --interval=10s --timeout=3s --retries=3 \
  CMD curl --fail --silent http://127.0.0.1:8787/api/v1/health || exit 1
ENTRYPOINT ["sessionmesh-daemon"]
