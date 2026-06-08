# syntax=docker/dockerfile:1
FROM rust:1-bookworm AS build
WORKDIR /src
# The manager is a Cargo *workspace member*: it inherits dependency versions
# from the root manifest ([workspace.dependencies]) and has a path dependency
# on crates/zitadel-auth. Building it standalone (copying only manager/) fails
# with "failed to find a workspace root", so we reproduce the full workspace and
# ask cargo to build just the manager package. cargo compiles only
# llm-chat-manager and its dependency closure (zitadel-auth + registry crates);
# the other members (worker, admin-api, clients/rust) are present so the
# workspace resolves, but they are NOT compiled.
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY manager ./manager
COPY worker ./worker
COPY admin-api ./admin-api
COPY clients/rust ./clients/rust
RUN cargo build --release --locked -p llm-chat-manager

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
# Workspace builds land in the ROOT target/ dir, not manager/target/.
COPY --from=build /src/target/release/llm-chat-manager /usr/local/bin/llm-chat-manager
COPY deploy/compose/entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /usr/local/bin/entrypoint.sh
ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
