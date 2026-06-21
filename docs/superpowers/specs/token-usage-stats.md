# Per-user token-usage statistics — design

**Status:** designed 2026-06-21; not yet implemented.

**Goal:** record claude's token usage for every Q&A, attribute it to the
authenticated user (human *or* machine), and surface per-user totals
(tokens-in / tokens-out / cost) on the Console's Users page.

## Source of truth (verified)

The worker drives `claude -p --output-format stream-json`. The `result` event
it already parses for the answer text also carries usage. Verified live:

```json
{ "type": "result", "subtype": "success", "result": "…answer…",
  "usage": { "input_tokens": 4335, "output_tokens": 4,
             "cache_creation_input_tokens": 7060,
             "cache_read_input_tokens": 19447 },
  "total_cost_usd": 0.102, "modelUsage": { "claude-opus-4-8": { … } } }
```

No new Anthropic/API calls are needed — the data is in the event the worker
discards today (`worker/src/lib.rs`, stream-json reader ~line 795). The legacy
PTY transport emits no `usage`; those answers store NULL usage.

## Definitions

- **tokens_in (displayed)** = `input_tokens + cache_read_input_tokens +
  cache_creation_input_tokens` (the full input footprint). The three components
  are stored **raw and separately** (columns `tokens_in`, `cache_read_tokens`,
  `cache_creation_tokens` hold the un-summed values); the cache-inclusive total
  is composed when building the `/control` reply, so the detail panel can still
  show the split.
- **tokens_out** = `output_tokens`.
- **cost_usd** = `total_cost_usd`.
- Totals are **cumulative, all-time**. Because usage is stored per Q&A row,
  per-day/time-windowed views are a `GROUP BY date(time_in)` away later (out of
  scope now).

## Architecture & data flow

Rides the two existing planes — no new infrastructure.

```
worker (reads usage from claude result)
   │  qa payload now includes a `usage` object
   ▼
manager  (chat plane, source of truth)
   • chat_question gains user_id + token columns; per-Q&A is authoritative
   • /control "usage" aggregates SUM(...) GROUP BY user_id   (chat.admin)
   ▲
   │ header-based control_query (the hardened path)
admin-api  • GET /api/usage  (chat.admin gate)
   ▲
   │ same-origin /api proxy
admin-web  • Users page: Tokens-in / Tokens-out / Cost columns + detail panel
```

## Components

### 1. Worker — `worker/src/lib.rs` (stream-json reader)

In the `is_result && ok` branch (~795) that builds the qa payload, also read
`usage` and `total_cost_usd` and attach a `usage` object to the payload:

```jsonc
{ "num": n, "question": q, "answer": answer, "sessionId": sid,
  "isNew": true, "final": true,
  "usage": {                       // null/omitted on the PTY transport
    "inputTokens": 4335, "outputTokens": 4,
    "cacheReadTokens": 19447, "cacheCreationTokens": 7060,
    "costUsd": 0.102, "model": "claude-opus-4-8" }
}
```

A small pure helper `parse_usage(&Value) -> Option<Usage>` keeps the parsing
unit-testable. Missing/partial usage → fields default to 0 / `None` cost; the
answer is forwarded regardless (never fail an answer over telemetry).

### 2. Manager — `manager/src/main.rs` (storage + aggregation)

**Schema.** Extend `chat_question` with idempotent `ALTER TABLE ADD COLUMN`
(run for both the SQLite and Postgres init paths; ignore "duplicate column"):
`user_id TEXT`, `tokens_in INTEGER`, `tokens_out INTEGER`,
`cache_read_tokens INTEGER`, `cache_creation_tokens INTEGER`,
`cost_usd REAL`, `model TEXT`. Existing rows keep NULLs (unattributed → shown
as "—").

**Attribution.** `user_id` is written at INSERT, from the authenticated session
owner the manager already holds (JWT `sub` in `session_to_owner` / the per-
connection record). It is never client-supplied.

**Recording usage.** When the `/qa` payload carrying `usage` is paired to its
question (the same place `mark_answered` runs), write the token columns onto
that row. A pure `usage_update_values(&Usage)` builds the bound parameters.

**New `/control` command `"usage"`** (added to the `match cmd` dispatch ~1226,
chat.admin-gated like the rest). Runs:

```sql
SELECT user_id,
       COUNT(*)                       AS requests,
       COALESCE(SUM(tokens_in),0)     AS tokens_in,
       COALESCE(SUM(tokens_out),0)    AS tokens_out,
       COALESCE(SUM(cache_read_tokens),0)     AS cache_read_tokens,
       COALESCE(SUM(cache_creation_tokens),0) AS cache_creation_tokens,
       COALESCE(SUM(cost_usd),0)      AS cost_usd,
       MAX(time_out)                  AS last_used
FROM chat_question
WHERE status IN ('answered','confirmed')
GROUP BY user_id;
```

The handler composes each reply row's `tokensIn` = `tokens_in + cache_read_tokens
+ cache_creation_tokens` (the SQL above returns the raw component sums). Reply
shape:

```json
{ "ok": true,
  "users": [ { "userId": "290…", "requests": 42,
               "tokensIn": 123456, "tokensOut": 7890,
               "cacheReadTokens": 100000, "cacheCreationTokens": 20000,
               "costUsd": 1.23, "lastUsed": 1718971200 } ],
  "totals": { "requests": 42, "tokensIn": 123456, "tokensOut": 7890,
              "costUsd": 1.23 } }
```

`tokensIn` in the reply is the pre-summed `input + cache_read + cache_creation`;
the components remain available for the detail panel.

### 3. admin-api — `admin-api/src/`

- `manager.rs`: `usage()` via the existing header-based `control_query(url,
  token, "usage")`; reuse the minted chat.admin token.
- `api/mod.rs`: `GET /api/usage` handler behind the `Operator` (chat.admin)
  extractor, returning the manager's reply verbatim (typed pass-through).

### 4. admin-web — `admin-web/`

- `lib/api.ts` + a `UsageRow` type; fetch `/api/usage` on the Users page and
  index by `userId`.
- `components/users/columns.tsx`: add **Tokens in**, **Tokens out**, **Cost
  (USD)** columns (right-aligned, thousands-formatted; cost to 2–4 dp), joined
  by `user.id`. Header tooltips (markdown) explaining each, matching the
  existing column-tooltip pattern. Unattributed/no-usage → "—".
- User **detail panel**: full breakdown — input vs cache-read vs cache-creation,
  output, requests, cost, last used.

## Boundaries / non-goals

- No per-day/time-series UI yet (data supports it; defer the charts).
- No quotas/limits/enforcement — this is reporting only.
- No reset/rollover; cumulative all-time.
- Cost is claude's reported `total_cost_usd` (informational, not billing-grade).

## Error handling

- Worker: partial/missing `usage` → zeros / no cost; answer still delivered.
- Manager: a usage write failure is logged, never blocks the answer; the row
  simply keeps NULL usage.
- PTY transport: no usage → NULL columns (the default transport is stream-json).
- admin-api/web: `/api/usage` unreachable → the columns render "—" and the page
  still loads (best-effort, like the Sessions panel).

## Testing

- **Worker:** `parse_usage` returns the right struct from a sample `result`
  line; returns None/zeros on a line without `usage`.
- **Manager:** INSERT records `user_id`; the usage update writes the token
  columns; the `usage` aggregate query sums per user and excludes pending rows
  (SQLite fixture). `usage_update_values` is pure-tested.
- **admin-api:** `/api/usage` is chat.admin-gated and passes the manager reply
  through unchanged.
- **admin-web:** the Users columns render formatted numbers and join by id;
  missing usage shows "—".

## Migration

Idempotent `ALTER TABLE ADD COLUMN` on startup for existing `manager.sqlite` /
Postgres DBs. No data rewrite; historical rows are unattributed (NULL) until new
traffic accrues.
