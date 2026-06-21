# syntax=docker/dockerfile:1
# kabytech-backend: the Rust OIDC Relying Party (axum). Multi-stage like
# admin-api.Dockerfile — the crate lives in a Cargo workspace, so the whole
# workspace manifest + lock and every member must be COPYd before `cargo build`.
FROM rust:1-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY manager/Cargo.toml ./manager/Cargo.toml
COPY manager/src ./manager/src
COPY worker/Cargo.toml worker/build.rs ./worker/
COPY worker/src ./worker/src
COPY admin-api/Cargo.toml ./admin-api/Cargo.toml
COPY admin-api/src ./admin-api/src
COPY clients/rust/Cargo.toml ./clients/rust/Cargo.toml
COPY clients/rust/src ./clients/rust/src
COPY services/kabytech/backend/Cargo.toml ./services/kabytech/backend/Cargo.toml
COPY services/kabytech/backend/src ./services/kabytech/backend/src
RUN cargo build --release --locked -p kabytech-backend

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/kabytech-backend /usr/local/bin/kabytech-backend
# A tiny entrypoint sources /out/manager.generated.env (project_id / audience)
# and reads the OIDC client id/secret from mounted secret files before exec.
COPY deploy/compose/kabytech-backend-entrypoint.sh /usr/local/bin/kabytech-backend-entrypoint.sh
RUN chmod +x /usr/local/bin/kabytech-backend-entrypoint.sh
EXPOSE 7670
ENTRYPOINT ["/usr/local/bin/kabytech-backend-entrypoint.sh"]
