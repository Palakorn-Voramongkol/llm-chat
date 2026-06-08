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

## Use the source of truth — don't scrape-and-reconstruct

When you need data another program produces (its output, its state, its
structure), consume that program's **real, structured output** — not a
rendered/derived view of it that you then try to reverse-engineer.

- do **not** scrape a rendered terminal/TUI/HTML view and guess back the
  original structure with heuristics (width thresholds to "unwrap" lines,
  regexes to strip chrome, inserting blank lines to fake layout)
- do **not** stack pattern-matches to paper over a lossy capture
- **do** find the program's machine-readable mode (JSON/stream-json/`--print`,
  an API, an event stream) and use that as the single source of truth

**Why:** a rendered view has already lost information (wrapping, chrome,
truncation). Every heuristic to undo that is a guess that is fragile, never
exact, and accumulates as exactly the "dirty fixes" rule 1 forbids. Example
from this project: the worker scraped claude's xterm grid and reconstructed
answers with width/blank/chrome heuristics; the root-cause fix was to drive
claude with `--output-format stream-json` and read its actual answer text.

**How to apply:** when output "looks wrong," don't add another rule to the
parser — ask "is there a structured source I should be reading instead?" If the
current design forces reconstruction, treat that as the bug to fix.

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

## Don't guess — verify

Do **not** guess values, APIs, behaviors, or appearances and ship them as if
you know. If you don't know something, find it out from the real source before
acting:

- an exact color/shape/layout (e.g. "what does a LINE chat bubble look like")
  → look it up from a reference and, for anything visual, **render it and look
  at the result** (a screenshot, a preview page) before claiming it's right —
  don't hand-derive CSS you never see
- an API signature, a config key, a flag, a return shape → read the docs or the
  actual code/types, don't assume
- a runtime behavior → run it and observe, don't predict
- a magic constant (port, path, role name, hex) → read it from the source of
  truth, don't invent a plausible one

**Why:** a confident guess that's wrong wastes more time than admitting "I need
to check," and it erodes trust — the user can't tell which of your claims are
verified and which are invented. This is the same failure as rule 2 (scraping a
view and reconstructing it): substituting a guess for the real thing.

**How to apply:** when you catch yourself about to write "this should be…",
stop and get the real answer. If you genuinely can't verify (no access, can't
see the pixels), say so explicitly and flag the part that's unverified — never
present a guess as fact.

## Fail closed — no fallbacks, no silent defaults (this is a security-sensitive system)

This system handles authentication, authorization, per-user identity, secrets,
and resource/path confinement. In **any** security-relevant path, never fall
back to a weaker behavior and never substitute a silent default for a value that
is required but missing, invalid, or unprovable. **Fail closed** — reject the
operation, or refuse to start, loudly.

- do **not** fall back to a shared / anonymous / "default" identity or
  environment when the real one is absent (e.g. no authenticated user id →
  reject; never invent a `_shared` user or directory)
- do **not** default a missing required secret / credential / path / config to
  some value — require it and **fail fast at startup**, naming exactly what is
  missing
- do **not** sanitize-and-continue a dangerous input (path traversal, injection,
  spoofed identity) — reject it; do not strip the bad part and proceed
- do **not** proceed when a safety / confinement check cannot be **proven**
  (canonicalization fails, a bound check is inconclusive, a signature can't be
  verified) — reject

**Why:** every fallback or silent default in a security boundary is a loophole.
A "convenient" default identity, a shared fallback directory, or a
sanitize-and-keep-going path is exactly the gap an attacker walks through.
Failing closed is occasionally less convenient but never insecure: a missing or
suspicious value must stop the system loudly, not quietly weaken its guarantees.

**How to apply:** when you are about to write `unwrap_or(<default>)`,
`x ?? default`, `if missing { use X }`, or "strip the bad part and continue" on
anything touching auth, identity, secrets, or resource/path boundaries — stop.
Make the value REQUIRED (fail fast / reject). Reserve defaults strictly for
non-security, purely behavioral or cosmetic choices, and state them explicitly.
(This is rule 1 applied to security: a fallback that weakens a guarantee is a
dirty fix wearing a convenience hat.)

## Never relax security, authentication, or authorization

Do not weaken, loosen, bypass, or "temporarily" disable any security control —
authentication, authorization, identity/JWT/signature verification, role and
scope gates, path/resource confinement, TLS, input validation — to make
something work, pass a test, unblock a demo, or simplify code. The secure path
is the only path. Tighten, never loosen.

- do **not** disable or skip an auth/authz check (comment it out, `if true`,
  remove the role gate, accept an unsigned/unverified token) to get past a
  failure — fix the actual cause
- do **not** loosen a requirement for convenience: don't widen a role/scope, set
  a CORS/origin allowlist to `*` with credentials, downgrade to a weaker auth
  mode, or trust client-supplied identity
- do **not** add a backdoor, bypass flag, or "dev-only" shortcut around auth —
  dev and prod use the **same** security model
- do **not** broaden a confinement boundary or accept input you haven't verified

If a security control genuinely blocks correct behavior, the control or the
design is what changes — **deliberately, surfaced to the user** — never quietly
relaxed in passing.

**Why:** relaxed auth/authz is the most expensive class of bug: it doesn't fail
loudly, it silently grants access. A bypass added "just to test" or "just for
dev" outlives its excuse and ships. In a sensitive system an unauthorized action
is worse than a failed one.

**How to apply:** if a change would make an auth/authz/identity check pass more
often, accept more inputs, or require fewer proofs, treat it as a red flag and
stop. Confirm the control is still at least as strict as before; if you must
change it, say so explicitly and let the user decide.
