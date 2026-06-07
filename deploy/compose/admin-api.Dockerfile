# syntax=docker/dockerfile:1
# admin-api: the Rust BFF (axum). Multi-stage like manager.Dockerfile, but the
# crate lives in a Cargo workspace, so the whole workspace manifest + lock and
# every member it shares the lock with must be COPYd before `cargo build`.
FROM rust:1-bookworm AS build
WORKDIR /src
# Workspace skeleton: root manifest + single lock, then each member's sources.
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY manager/Cargo.toml ./manager/Cargo.toml
COPY manager/src ./manager/src
COPY worker/Cargo.toml worker/build.rs ./worker/
COPY worker/src ./worker/src
COPY admin-api/Cargo.toml ./admin-api/Cargo.toml
COPY admin-api/src ./admin-api/src
RUN cargo build --release --locked -p llm-chat-admin-api

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/llm-chat-admin-api /usr/local/bin/llm-chat-admin-api
# compose cannot env_file a path that only exists inside a runtime volume, so a
# tiny entrypoint sources /out/manager.generated.env (project_id / audience) and
# resolves the OIDC *_FILE secret indirection before exec-ing the binary.
COPY deploy/compose/admin-api-entrypoint.sh /usr/local/bin/admin-api-entrypoint.sh
RUN chmod +x /usr/local/bin/admin-api-entrypoint.sh
EXPOSE 7676
ENTRYPOINT ["/usr/local/bin/admin-api-entrypoint.sh"]
