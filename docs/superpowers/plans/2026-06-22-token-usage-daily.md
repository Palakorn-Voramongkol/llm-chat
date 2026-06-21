# Per-user Daily Token-Usage Trend — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Show each user's last-30-day token trend (dual-axis chart of tokens-in/out + cost) and a per-day breakdown in their Console detail panel.

**Architecture:** One more manager `/control` query (`usage-daily`, `GROUP BY user_id, day`) → admin-api `GET /api/usage-daily` (mirror of `/api/usage`) → admin-web fetches it best-effort, zero-fills the 30-day window in a pure helper, and renders a recharts `ComposedChart` + table in the existing "Token usage" detail-panel section. No schema change (`time_in` already stored).

**Tech Stack:** Rust (`llm-chat-manager`, `llm-chat-admin-api`; `sqlx` SQLite+Postgres, `chrono`, `serde_json`, `axum`), TypeScript/Next.js (`admin-web`, recharts `^3.8.1`, vitest).

## Global Constraints

- Spec: `docs/superpowers/specs/token-usage-daily.md` (authoritative).
- **tokensIn (per user, per day)** = `input + cache_read + cache_creation`; **tokensOut** = `output`; **cost** = `cost_usd`. The fold is composed in the reply, raw components summed in SQL.
- **Window** = last 30 calendar days by `time_in` date; absent days **zero-filled client-side**. Cutoff = `(now − 30 days)` ISO-8601 string, computed in Rust and bound (ISO-8601 sorts lexically — portable across SQLite/Postgres).
- Only attributed rows: `WHERE status IN ('answered','confirmed') AND user_id IS NOT NULL AND time_in >= :cutoff`.
- Every changed `ChatDb` method implements BOTH the SQLite and Postgres arms.
- Best-effort UI: a failed `/api/usage-daily` fetch shows the chart's empty state, never blocks the page.
- Build/test: manager `cargo test -p llm-chat-manager`; admin-api `cargo test -p llm-chat-admin-api`; admin-web `corepack pnpm -C admin-web test`.

---

## File Structure

- `manager/src/main.rs` — `DailyRow` (`sqlx::FromRow`), `ChatDb::usage_daily(cutoff)`, pure `compose_daily_reply`, `/control "usage-daily"` arm (mirrors the `"usage"` arm ~line 1514; `UserUsage`/`compose_usage_reply`/`usage_by_user` ~lines 92/105/348 are the template; `now_iso` ~1844 shows the chrono usage).
- `admin-api/src/api/mod.rs` — `GET /api/usage-daily` handler + route (exact mirror of `usage` ~line with `async fn usage`).
- `admin-web/lib/types.ts` — `DailyRow` type.
- `admin-web/lib/usage-daily.ts` (new) — pure `buildDailySeries(rows, endDate)`.
- `admin-web/components/users/usage-trend.tsx` (new) — the `ComposedChart` + per-day table + empty state.
- `admin-web/app/(dash)/users/page.tsx` — fetch `/api/usage-daily` into `dailyByUser`, render `<UsageTrend>` in the "Token usage" `PanelSection` (~line 387).

---

### Task 1: Manager — /control "usage-daily" (per-user per-day aggregate)

**Files:**
- Modify: `manager/src/main.rs` (add `DailyRow`, `compose_daily_reply`, `ChatDb::usage_daily`, `"usage-daily"` arm)
- Test: `manager/src/main.rs` (`#[cfg(test)] mod usage_daily_tests`)

**Interfaces:**
- Consumes: `chat_question` usage columns + `time_in`; `ChatDb::insert_pending(..., user_id)`, `update_status`, `record_usage` (existing).
- Produces:
  - `struct DailyRow { user_id: Option<String>, day: String, input: i64, cache_read: i64, cache_creation: i64, output: i64, cost: f64 }` (`sqlx::FromRow`).
  - `ChatDb::usage_daily(&self, cutoff: &str) -> Result<Vec<DailyRow>, sqlx::Error>`.
  - `fn compose_daily_reply(rows: &[DailyRow]) -> serde_json::Value` → `{ ok, days: [{userId, day, tokensIn, tokensOut, costUsd}] }` with `tokensIn = input + cache_read + cache_creation`.
  - `/control` accepts `{"cmd":"usage-daily"}`.

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod usage_daily_tests {
    use super::*;

    #[test]
    fn compose_daily_folds_tokens_in() {
        let rows = vec![
            DailyRow { user_id: Some("u1".into()), day: "2026-06-21".into(),
                       input: 10, cache_read: 100, cache_creation: 20, output: 5, cost: 0.5 },
        ];
        let v = compose_daily_reply(&rows);
        assert_eq!(v["ok"], true);
        assert_eq!(v["days"][0]["userId"], "u1");
        assert_eq!(v["days"][0]["day"], "2026-06-21");
        assert_eq!(v["days"][0]["tokensIn"], 130);   // 10 + 100 + 20
        assert_eq!(v["days"][0]["tokensOut"], 5);
    }

    #[tokio::test]
    async fn usage_daily_groups_by_day_honors_cutoff_excludes_null() {
        use sqlx::sqlite::SqlitePoolOptions;
        let pool = SqlitePoolOptions::new().connect("sqlite::memory:").await.unwrap();
        init_schema_sqlite(&pool).await.unwrap();
        let db = ChatDb::Sqlite(pool);
        // two answered rows for u1 on the same recent day, one on another recent
        // day, one stale (before cutoff), one with NULL user (excluded).
        async fn add(db: &ChatDb, q: &str, ti: &str, user: Option<&str>, tin: i64) {
            let seq = db.insert_pending("c", "s", q, "t", ti, None, user).await.unwrap();
            db.update_status(seq, "answered").await.unwrap();
            db.record_usage(seq, &UsageRow { input: tin, output: 1, cache_read: 0,
                cache_creation: 0, cost: Some(0.1), model: None }).await.unwrap();
        }
        add(&db, "q1", "2026-06-21T10:00:00.000Z", Some("u1"), 10).await;
        add(&db, "q2", "2026-06-21T12:00:00.000Z", Some("u1"), 20).await;
        add(&db, "q3", "2026-06-20T09:00:00.000Z", Some("u1"), 5).await;
        add(&db, "q4", "2026-01-01T00:00:00.000Z", Some("u1"), 999).await; // stale
        add(&db, "q5", "2026-06-21T10:00:00.000Z", None, 7).await;          // null user
        let rows = db.usage_daily("2026-06-15T00:00:00.000Z").await.unwrap();
        // u1 on 06-21 (10+20=30) and 06-20 (5); stale + null excluded.
        assert_eq!(rows.len(), 2);
        let d21 = rows.iter().find(|r| r.day == "2026-06-21").unwrap();
        assert_eq!(d21.input, 30);
        assert!(rows.iter().all(|r| r.user_id.as_deref() == Some("u1")));
        assert!(rows.iter().all(|r| r.day != "2026-01-01"));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p llm-chat-manager usage_daily`
Expected: FAIL — `DailyRow` / `compose_daily_reply` / `usage_daily` not found.

- [ ] **Step 3: Write minimal implementation**

Add near `UserUsage`/`compose_usage_reply` (~line 92):

```rust
#[derive(Debug, Clone, sqlx::FromRow)]
struct DailyRow {
    user_id: Option<String>,
    day: String,
    input: i64,
    cache_read: i64,
    cache_creation: i64,
    output: i64,
    cost: f64,
}

/// PURE: build the /control "usage-daily" reply. tokensIn folds the cache
/// components into the input footprint, per (user, day).
fn compose_daily_reply(rows: &[DailyRow]) -> serde_json::Value {
    let days: Vec<serde_json::Value> = rows.iter().map(|r| serde_json::json!({
        "userId": r.user_id,
        "day": r.day,
        "tokensIn": r.input + r.cache_read + r.cache_creation,
        "tokensOut": r.output,
        "costUsd": r.cost,
    })).collect();
    serde_json::json!({ "ok": true, "days": days })
}
```

Add the query method to `impl ChatDb` (next to `usage_by_user` ~line 348). The
day is `substr(time_in,1,10)` = the `YYYY-MM-DD` prefix of the ISO timestamp:

```rust
    async fn usage_daily(&self, cutoff: &str) -> Result<Vec<DailyRow>, sqlx::Error> {
        let sql = "SELECT user_id, substr(time_in,1,10) AS day,
                     COALESCE(SUM(tokens_in),0) AS input,
                     COALESCE(SUM(cache_read_tokens),0) AS cache_read,
                     COALESCE(SUM(cache_creation_tokens),0) AS cache_creation,
                     COALESCE(SUM(tokens_out),0) AS output,
                     COALESCE(SUM(cost_usd),0) AS cost
                   FROM chat_question
                   WHERE status IN ('answered','confirmed')
                     AND user_id IS NOT NULL AND time_in >= ?
                   GROUP BY user_id, day
                   ORDER BY day";
        match self {
            ChatDb::Sqlite(p) => sqlx::query_as::<_, DailyRow>(sql).bind(cutoff).fetch_all(p).await,
            ChatDb::Postgres(p) => {
                // Postgres uses $1, not ?, for the bind placeholder.
                let pg = sql.replace("time_in >= ?", "time_in >= $1");
                sqlx::query_as::<_, DailyRow>(&pg).bind(cutoff).fetch_all(p).await
            }
        }
    }
```

Add the `/control` arm next to `"usage"` (~line 1514). The cutoff is 30 days ago
in the same ISO format `time_in` uses (`now_iso`, ~1844):

```rust
            "usage-daily" => {
                let cutoff = (chrono::Utc::now() - chrono::Duration::days(30))
                    .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
                let db = state.lock().await.chat_db.clone();
                match db.usage_daily(&cutoff).await {
                    Ok(rows) => compose_daily_reply(&rows),
                    Err(e) => serde_json::json!({"ok": false, "error": format!("usage-daily query: {e}")}),
                }
            }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p llm-chat-manager usage_daily && cargo test -p llm-chat-manager`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add manager/src/main.rs
git commit -m "feat(manager): /control usage-daily — per-user per-day token aggregate"
```

---

### Task 2: admin-api — GET /api/usage-daily

**Files:**
- Modify: `admin-api/src/api/mod.rs` (route + `usage_daily` handler, mirroring `usage`)
- Test: `admin-api/src/api/mod.rs` (gate test, mirroring `usage_route_requires_operator`)

**Interfaces:**
- Consumes (Task 1): `/control {"cmd":"usage-daily"}` → `{ok, days:[…]}`.
- Produces: `GET /api/usage-daily`, `Operator` (chat.admin) gated, returning the manager reply verbatim.

- [ ] **Step 1: Write the failing test**

Mirror the existing `usage_route_requires_operator` test (in `mod contract_tests`), reusing its `test_router_no_session()` helper:

```rust
    #[tokio::test]
    async fn usage_daily_route_requires_operator() {
        let app = test_router_no_session();
        let res = app.oneshot(
            axum::http::Request::builder().uri("/api/usage-daily")
                .body(axum::body::Body::empty()).unwrap()
        ).await.unwrap();
        assert_eq!(res.status(), axum::http::StatusCode::UNAUTHORIZED);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p llm-chat-admin-api usage_daily_route`
Expected: FAIL — route not found → 404 (not 401).

- [ ] **Step 3: Write minimal implementation**

Register the route next to `/api/usage`:

```rust
        .route("/api/usage-daily", get(usage_daily))
```

Add the handler next to `usage` (exact mirror, only the command string and the
unconfigured fallback differ):

```rust
/// Per-user per-day token usage (last 30 days) from /control "usage-daily".
/// chat.admin-gated, capability-gated on MANAGER_CONTROL_URL — mirrors usage().
async fn usage_daily(_op: Operator, State(st): State<AppState>) -> Result<Json<Value>, ApiError> {
    let Some(url) = st.cfg.manager_control_url.clone() else {
        return Ok(Json(json!({ "configured": false, "days": [] })));
    };
    let token = st.zitadel.mint_chat_token().await?;
    Ok(Json(
        crate::manager::control_query(&url, &token, "usage-daily")
            .await
            .unwrap_or_else(|e| json!({ "ok": false, "error": e })),
    ))
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p llm-chat-admin-api usage_daily_route && cargo test -p llm-chat-admin-api`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add admin-api/src/api/mod.rs
git commit -m "feat(admin-api): GET /api/usage-daily proxying manager /control usage-daily"
```

---

### Task 3: admin-web — DailyRow type + zero-filling buildDailySeries

**Files:**
- Modify: `admin-web/lib/types.ts` (add `DailyRow`, `UsageDailyResponse`)
- Create: `admin-web/lib/usage-daily.ts` (pure `buildDailySeries`)
- Test: `admin-web/lib/usage-daily.test.ts`

**Interfaces:**
- Consumes (Task 2): `GET /api/usage-daily` → `{ ok, days: DailyRow[] }`.
- Produces:
  - `interface DailyRow { userId: string | null; day: string; tokensIn: number; tokensOut: number; costUsd: number }`.
  - `interface DaySeries { day: string; tokensIn: number; tokensOut: number; costUsd: number }`.
  - `function buildDailySeries(rows: DailyRow[] | undefined, endDate: Date): DaySeries[]` — exactly 30 entries, oldest→newest, zero-filled, rows placed on their `day`.

- [ ] **Step 1: Write the failing test**

```ts
import { describe, it, expect } from "vitest";
import { buildDailySeries } from "@/lib/usage-daily";
import type { DailyRow } from "@/lib/types";

describe("buildDailySeries", () => {
  const end = new Date("2026-06-21T12:00:00Z");
  it("returns 30 entries, oldest first, ending on endDate", () => {
    const s = buildDailySeries([], end);
    expect(s).toHaveLength(30);
    expect(s[0].day).toBe("2026-05-23");
    expect(s[29].day).toBe("2026-06-21");
  });
  it("zero-fills missing days and places rows on the right day", () => {
    const rows: DailyRow[] = [
      { userId: "u1", day: "2026-06-21", tokensIn: 130, tokensOut: 5, costUsd: 0.5 },
      { userId: "u1", day: "2026-06-19", tokensIn: 40, tokensOut: 2, costUsd: 0.1 },
    ];
    const s = buildDailySeries(rows, end);
    const byDay = Object.fromEntries(s.map((d) => [d.day, d]));
    expect(byDay["2026-06-21"].tokensIn).toBe(130);
    expect(byDay["2026-06-19"].tokensOut).toBe(2);
    expect(byDay["2026-06-20"]).toEqual({ day: "2026-06-20", tokensIn: 0, tokensOut: 0, costUsd: 0 });
  });
  it("undefined rows -> all zeros", () => {
    const s = buildDailySeries(undefined, end);
    expect(s.every((d) => d.tokensIn === 0 && d.tokensOut === 0 && d.costUsd === 0)).toBe(true);
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `corepack pnpm -C admin-web test -- usage-daily`
Expected: FAIL — cannot resolve `@/lib/usage-daily`.

- [ ] **Step 3: Write minimal implementation**

`admin-web/lib/types.ts` (add):

```ts
export interface DailyRow {
  userId: string | null;
  day: string;        // "YYYY-MM-DD"
  tokensIn: number;
  tokensOut: number;
  costUsd: number;
}
export interface UsageDailyResponse {
  ok?: boolean;
  configured?: boolean;
  days?: DailyRow[];
}
```

`admin-web/lib/usage-daily.ts` (new):

```ts
import type { DailyRow } from "@/lib/types";

export interface DaySeries {
  day: string;
  tokensIn: number;
  tokensOut: number;
  costUsd: number;
}

/** UTC "YYYY-MM-DD" for a Date. */
function dayKey(d: Date): string {
  return d.toISOString().slice(0, 10);
}

/**
 * The trailing 30 days ending on `endDate` (inclusive), oldest -> newest, with
 * each `rows` entry placed on its `day` and every other day zero-filled. Pure:
 * `endDate` is passed in so the result is deterministic and unit-testable.
 */
export function buildDailySeries(rows: DailyRow[] | undefined, endDate: Date): DaySeries[] {
  const byDay = new Map<string, DailyRow>();
  for (const r of rows ?? []) byDay.set(r.day, r);
  const out: DaySeries[] = [];
  for (let i = 29; i >= 0; i--) {
    const d = new Date(endDate);
    d.setUTCDate(d.getUTCDate() - i);
    const key = dayKey(d);
    const r = byDay.get(key);
    out.push({
      day: key,
      tokensIn: r?.tokensIn ?? 0,
      tokensOut: r?.tokensOut ?? 0,
      costUsd: r?.costUsd ?? 0,
    });
  }
  return out;
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `corepack pnpm -C admin-web test -- usage-daily`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add admin-web/lib/types.ts admin-web/lib/usage-daily.ts admin-web/lib/usage-daily.test.ts
git commit -m "feat(admin-web): DailyRow type + zero-filling buildDailySeries"
```

---

### Task 4: admin-web — UsageTrend chart/table + Users-page wiring

**Files:**
- Create: `admin-web/components/users/usage-trend.tsx` (chart + table + empty state)
- Modify: `admin-web/app/(dash)/users/page.tsx` (fetch `/api/usage-daily` into `dailyByUser`; render `<UsageTrend>` in the "Token usage" `PanelSection` ~line 387)
- Test: `admin-web/components/users/usage-trend.test.tsx`

**Interfaces:**
- Consumes (Task 3): `buildDailySeries`, `DailyRow`, `DaySeries`; (Task 2) `/api/usage-daily`.
- Produces: `<UsageTrend rows={DailyRow[] | undefined} endDate={Date} />` rendering a dual-axis `ComposedChart` + a per-day table, or an empty state when no day has activity.

- [ ] **Step 1: Write the failing test**

```tsx
import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { UsageTrend } from "@/components/users/usage-trend";
import type { DailyRow } from "@/lib/types";

describe("UsageTrend", () => {
  const end = new Date("2026-06-21T12:00:00Z");
  it("shows the empty state when no day has activity", () => {
    render(<UsageTrend rows={undefined} endDate={end} />);
    expect(screen.getByText(/no usage in the last 30 days/i)).toBeInTheDocument();
  });
  it("renders a per-day table row for an active day", () => {
    const rows: DailyRow[] = [
      { userId: "u1", day: "2026-06-21", tokensIn: 34505, tokensOut: 10, costUsd: 0.1154 },
    ];
    render(<UsageTrend rows={rows} endDate={end} />);
    expect(screen.getByText("2026-06-21")).toBeInTheDocument();
    expect(screen.getByText("34,505")).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `corepack pnpm -C admin-web test -- usage-trend`
Expected: FAIL — cannot resolve `@/components/users/usage-trend`.

- [ ] **Step 3: Write minimal implementation**

`admin-web/components/users/usage-trend.tsx` (new). Uses recharts (already a dep; the Dashboard imports the same way) and the exported `fmtTokens`/`fmtCost`:

```tsx
"use client";
import {
  CartesianGrid, ComposedChart, Line, ResponsiveContainer, Tooltip, XAxis, YAxis,
} from "recharts";
import type { DailyRow } from "@/lib/types";
import { buildDailySeries } from "@/lib/usage-daily";
import { fmtTokens, fmtCost } from "@/components/users/columns";

export function UsageTrend({ rows, endDate }: { rows: DailyRow[] | undefined; endDate: Date }) {
  const series = buildDailySeries(rows, endDate);
  const active = series.filter((d) => d.tokensIn || d.tokensOut || d.costUsd);
  if (active.length === 0) {
    return <p className="text-muted-foreground text-sm">No usage in the last 30 days.</p>;
  }
  const mmdd = (day: string) => day.slice(5); // "MM-DD"
  return (
    <div className="space-y-3">
      <ResponsiveContainer width="100%" height={200}>
        <ComposedChart data={series} margin={{ top: 4, right: 8, left: -16, bottom: 0 }}>
          <CartesianGrid strokeDasharray="3 3" vertical={false} />
          <XAxis dataKey="day" tickFormatter={mmdd} tick={{ fontSize: 11 }} tickLine={false} axisLine={false} minTickGap={24} />
          <YAxis yAxisId="tok" tick={{ fontSize: 11 }} tickLine={false} axisLine={false} width={48} />
          <YAxis yAxisId="cost" orientation="right" tick={{ fontSize: 11 }} tickLine={false} axisLine={false} width={48}
                 tickFormatter={(v: number) => `$${v.toFixed(2)}`} />
          <Tooltip
            formatter={(value: number, name: string) =>
              name === "Cost" ? fmtCost(value) : fmtTokens(value)}
            labelFormatter={(d: string) => d} />
          <Line yAxisId="tok" type="monotone" dataKey="tokensIn" name="Tokens in" stroke="#5b53e8" dot={false} strokeWidth={2} />
          <Line yAxisId="tok" type="monotone" dataKey="tokensOut" name="Tokens out" stroke="#10b981" dot={false} strokeWidth={2} />
          <Line yAxisId="cost" type="monotone" dataKey="costUsd" name="Cost" stroke="#f59e0b" dot={false} strokeWidth={2} />
        </ComposedChart>
      </ResponsiveContainer>
      <table className="w-full text-sm">
        <thead>
          <tr className="text-muted-foreground text-left">
            <th className="font-medium">Day</th>
            <th className="font-medium text-right">Tokens in</th>
            <th className="font-medium text-right">Tokens out</th>
            <th className="font-medium text-right">Cost</th>
          </tr>
        </thead>
        <tbody>
          {[...active].reverse().map((d) => (
            <tr key={d.day}>
              <td>{d.day}</td>
              <td className="text-right tabular-nums">{fmtTokens(d.tokensIn)}</td>
              <td className="text-right tabular-nums">{fmtTokens(d.tokensOut)}</td>
              <td className="text-right tabular-nums">{fmtCost(d.costUsd)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
```

`admin-web/app/(dash)/users/page.tsx` — wire the fetch + render:

1. Add state next to `usageByUser` (~line 53):
   ```ts
   const [dailyByUser, setDailyByUser] = useState<Map<string, DailyRow[]>>(new Map());
   ```
   and import `DailyRow` from `@/lib/types` (the file already imports `UsageRow` etc.).
2. In `load()`, after the existing best-effort `/api/usage` fetch block, add a sibling best-effort block (mirror it exactly — its own `try/catch`, a failure leaves the map empty):
   ```ts
   try {
     const r = await api.get<UsageDailyResponse>("/api/usage-daily");
     const m = new Map<string, DailyRow[]>();
     for (const row of r.days ?? []) {
       const list = m.get(row.userId ?? "") ?? [];
       list.push(row);
       if (row.userId) m.set(row.userId, list);
     }
     setDailyByUser(m);
   } catch { setDailyByUser(new Map()); }
   ```
3. In the "Token usage" `PanelSection` (~line 387), after the cumulative `PanelField`s, render the trend:
   ```tsx
   <div className="pt-3">
     <UsageTrend rows={dailyByUser.get(selected.id)} endDate={new Date()} />
   </div>
   ```
   Import `UsageTrend` from `@/components/users/usage-trend` and `UsageDailyResponse` from `@/lib/types`.

- [ ] **Step 4: Run test to verify it passes**

Run: `corepack pnpm -C admin-web test -- usage-trend` then `corepack pnpm -C admin-web build`
Expected: tests PASS; the page compiles. Render-testing is already configured (`@testing-library/react`, `jsdom` env, `vitest.setup.ts` imports `@testing-library/jest-dom/vitest`), so `render`/`screen` work as written. recharts' `ResponsiveContainer` measures to 0×0 under jsdom (the chart paths may not draw), but the **table rows and empty-state text** are the asserted contract and render regardless.

- [ ] **Step 5: Commit**

```bash
git add admin-web/components/users/usage-trend.tsx admin-web/components/users/usage-trend.test.tsx admin-web/app/(dash)/users/page.tsx
git commit -m "feat(admin-web): per-user daily token trend chart + table in the detail panel"
```

---

## Final integration check (after Task 4)

- [ ] Rebuild + recreate the changed images and the native worker: `docker compose build manager admin-api admin-web && docker compose up -d --no-deps --force-recreate manager admin-api admin-web` (verify image timestamps are fresh — if BuildKit cache-hits stale, use `--no-cache`).
- [ ] Drive ≥1 question (`llm-chat ask --send "hi" --manager ws://127.0.0.1:7777/chat`); `/control "usage-daily"` (via a chat.admin token) returns a `days` entry for today with non-zero `tokensIn`.
- [ ] In the Console, open a user with usage → the detail panel shows the trend chart (3 lines, dual axis) + the per-day table; a user with no usage shows "No usage in the last 30 days."
- [ ] A `chat.user`-only token cannot reach `/api/usage-daily` (401/403).
