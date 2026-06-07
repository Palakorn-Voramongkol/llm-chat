"""Static contract tests for the admin-api / admin-web Dockerfiles.

No Docker daemon: we assert the build *recipe* matches the locked contract
(multi-stage rust->debian-slim for the BFF; node:20-alpine standalone for the
web). A real `docker build` is exercised by the compose acceptance task (Task 26).
"""
from pathlib import Path

HERE = Path(__file__).resolve().parent
API = HERE / "admin-api.Dockerfile"
WEB = HERE / "admin-web.Dockerfile"


def test_admin_api_is_multistage_rust_to_debian_slim() -> None:
    t = API.read_text(encoding="utf-8")
    # build stage: workspace-aware rust toolchain
    assert "FROM rust:1-bookworm AS build" in t
    # workspace build: the crate lives in a Cargo workspace, so the whole
    # workspace manifest + the sibling crates it shares a lock with must be
    # present for `cargo build --locked` to resolve.
    assert "cargo build --release --locked -p llm-chat-admin-api" in t
    # runtime stage: slim debian with TLS roots (rustls verifies the issuer cert)
    assert "FROM debian:bookworm-slim" in t
    assert "ca-certificates" in t
    assert (
        "COPY --from=build /src/target/release/llm-chat-admin-api "
        "/usr/local/bin/llm-chat-admin-api" in t
    )
    assert 'ENTRYPOINT ["/usr/local/bin/llm-chat-admin-api"]' in t


def test_admin_web_is_node_standalone() -> None:
    t = WEB.read_text(encoding="utf-8")
    assert "FROM node:20-alpine AS build" in t
    assert "corepack enable" in t            # pnpm via corepack (repo uses pnpm)
    assert "pnpm install --frozen-lockfile" in t
    assert "pnpm run build" in t
    # next.config sets output:'standalone' -> copy the standalone server
    assert "COPY --from=build /app/.next/standalone ./" in t
    assert "COPY --from=build /app/.next/static ./.next/static" in t
    assert 'CMD ["node", "server.js"]' in t
    assert "EXPOSE 3000" in t
