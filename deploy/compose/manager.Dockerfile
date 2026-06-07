# syntax=docker/dockerfile:1
FROM rust:1-bookworm AS build
WORKDIR /src
COPY manager/Cargo.toml manager/Cargo.lock ./manager/
COPY manager/src ./manager/src
RUN cargo build --release --locked --manifest-path manager/Cargo.toml

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/manager/target/release/llm-chat-manager /usr/local/bin/llm-chat-manager
COPY deploy/compose/entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /usr/local/bin/entrypoint.sh
ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
