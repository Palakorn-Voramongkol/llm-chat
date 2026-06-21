# Per-user daily token-usage trend — design

**Status:** designed 2026-06-22; not yet implemented. Extends
[`token-usage-stats.md`](token-usage-stats.md) (the cumulative per-user feature)
with the time-series view it deferred.

**Goal:** In a user's Console detail panel, show their **last-30-day** token
trend — a dual-axis chart of tokens-in, tokens-out, and cost — plus a per-day
breakdown table.

## Source of truth (already present)

Each `chat_question` row carries `time_in` (ISO-8601 string) and the per-answer
usage columns added by the cumulative feature (`tokens_in`, `tokens_out`,
`cache_read_tokens`, `cache_creation_tokens`, `cost_usd`, `user_id`). Daily
buckets are a `GROUP BY user_id, substr(time_in,1,10)`. No schema change.
`recharts ^3.8.1` is already a dependency and the Dashboard already renders
charts (`admin-web/app/(dash)/dashboard/page.tsx`) — that pattern is the model.

## Definitions

- **tokensIn (per user, per day)** = `SUM(tokens_in) + SUM(cache_read_tokens) +
  SUM(cache_creation_tokens)` (the cache-inclusive input footprint, same fold as
  the cumulative feature). **tokensOut** = `SUM(tokens_out)`. **cost** =
  `SUM(cost_usd)`.
- **Window** = the last **30 calendar days** by `time_in` date (inclusive of
  today). Days with no activity are **zero-filled client-side** so the trend
  line is continuous.
- Only attributed rows count (`user_id IS NOT NULL`, `status IN
  ('answered','confirmed')`) — consistent with `/control "usage"`.

## Architecture & data flow

One more `/control` query alongside the cumulative `usage` one:

```
manager  /control "usage-daily"  →  admin-api  GET /api/usage-daily  →  admin-web (chart + table in the user detail panel)
```

### 1. Manager — `/control "usage-daily"` (`manager/src/main.rs`)

A new `ChatDb::usage_daily(cutoff: &str)` method runs (identical SQL text for
both SQLite and Postgres; one bound parameter):

```sql
SELECT user_id, substr(time_in,1,10) AS day,
       COALESCE(SUM(tokens_in),0)            AS input,
       COALESCE(SUM(cache_read_tokens),0)    AS cache_read,
       COALESCE(SUM(cache_creation_tokens),0)AS cache_creation,
       COALESCE(SUM(tokens_out),0)           AS output,
       COALESCE(SUM(cost_usd),0)             AS cost
FROM chat_question
WHERE status IN ('answered','confirmed') AND user_id IS NOT NULL AND time_in >= ?
GROUP BY user_id, day
ORDER BY day
```

The `cutoff` is computed in Rust as `(now − 30 days)` formatted as an ISO-8601
string and bound (ISO-8601 sorts lexically, so the string comparison is correct
and dialect-portable — no SQLite/Postgres date-function divergence).

A pure `compose_daily_reply(rows: &[DailyRow]) -> Value` folds `tokensIn =
input + cache_read + cache_creation` per row and emits:

```json
{ "ok": true, "days": [
  { "userId": "377…", "day": "2026-06-21", "tokensIn": 34505, "tokensOut": 10, "costUsd": 0.1154 }
] }
```

The `/control` dispatch gains a `"usage-daily"` arm (chat.admin-gated, alongside
`"usage"`); on a query error it returns `{"ok":false,"error":…}`.

### 2. admin-api — `GET /api/usage-daily` (`admin-api/src/api/mod.rs`)

An exact mirror of the `usage` handler: `Operator` (chat.admin) gate, the
`manager_control_url` capability check, `mint_chat_token()`, then
`control_query(&url, &token, "usage-daily")` with degrade-on-error.

### 3. admin-web (`admin-web/`)

- A `DailyRow` type (`lib/types.ts`): `{ userId, day, tokensIn, tokensOut, costUsd }`.
- On the Users page, fetch `/api/usage-daily` **best-effort** (the same
  isolated try/catch pattern as `/api/usage`), grouping into
  `dailyByUser: Map<string, DailyRow[]>` keyed by `userId`. A failed fetch
  leaves the map empty — the chart shows its empty state, the page never blanks.
- A pure helper `buildDailySeries(rows: DailyRow[] | undefined): DaySeries[]`
  (in a small `lib/usage-daily.ts`) that **zero-fills** the trailing 30 days:
  returns exactly 30 entries `{ day, tokensIn, tokensOut, costUsd }`, oldest→
  newest, with absent days at 0. (Today is passed in / derived from `new Date()`
  inside the page; the helper takes the rows + an end-date so it stays pure and
  unit-testable.)
- In the detail panel's existing **"Token usage"** `PanelSection`, below the
  cumulative breakdown, render:
  - A **recharts `ComposedChart`** in a `ResponsiveContainer` (mirroring the
    Dashboard's chart setup): `XAxis dataKey="day"`; a **left** `YAxis`
    (`yAxisId="tok"`) with two `Line`s — `tokensIn` and `tokensOut`; a **right**
    `YAxis` (`yAxisId="cost"`, `orientation="right"`) with a `Line` for
    `costUsd` (three `Line`s total — cleaner than fills on a dual axis); a
    `Tooltip` showing all three series; `CartesianGrid`. Heights/margins follow
    the Dashboard chart.
  - A **per-day breakdown table** beneath the chart, **most-recent first**,
    listing only the days that have activity: Day · Tokens in · Tokens out ·
    Cost (reusing `fmtTokens`/`fmtCost`).
  - **Empty state** when the user has no rows in the window: "No usage in the
    last 30 days."

## Error handling

- Best-effort everywhere: a failed `/api/usage-daily` → empty map → chart empty
  state; never blocks the panel or the page (mirrors `/api/usage`).
- Manager query error → `{"ok":false,"error":…}` (the handler degrades like
  `chat_sessions`).
- Charts render client-side only (the page is already a client component;
  `ResponsiveContainer` needs a measured DOM — same as the Dashboard).

## Testing

- **Manager:** `usage_daily` groups by `user_id` + day, honors the `cutoff`
  (rows older than the cutoff are excluded), excludes `user_id IS NULL`, and
  sums per day (SQLite fixture with rows across two days + one stale row);
  `compose_daily_reply` folds `tokensIn` correctly (pure test).
- **admin-api:** `GET /api/usage-daily` is chat.admin-gated (401 without a
  session), mirroring the `/api/usage` gate test.
- **admin-web:** `buildDailySeries` returns 30 entries, zero-fills gaps, places
  rows on the correct day, and is ordered oldest→newest (pure unit test); a
  smoke test that the detail chart/table render with sample data and show the
  empty state otherwise.

## Non-goals

- Per-user only (in the detail panel) — no cross-user overlay chart.
- Fixed 30-day window — no date-range picker.
- No CSV/export; no totals-over-time on the Dashboard (separate spec if wanted).
