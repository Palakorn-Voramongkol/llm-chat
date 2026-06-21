# Self-counted per-user usage — design

**Status:** designed 2026-06-22; not yet implemented. **Supersedes the
measurement basis** of [`token-usage-stats.md`](token-usage-stats.md) and
[`token-usage-daily.md`](token-usage-daily.md): those count claude's
`result.usage`, which is the wrong number (see below). This replaces it with
metrics the platform counts itself, per user.

## Why (the bug this fixes)

One shared claude (CLI) account serves every platform user. claude's
`result.usage` reports that **account's** full token accounting for a call —
dominated by claude's own system prompt + prompt cache (e.g. tokensIn ≈ 34k for
a one-word answer). It is not attributable to the user who asked. The platform
must instead count, **per user, by itself**, what that user actually sent (their
request) and received (the answer).

## The metric (per user, attributed to the authenticated JWT `sub`)

- **Requests** — number of questions (already tracked).
- **Chars in** — Unicode character count of the user's request **text** (their
  typed question, *excluding* the injected `"Read the file at <path>."`
  attachment prefixes).
- **Files** — number of attachments the request carried.
- **File bytes** — total decoded byte size of those attachments (the real
  image/PDF data the user sent).
- **Chars out** — Unicode character count of the answer text returned to the
  user.

"Chars" = `str::chars().count()` (Unicode scalar values), never byte `len()`.
**Dropped entirely:** claude's `tokens_in/out`, `cache_read/creation`, and
`cost` — account-level, not per-user. The existing token columns stay in the DB
(harmless history) but are no longer counted or surfaced.

## Counting (manager only — no worker change)

The manager already holds everything needed in `handle_chat` before it inserts
the row:
- `q_text` (the original question text, `manager/src/main.rs:2266`),
- the `attachments` array (`:2293`), each item carrying base64 `data`,
- and `mark_answered` already receives `answer_text`.

So the manager self-counts:
- **chars_in** = `q_text.chars().count()` — counted at INSERT, on the original
  question (before the `"Read the file at …"` rewrite).
- **files** = the attachments array length.
- **file_bytes** = Σ `b64_decoded_len(att.data)` over the attachments, where a
  PURE helper computes the decoded size from the base64 string length without
  decoding (no base64 dependency):
  ```
  b64_decoded_len(s) = s.len()/4*3 - (number of trailing '=' padding chars)
  ```
  (exact for well-formed base64; 0 for empty).
- **chars_out** = `answer_text.chars().count()` — computed inside `mark_answered`
  from the `answer_text` it already receives.

Counting is best-effort telemetry: it must never fail or delay an answer.

## Schema (manager)

Add nullable columns to `chat_question` via idempotent `ALTER TABLE ADD COLUMN`
(both the SQLite swallow-error and Postgres `IF NOT EXISTS` paths, matching the
existing migration block): `chars_in INTEGER`, `chars_out INTEGER`,
`files INTEGER`, `file_bytes INTEGER`. Historical rows keep NULL → shown as 0/—.
`user_id` (already added) is the attribution key.

- `insert_pending(...)` gains `chars_in`, `files`, `file_bytes` parameters
  (counted in `handle_chat`).
- `mark_answered(...)` additionally sets `chars_out = answer_text.chars().count()`
  in its existing UPDATE.

## Aggregation (manager `/control`)

`/control "usage"` (cumulative) and `/control "usage-daily"` (per-day, 30-day
cutoff — unchanged windowing) switch their SELECTs and reply composers to the
new columns. The `WHERE status IN ('answered','confirmed') AND user_id IS NOT
NULL` filter and the daily `GROUP BY user_id, day` are unchanged.

Reply shapes:
- `usage`: `{ ok, users: [{ userId, requests, charsIn, charsOut, files,
  fileBytes }], totals: { requests, charsIn, charsOut, files, fileBytes } }`.
- `usage-daily`: `{ ok, days: [{ userId, day, charsIn, charsOut, fileBytes }] }`
  (per-day omits per-day `files`/`requests` to keep the chart focused; the
  cumulative panel carries those).

## admin-api

`GET /api/usage` and `GET /api/usage-daily` are unchanged in shape (verbatim
pass-through of the manager reply); only the JSON fields differ. The chat.admin
gate and capability gate stay.

## admin-web

- **Types** (`lib/types.ts`): `UsageRow { userId, requests, charsIn, charsOut,
  files, fileBytes }`; `DailyRow { userId, day, charsIn, charsOut, fileBytes }`.
  Remove the token/cache/cost fields.
- **Formatters** (`components/users/columns.tsx`): keep `fmtTokens` as a generic
  thousands formatter but **rename its export to `fmtCount`** (and update call
  sites); add `fmtBytes(n)` → `"1.2 KB"` / `"3.4 MB"` (1024-based, "—" for
  missing). Drop `fmtCost`.
- **Users table columns:** replace "Tokens in/out" + "Cost" with **Chars in**,
  **Chars out** (right-aligned, `fmtCount`). Header tooltips explain they are
  the platform's own per-user counts (request/answer text), not claude tokens.
- **User detail panel** ("Token usage" → **"Usage"** section): Requests, Chars
  in, Files, File bytes (`fmtBytes`), Chars out.
- **Daily trend** (`components/users/usage-trend.tsx`): `Line`s for **chars-in**
  and **chars-out** on the left axis (compact `fmtCount` ticks), and a `Line`
  for **file-bytes** on the right axis (compact `fmtBytes` ticks) — reusing the
  dual-axis layout the cost line used. Per-day table: Day · Chars in · Chars out
  · File bytes. `buildDailySeries` carries `charsIn, charsOut, fileBytes`.

## Error handling

- Counting failures are impossible by construction (string/array length), but
  the recording UPDATE is best-effort: a failure is logged and never blocks the
  answer (same as the prior `record_usage`).
- Missing/old rows → NULL columns → `COALESCE(...,0)` in aggregates → 0 in UI.
- Best-effort fetch in admin-web (unchanged): a failed `/api/usage*` leaves the
  map empty, "—"/empty chart, page never blanks.

## Testing

- **Manager:** `b64_decoded_len` is exact for padded/unpadded/empty base64
  (pure test); `chars_in`/`chars_out` count Unicode scalars not bytes (e.g. a
  multi-byte string); `insert_pending` records chars_in/files/file_bytes;
  `mark_answered` records chars_out; the `usage`/`usage-daily` aggregates sum the
  new columns per user (+ per day), exclude null-user + non-answered rows.
- **admin-api:** both routes stay chat.admin-gated (existing gate tests cover
  the routes; shapes are pass-through).
- **admin-web:** `fmtBytes` formats KB/MB and "—"; `buildDailySeries` carries the
  new fields; the Users columns + detail + chart render the renamed metrics and
  the empty state.

## Non-goals

- Not tokens — chars + file bytes (claude's exact tokenizer isn't public; the
  only exact token source is the account-level usage we're dropping).
- No claude cost / cache surfaced.
- No per-attachment-type breakdown (just count + total bytes).
- DB keeps the dormant token columns; no destructive migration.
