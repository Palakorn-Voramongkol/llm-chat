# raw-stream-debug

**Diagnostic, not pass/fail.** Opens one session via the manager's `/control`
and dumps every byte claude sends back over `/s/<sid>`. Useful when something
is broken and you want to see the actual ANSI stream without xterm/parser
intermediation.

## Setup

Manager up (any number of instances):

```bash
setsid env MANAGER_STEALTH=1 MANAGER_INSTANCES=1 \
  ./manager/target/release/llm-chat-manager \
  > /tmp/manager.log 2>&1 < /dev/null &
disown
sleep 14
```

## Run

```bash
node testcases/e2e/cases/raw-stream-debug/run.cjs
```

## What it prints

A handful of decoded chunks (first 3, then every 20th) in JSON-string form,
followed by `FINAL: N chunks, M bytes received from claude`.

If `M` is small (<200 bytes) something is wrong before claude rendered its
banner — check the manager log for spawn errors.

If `M > 1000` and you can spot `❯`, `●`, claude's title bar, etc., the PTY
plumbing is fine.
