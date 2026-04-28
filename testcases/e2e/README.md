# End-to-end test cases

One subfolder per scenario under `cases/`. Each scenario has a `run.*` script
and a `README.md` explaining what it tests, what to set up first, and what a
PASS looks like.

All test cases assume the project's release binaries are built:

```bash
(cd src-tauri && cargo build --release)
(cd manager   && cargo build --release)
```

Auth token: every test reads it from `~/.local/share/com.llm-chat.app/auth.token`,
which the manager writes at startup. (The CLI tests can also use
`LLM_CHAT_TOKEN=devtoken123` against a manually-spawned single backend.)

## Scenarios (run order = simplest → most complex)

| Scenario | Setup | What it proves |
|---|---|---|
| [`single-backend-qa`](cases/single-backend-qa/) | one backend on 7878 with `LLM_CHAT_AUTH_TOKEN=devtoken123` | A single backend's PTY + parser + `/qa/` round-trip works for the auto-spawned boot session. |
| [`raw-stream-debug`](cases/raw-stream-debug/) | manager up | Captures the raw `/s/<sid>` byte stream for one session — diagnostic, not pass/fail. |
| [`buffer-debug`](cases/buffer-debug/) | manager up | Long capture of one session, counts `❯` / `●` / target-word occurrences. Diagnostic. |
| [`manager-4session`](cases/manager-4session/) | manager with `MANAGER_INSTANCES=2` | 4 sessions opened via `/control` round-robin to 2 backends; each gets its own claude reply via raw `/s/` AND parsed `/qa/`. |
| [`cli-multi-client`](cases/cli-multi-client/) | manager with `MANAGER_INSTANCES=1` + Python CLI venv | 2 Python clients connect to `/s/new`; manager auto-spawns a session per client; each gets its own reply; auto-close on disconnect. |

## Reading the output

After tracing was added, both the manager and the backend log to stderr in
this format (override level via `RUST_LOG=manager=debug,backend=debug`):

```
2026-04-28T10:00:00.000Z  INFO manager: instance spawned instance_port=7878 backend_pid=1234 …
2026-04-28T10:00:01.000Z DEBUG backend::ws: WS server listening port=7878
2026-04-28T10:00:01.500Z  INFO manager: backend ready instance_port=7878
2026-04-28T10:00:01.510Z  INFO manager: manager listening addr=ws://127.0.0.1:7777
```

Manager logs the lifecycle (spawn / ready / session-open / session-close).
Backend logs only what manager can't observe (handshake errors, PTY-internal
events). For machine-readable JSON, set `LOG_JSON=1`.
