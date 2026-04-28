# cli-multi-client

**One backend, two Python CLI clients, two simultaneous sessions, auto-close
on disconnect.** Exercises the `/s/new` magic-sid flow: each client opens
ONE WebSocket; the manager auto-spawns a session for it, returns the sid as
the first JSON frame, bridges the PTY, and auto-closes the session when the
client drops.

## Setup

Build the Python CLI venv (one time):

```bash
cd ../llm-chat-py-cli
python3 -m venv .venv
.venv/bin/pip install -r requirements.txt
```

Start the manager with **one** backend:

```bash
# from llm-chat project root
setsid env MANAGER_STEALTH=1 MANAGER_INSTANCES=1 \
  ./manager/target/release/llm-chat-manager \
  > /tmp/manager.log 2>&1 < /dev/null &
disown
sleep 14
grep -E "OK|listening|spawning" /tmp/manager.log
# Expect:
#   instance_port=7878 backend_pid=…
#   backend ready instance_port=7878
#   manager listening addr=ws://127.0.0.1:7777
```

## Run

```bash
bash testcases/e2e/cases/cli-multi-client/run.sh
```

## PASS criteria

```
=== summary ===
C1 magic=PINEAPPLE_ALPHA own=1 foreign=0
C2 magic=DAFFODIL_BRAVO  own=1 foreign=0
PASS
```

- `own ≥ 1`  — each client saw `● <its magic>` in the raw stream (claude answered)
- `foreign = 0` — no client saw the *other* client's magic word (no cross-talk)

In the manager log you should also see — one block per client:

```
INFO manager::s_new: auto-spawn session sid=s1777… backend_port=7878
INFO manager::s_new: client disconnected → auto-close sid=s1777…
```

## Cleanup

The clients auto-close their own sessions (that's the whole point). Just
tear down the manager when done:

```bash
pkill -f llm-chat-manager
pkill -f xvfb-run
pkill -f Xvfb
pkill -f "release/llm-chat$"
```
