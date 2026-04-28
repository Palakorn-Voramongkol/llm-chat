# single-backend-qa

**Smallest possible end-to-end check.** Talks to ONE backend directly (no
manager). Confirms the PTY → xterm → parser → `/qa/` round-trip works for the
backend's auto-spawned boot session.

## Setup

```bash
# from project root
LLM_CHAT_AUTH_TOKEN=devtoken123 LLM_CHAT_WS_PORT=7878 \
  setsid xvfb-run -a ./worker/target/release/llm-chat \
  > /tmp/single-backend.log 2>&1 < /dev/null &
disown
sleep 8
grep "WS server" /tmp/single-backend.log     # expect "listening on ws://127.0.0.1:7878"
```

(`setsid` and `< /dev/null` matter — without them, the backend exits the
moment its parent shell tries to read from the controlling TTY.)

## Run

```bash
node testcases/e2e/cases/single-backend-qa/run.cjs
```

## PASS criteria

```
PASS: end-to-end works
```

Internally checks:
- `/control list` returns the boot session id
- After sending `reply with only the single word SIMPLEST`, the raw `/s/`
  stream contains `SIMPLEST` (claude answered)
- `/qa/<sid>` emits at least one `qa-detected` event with `SIMPLEST` in the
  parsed answer (parser saw the same thing)

## Cleanup

```bash
pkill -f "release/llm-chat$"
pkill -f Xvfb
```
