# llm-chat Linux build & smoke test
#
# Stage 1 — build llm-chat backend (Tauri 2) and llm-chat-manager from sources.
# Stage 2 — minimal runtime image with WebKitGTK, Xvfb, and a stub `claude`
#           binary (we don't have the real CLI in CI). The default CMD runs
#           the manager under Xvfb and exposes ports 7777 (manager) + 7878-7879
#           (backends). Smoke tests run via `docker exec` against the manager.

FROM ubuntu:24.04 AS builder

ENV DEBIAN_FRONTEND=noninteractive

# Tauri 2 on Linux needs WebKitGTK 4.1, libsoup-3, javascriptcoregtk-4.1, plus
# librsvg/glib/gtk dev headers. portable-pty needs nothing extra.
RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential \
        ca-certificates \
        curl \
        file \
        libgtk-3-dev \
        libwebkit2gtk-4.1-dev \
        libjavascriptcoregtk-4.1-dev \
        libsoup-3.0-dev \
        librsvg2-dev \
        libssl-dev \
        pkg-config \
        wget \
    && rm -rf /var/lib/apt/lists/*

# Install Rust via rustup (system rustc on ubuntu:24.04 is 1.75 — we want a
# recent toolchain for tauri 2).
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
ENV PATH="/root/.cargo/bin:${PATH}"

WORKDIR /build
COPY src-tauri/ src-tauri/
COPY manager/ manager/
COPY src/ src/
COPY package.json package-lock.json* ./

# Build the backend lib & exe and the manager. We don't run `tauri build`
# (it bundles AppImages we don't need); a plain `cargo build` produces the
# binaries we want.
RUN cargo build --manifest-path src-tauri/Cargo.toml \
 && cargo build --manifest-path manager/Cargo.toml

# ---------- runtime ----------
FROM ubuntu:24.04 AS runtime

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates \
        curl \
        libgtk-3-0 \
        libwebkit2gtk-4.1-0 \
        libjavascriptcoregtk-4.1-0 \
        libsoup-3.0-0 \
        librsvg2-2 \
        sudo \
        xvfb \
        xdg-utils \
    && rm -rf /var/lib/apt/lists/*

# Non-root dev user. Password "GogoPure0811" is for `sudo` and `su` inside the
# container; it is hardcoded into the image layer (anyone with the image can
# read the /etc/shadow hash), so don't reuse it elsewhere.
RUN useradd -m -s /bin/bash claudecode \
 && echo 'claudecode:GogoPure0811' | chpasswd \
 && usermod -aG sudo claudecode

# Node + npm + the real Claude CLI. Login is interactive — run
# `docker exec -it <container> claude /login` once after first start, or pass
# `-e ANTHROPIC_API_KEY=...` to docker run for headless auth.
RUN curl -fsSL https://deb.nodesource.com/setup_22.x | bash - \
 && apt-get install -y --no-install-recommends nodejs \
 && rm -rf /var/lib/apt/lists/* \
 && npm install -g @anthropic-ai/claude-code \
 && claude --version || true

WORKDIR /app
COPY --from=builder /build/src-tauri/target/debug/llm-chat /app/src-tauri/target/debug/llm-chat
COPY --from=builder /build/manager/target/debug/llm-chat-manager /app/manager/target/debug/llm-chat-manager

# Pre-create dirs the manager + claude need so their first write doesn't have
# to mkdir as the non-root user, then hand /app and /home/claudecode over to
# the claudecode user. Xvfb refuses to create /tmp/.X11-unix when euid != 0,
# so we mkdir it here with the standard sticky-bit mode.
RUN mkdir -p /home/claudecode/.claude /home/claudecode/.local/share/com.llm-chat.app /tmp/.X11-unix \
 && chmod 1777 /tmp/.X11-unix \
 && chown -R claudecode:claudecode /app /home/claudecode

# Entrypoint: start Xvfb, wait for its socket to appear, then exec the manager.
# Without the wait, the manager spawns the Tauri backends before Xvfb is ready
# and they panic in `gtk::rt::init`.
RUN printf '#!/bin/sh\n\
Xvfb :99 -screen 0 1024x768x24 &\n\
for i in $(seq 1 50); do\n\
    [ -S /tmp/.X11-unix/X99 ] && break\n\
    sleep 0.1\n\
done\n\
exec /app/manager/target/debug/llm-chat-manager\n' > /app/entrypoint.sh \
 && chmod 755 /app/entrypoint.sh

USER claudecode
WORKDIR /home/claudecode

# Default config: 2 backends starting at 7878, manager on 7777, headless.
# `--dangerously-skip-permissions` requires a non-root user (we run as
# `claudecode`) and skips Claude Code's tool-permission prompts so backends
# don't hang waiting for human input.
ENV MANAGER_PORT=7777 \
    MANAGER_INSTANCES=2 \
    MANAGER_START_PORT=7878 \
    MANAGER_STEALTH=1 \
    LLM_CHAT_EXE=/app/src-tauri/target/debug/llm-chat \
    LLM_CHAT_CLAUDE_ARGS=--dangerously-skip-permissions \
    DISPLAY=:99

EXPOSE 7777 7878 7879

CMD ["/app/entrypoint.sh"]
