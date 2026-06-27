# Rich output mode for the stream-json transport

**Date:** 2026-06-27
**Status:** Approved (design)
**Scope:** `worker/` (core) + `manager/` (thin passthrough)

## Problem

The headless worker drives `claude -p --output-format stream-json --verbose` and
`JsonSession`'s stdout reader forwards **only** the final `result` event
(`worker/src/lib.rs:818`), discarding every intermediate event. A consumer of
`/qa/<sid>` therefore sees one finished answer per question and nothing of the
live work — no thinking, no tool calls, no streaming.

The legacy PTY/GUI path surfaced that richness by **scraping the xterm grid**
(`worker/frontend/claude_cli_parser.js`: width-threshold line-unwrapping +
chrome regexes), which is exactly what CLAUDE.md rule 2 forbids and what the
stream-json transport was built to replace. We want the richness **without**
reviving the scrape.

## Key insight

`claude` already emits all of it as **structured JSON** we currently drop.
Verified on claude v2.1.195:

- **Turn-level** (plain `--verbose`): top-level `assistant` (blocks: `thinking`
  / `tool_use` / `text`), `user` (`tool_result`), `result`, plus `system:init`
  and noise (`system:hook_*`, `rate_limit_event`).
- **Token-level** (`--include-partial-messages`): additionally `stream_event`
  lines mirroring the Anthropic streaming SSE — `message_start`,
  `content_block_start`, `content_block_delta` (`text_delta` / `thinking_delta`
  / `input_json_delta` / `signature_delta`), `content_block_stop`,
  `message_delta`, `message_stop`. Each line carries claude's own `session_id`.

Forwarding these verbatim **honors** rule 2 (read the structured source) instead
of bending it.

## Design

### Selector: `off` | `turn` | `token`

- **`off`** (default) — only the final `result`. Identical to today.
- **`turn`** — final result **plus** `assistant` / `user` / `system:init`
  events as each completes. No extra claude flag.
- **`token`** — the `turn` set **plus** `stream_event` partial deltas. Adds
  `--include-partial-messages` to the claude spawn args.

### Selection (per session, resolved at spawn)

1. Open-command field: `{"cmd":"open","rich":"token"}`.
2. Worker env `LLM_CHAT_RICH=off|turn|token` (worker-wide default).
3. Fallback `off`.

`token` is the only level that changes claude's argv, decided once in
`JsonSession::spawn`. The worker is the source of truth for the resolved level
and echoes it in the open reply (`"rich":"<level>"`), mirroring how `transport`
is reported today.

### Wire shape — additive, backward-compatible

Each rich line is wrapped with **no top-level `num`**:

```json
{"type":"event","level":"token","kind":"stream_event","sessionId":"s…","seqNo":7,"raw":{…claude event verbatim…}}
```

- `raw` is claude's event passed through untouched (source of truth; no
  reshaping). Only pure noise is dropped (`system:hook_*`, `rate_limit_event`),
  and an oversized `raw` is capped so the bounded broadcast channel can't lag.
- The manager **ignores** every no-`num` line: `manager/src/main.rs:2879`
  (`num … None => continue`). Answer pairing / FIFO is untouched.
- The final `result` payload (`num` + `final:true`) is **unchanged**, so every
  existing python/rust client behaves identically.

### `/qa/<sid>` relay

`/qa/<sid>` already forwards whatever JSON lines land on `qa_tx`
(`worker/src/lib.rs:2771`), so emitting more lines needs no endpoint change.

## Components touched

- **worker/src/lib.rs**
  - `RichLevel` enum + `parse`.
  - `do_spawn_session(..., rich_override: Option<String>)` — resolve level
    (override → env → off); both call sites updated (GUI `:1409` passes `None`).
  - `JsonSession::spawn(..., rich: RichLevel)` — append
    `--include-partial-messages` iff `Token`; reader forwards intermediate
    events per level with a per-session `seqNo` counter; `result` handling
    unchanged.
  - Control `open` handler — read `req["rich"]`, pass through, echo resolved
    level in reply.
- **manager/src/main.rs** (thin passthrough)
  - `/chat` parses `?rich=` next to `?cwd=` (`:1539`).
  - `handle_chat` / `cmd_open` / `open_request_body` thread the value into the
    worker `open` body.

## Testing (bottom-up, per CLAUDE.md)

Run `llm-chat-headless` directly. Open three sessions — `rich=off`, `rich=turn`,
`rich=token` — drive the same tool-using prompt (`echo hello-from-tool`),
subscribe `/qa/<sid>`, and confirm:

- `off` → only the final `result` line.
- `turn` → `assistant`/`tool_use`/`tool_result` events, then the final result.
- `token` → additionally a `content_block_delta` stream.
- The final `result` payload is byte-identical across all three; the manager
  (if in the loop) pairs exactly one answer per question in every case.

## Out of scope (follow-ups)

- Rendering deltas in the python/rust clients.
- Per-session `rich` UI in the admin Console.
