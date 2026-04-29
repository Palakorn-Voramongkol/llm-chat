# Project rules for Claude

## No dirty fixes — root-cause everything

When something fails (build error, test failure, runtime error, hook
rejection, type error, anything), find the actual cause and fix that. Do not
apply a "dirty fix":

- do **not** skip the failing check (`--no-verify`, `# type: ignore`,
  `// eslint-disable-next-line` on the failing line)
- do **not** hardcode a value to step around the bug
- do **not** catch-and-ignore the error
- do **not** comment out the failing assertion

If a quick workaround is genuinely the only viable path right now (e.g.
unblocking an urgent demo), say so explicitly and flag the underlying issue
as remaining work. Do not pretend it's resolved.

**Why:** Quick fixes hide the real defect, accumulate as silent tech debt,
and usually surface again later in a worse form. Correctness over green-light
theater.

**How to apply:** Whenever a fix is tempting because it "makes the error go
away," pause and ask "what's actually broken here?" Trace the failure to its
source before editing. If the real fix is large, propose it explicitly rather
than shipping the band-aid; let the user decide.

## Debug bottom-up — isolate the lowest layer first

When an end-to-end flow fails (e.g. `client → manager → worker → claude`
returns no answer), do not start guessing at the top. Stop the upper layers,
start the lowest layer **alone**, and verify it works on its own. Then add
each layer back one at a time. The first layer that breaks is where the bug
lives.

For this project, the layers from bottom up are:

1. `claude` CLI alone (e.g. `claude --print "hello"` or interactively under
   `xvfb-run`).
2. `llm-chat-worker` alone — spawn it with `LLM_CHAT_STEALTH=1` under
   `xvfb-run`, drive its `/qa/<sid>` and `/control` WS endpoints directly.
3. `llm-chat-manager` on top of a known-good worker.
4. The Python client / nginx / Zitadel auth on top.

**Why:** A 60-second "no answer" timeout at the client tells you nothing
about which layer is wrong. Running the worker by hand lets you see PTY
spawn logs, claude stderr, and JS-parser events in real time — those are
silent under systemd. Most "the chat hangs" bugs surface as a missing
ready-event, an auth failure, or a parser regression, all of which are
obvious one layer down.

**How to apply:** Before adding tracing or bumping timeouts in the manager,
`systemctl stop llm-chat-manager`, run the worker in the foreground with
`RUST_LOG=debug`, and reproduce the failure against it directly. Only move
up to the manager once the worker round-trips a question cleanly.

## Check `config.md` first for known config gotchas

`config.md` at the repo root is a running log of non-obvious configuration
problems we've already debugged on this project (claude trust dialog,
systemd hardening pitfalls, Zitadel system-API quirks, JWKS preload races,
scope/roles claims, etc.) plus the fix that worked.

**When something looks like a configuration / deployment problem (auth
rejected, service won't start, "no answer" hangs, certs, env vars, ports)
read `config.md` BEFORE you start digging.** There is a real chance the
exact symptom is already documented with a one-paragraph fix.

**When you debug a new config problem to a clean fix, append it to
`config.md`** in the same format (Symptom / Why / Fix / Verify). New
entries go at the top. Future-you and future Claudes will save hours.
