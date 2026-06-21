# Self-counted Per-user Usage (chars + file bytes) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace claude's account-level `result.usage` with platform-counted per-user metrics — chars-in (request text), files + file-bytes (attachments), chars-out (answer text) — across the manager aggregates and the Console.

**Architecture:** The manager self-counts at the point it already holds the data (`handle_chat`: request text + attachment payloads; `mark_answered`: answer text), stores it in new `chat_question` columns, and the existing `/control "usage"`/`"usage-daily"` aggregates + admin-web surfaces switch to those columns. admin-api handlers are verbatim pass-throughs and need no change.

**Tech Stack:** Rust (`llm-chat-manager`; `sqlx` SQLite+Postgres, `serde_json`), TypeScript/Next.js (`admin-web`, recharts, vitest).

## Global Constraints

- Spec: `docs/superpowers/specs/self-counted-usage.md` (authoritative).
- Metric, per user, self-counted: **Requests**, **Chars in** (request text), **Files** (#attachments), **File bytes** (decoded attachment size), **Chars out** (answer text). Claude tokens/cache/cost are **dropped** from counting + surfaces (DB columns stay, dormant).
- "Chars" = `str::chars().count()` (Unicode scalars), NEVER byte `len()`. `chars_in` excludes the injected `"Read the file at …"` prefixes (count the original question).
- `file_bytes` = decoded base64 size via the pure formula `s.len()/4*3 - (trailing '=' count)` (no base64 dep); 0 for empty.
- Both SQLite and Postgres arms for every `ChatDb` change. Idempotent `ALTER TABLE ADD COLUMN` (SQLite swallow-error; Postgres `IF NOT EXISTS`).
- Counting/recording is best-effort telemetry: never fail or delay an answer.
- Reply field names (the cross-layer contract): cumulative `usage` → `{userId, requests, charsIn, charsOut, files, fileBytes}` (+ matching `totals`); daily `usage-daily` → `{userId, day, charsIn, charsOut, fileBytes}`.
- Build/test: manager `cargo test -p llm-chat-manager`; admin-web `corepack pnpm -C admin-web test` + `corepack pnpm -C admin-web build`. admin-api unchanged: `cargo test -p llm-chat-admin-api` must still pass.

---

## File Structure

- `manager/src/main.rs` — pure `b64_decoded_len`; new columns in the schema ALTER block; `insert_pending` gains `chars_in/files/file_bytes`; `mark_answered` sets `chars_out`; `handle_chat` counts + passes them; `UserUsage`/`usage_by_user`/`compose_usage_reply` and `DailyRow`/`usage_daily`/`compose_daily_reply` reworked to the new columns.
- `admin-web/lib/types.ts`, `admin-web/lib/usage-daily.ts`, `admin-web/components/users/columns.tsx`, `admin-web/components/users/usage-trend.tsx`, `admin-web/app/(dash)/users/page.tsx` — the metric rename across types, formatters, columns, detail panel, and the daily chart.
- **No admin-api change** — `usage`/`usage_daily` handlers forward the manager reply verbatim.

---

### Task 1: Manager — self-count chars/files/bytes at insert + answer

**Files:**
- Modify: `manager/src/main.rs` (pure `b64_decoded_len`; schema ALTER block ~line 497; `insert_pending` ~170; `mark_answered` ~299; `handle_chat` count site ~2360 + insert caller)
- Test: `manager/src/main.rs` (`#[cfg(test)] mod self_count_tests`)

**Interfaces:**
- Produces:
  - `fn b64_decoded_len(s: &str) -> i64` (pure).
  - `chat_question` columns `chars_in INTEGER, chars_out INTEGER, files INTEGER, file_bytes INTEGER`.
  - `insert_pending(connection_id, sid, q_id, text, time_in, attachment_paths_json, user_id, chars_in: i64, files: i64, file_bytes: i64) -> Result<i64, sqlx::Error>` (3 new trailing params).
  - `mark_answered` additionally writes `chars_out = answer_text.chars().count()`.

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod self_count_tests {
    use super::*;

    #[test]
    fn b64_decoded_len_is_exact() {
        assert_eq!(b64_decoded_len(""), 0);
        assert_eq!(b64_decoded_len("YWJj"), 3);        // "abc", no padding
        assert_eq!(b64_decoded_len("YWJjZA=="), 4);    // "abcd", 2 pad
        assert_eq!(b64_decoded_len("YWJjZGU="), 5);    // "abcde", 1 pad
    }

    #[tokio::test]
    async fn insert_and_answer_record_self_counts_unicode() {
        use sqlx::sqlite::SqlitePoolOptions;
        let pool = SqlitePoolOptions::new().connect("sqlite::memory:").await.unwrap();
        init_schema_sqlite(&pool).await.unwrap();
        let db = ChatDb::Sqlite(pool);
        // chars are Unicode scalars: "héllo" is 5 chars (6 bytes).
        let seq = db.insert_pending("c","s","q","héllo","now",None,Some("u1"),
                                    "héllo".chars().count() as i64, 2, 1500).await.unwrap();
        db.mark_answered(seq, "wörld", "now2").await.unwrap();
        let row: (Option<i64>, Option<i64>, Option<i64>, Option<i64>) = match &db {
            ChatDb::Sqlite(p) => sqlx::query_as(
                "SELECT chars_in, files, file_bytes, chars_out FROM chat_question WHERE seq=?")
                .bind(seq).fetch_one(p).await.unwrap(), _ => unreachable!() };
        assert_eq!(row, (Some(5), Some(2), Some(1500), Some(5))); // chars_out "wörld"=5
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p llm-chat-manager self_count`
Expected: FAIL — `b64_decoded_len` not found; `insert_pending` arity; `no column named chars_in`.

- [ ] **Step 3: Write minimal implementation**

Add the pure helper near the other free functions (e.g. above `now_iso`):

```rust
/// PURE: decoded byte length of a base64 string, computed from its length (no
/// decode, no dependency). Exact for well-formed base64; 0 for empty.
fn b64_decoded_len(s: &str) -> i64 {
    if s.is_empty() { return 0; }
    let pad = s.bytes().rev().take_while(|&b| b == b'=').count();
    (s.len() / 4 * 3 - pad) as i64
}
```

In the schema ALTER block (after the `ADD COLUMN user_id`/token lines ~497), add to the SQLite `for col in [...]` list (swallow-error) **and** the Postgres list (`IF NOT EXISTS`):

```rust
// SQLite list — append:
        "ALTER TABLE chat_question ADD COLUMN chars_in INTEGER;",
        "ALTER TABLE chat_question ADD COLUMN chars_out INTEGER;",
        "ALTER TABLE chat_question ADD COLUMN files INTEGER;",
        "ALTER TABLE chat_question ADD COLUMN file_bytes INTEGER;",
// Postgres list — append:
        "ALTER TABLE chat_question ADD COLUMN IF NOT EXISTS chars_in BIGINT;",
        "ALTER TABLE chat_question ADD COLUMN IF NOT EXISTS chars_out BIGINT;",
        "ALTER TABLE chat_question ADD COLUMN IF NOT EXISTS files BIGINT;",
        "ALTER TABLE chat_question ADD COLUMN IF NOT EXISTS file_bytes BIGINT;",
```

Extend `insert_pending` (~170): add `chars_in: i64, files: i64, file_bytes: i64` after `user_id`; add the three columns + placeholders + binds in BOTH arms. SQLite arm columns/values become:

```rust
                    "INSERT INTO chat_question
                     (connection_id, sid, q_id, text, time_in, status, attachment_paths, user_id,
                      chars_in, files, file_bytes)
                     VALUES (?, ?, ?, ?, ?, 'pending', ?, ?, ?, ?, ?)",
                )
                .bind(connection_id).bind(sid).bind(q_id).bind(text).bind(time_in)
                .bind(attachment_paths_json).bind(user_id)
                .bind(chars_in).bind(files).bind(file_bytes)
```

(Postgres arm: add `, chars_in, files, file_bytes` to columns and `, $8, $9, $10` before `RETURNING seq`, then the three `.bind`s.)

Extend `mark_answered` (~299) to also set `chars_out` — compute it from `answer_text` and add to the UPDATE (both arms). SQLite:

```rust
                sqlx::query(
                    "UPDATE chat_question SET answer_text = ?, status = 'answered',
                     time_out = ?, chars_out = ? WHERE seq = ?",
                )
                .bind(answer_text).bind(time_out)
                .bind(answer_text.chars().count() as i64).bind(seq)
```

(Postgres: `chars_out = $3 WHERE seq = $4`, bind `answer_text.chars().count() as i64` then `seq`.)

Wire `handle_chat` (the INSERT site ~2360, where `q_text`, `attachments`, `saved_paths`, `final_text` are in scope). Count BEFORE `insert_pending` and pass:

```rust
            let chars_in = q_text.chars().count() as i64;
            let atts = v.get("attachments").and_then(|x| x.as_array());
            let files = atts.map(|a| a.len() as i64).unwrap_or(0);
            let file_bytes: i64 = atts.map(|a| a.iter()
                .filter_map(|att| att.get("data").and_then(|d| d.as_str()))
                .map(b64_decoded_len).sum()).unwrap_or(0);
```

and add `chars_in, files, file_bytes` as the last three arguments to the existing `insert_pending(...)` call. (Any other `insert_pending` call sites the compiler flags — e.g. tests — pass `0, 0, 0`.)

Also **remove the claude-usage recording** at the answer-pairing site (the prior feature's block, just after the `mark_answered` call):

```rust
    if let Some(usage) = UsageRow::from_qa(&qa) {
        if let Err(e) = db.record_usage(seq, &usage).await { ... }
    }
```

Delete that block — `chars_out` is now written by `mark_answered`, and claude's `usage` is no longer recorded. (This makes `UsageRow`/`record_usage`/`UsageRow::from_qa` dead; Task 2 removes them and their tests.)

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p llm-chat-manager self_count && cargo test -p llm-chat-manager`
Expected: PASS (fix other `insert_pending` callers with `0,0,0`; the existing `record_usage`/usage tests still compile — they are reworked in Task 2).

- [ ] **Step 5: Commit**

```bash
git add manager/src/main.rs
git commit -m "feat(manager): self-count chars/files/file_bytes per question"
```

---

### Task 2: Manager — aggregate the self-counted metrics

**Files:**
- Modify: `manager/src/main.rs` (`UserUsage`/`usage_by_user`/`compose_usage_reply`; `DailyRow`/`usage_daily`/`compose_daily_reply`; update their existing tests)
- Test: `manager/src/main.rs` (rework `usage_agg_tests` + `usage_daily_tests`)

**Interfaces:**
- Consumes (Task 1): the `chars_in/chars_out/files/file_bytes` columns.
- Produces:
  - `usage` reply: `{ ok, users:[{userId, requests, charsIn, charsOut, files, fileBytes}], totals:{requests, charsIn, charsOut, files, fileBytes} }`.
  - `usage-daily` reply: `{ ok, days:[{userId, day, charsIn, charsOut, fileBytes}] }`.

- [ ] **Step 1: Update the failing tests**

Replace the bodies of `usage_agg_tests::compose_sums_components_and_totals` and `usage_daily_tests::compose_daily_folds_tokens_in` (and the DB aggregate tests) to assert the new shape. New cumulative-compose test:

```rust
    #[test]
    fn compose_usage_sums_chars_and_bytes() {
        let rows = vec![
            UserUsage { user_id: Some("u1".into()), requests: 2, chars_in: 100, chars_out: 50,
                        files: 1, file_bytes: 2048, last_used: Some("t2".into()) },
            UserUsage { user_id: Some("u2".into()), requests: 1, chars_in: 10, chars_out: 5,
                        files: 0, file_bytes: 0, last_used: Some("t1".into()) },
        ];
        let v = compose_usage_reply(&rows);
        assert_eq!(v["users"][0]["charsIn"], 100);
        assert_eq!(v["users"][0]["fileBytes"], 2048);
        assert_eq!(v["totals"]["charsIn"], 110);
        assert_eq!(v["totals"]["charsOut"], 55);
        assert_eq!(v["totals"]["fileBytes"], 2048);
    }
```

New daily-compose test:

```rust
    #[test]
    fn compose_daily_emits_chars_and_bytes() {
        let rows = vec![ DailyRow { user_id: Some("u1".into()), day: "2026-06-21".into(),
            chars_in: 100, chars_out: 50, file_bytes: 2048 } ];
        let v = compose_daily_reply(&rows);
        assert_eq!(v["days"][0]["charsIn"], 100);
        assert_eq!(v["days"][0]["charsOut"], 50);
        assert_eq!(v["days"][0]["fileBytes"], 2048);
    }
```

Update the two DB aggregate tests (`usage_by_user_groups_and_excludes_pending`, `usage_daily_groups_by_day_honors_cutoff_excludes_null`) to insert via the new `insert_pending(...,chars_in,files,file_bytes)` signature and assert `chars_in`/`file_bytes` instead of `input`/token fields (use `mark_answered` for `chars_out`; drop the `record_usage` calls).

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p llm-chat-manager usage_agg usage_daily`
Expected: FAIL — `UserUsage`/`DailyRow` field mismatch; reply keys absent.

- [ ] **Step 3: Write minimal implementation**

`UserUsage` → new fields:

```rust
#[derive(Debug, Clone, sqlx::FromRow)]
struct UserUsage {
    user_id: Option<String>,
    requests: i64,
    chars_in: i64,
    chars_out: i64,
    files: i64,
    file_bytes: i64,
    last_used: Option<String>,
}
```

`compose_usage_reply` body:

```rust
fn compose_usage_reply(rows: &[UserUsage]) -> serde_json::Value {
    let mut users = Vec::with_capacity(rows.len());
    let (mut treq, mut tin, mut tout, mut tf, mut tb) = (0i64, 0i64, 0i64, 0i64, 0i64);
    for r in rows {
        treq += r.requests; tin += r.chars_in; tout += r.chars_out; tf += r.files; tb += r.file_bytes;
        users.push(serde_json::json!({
            "userId": r.user_id, "requests": r.requests,
            "charsIn": r.chars_in, "charsOut": r.chars_out,
            "files": r.files, "fileBytes": r.file_bytes, "lastUsed": r.last_used,
        }));
    }
    serde_json::json!({ "ok": true, "users": users,
        "totals": { "requests": treq, "charsIn": tin, "charsOut": tout, "files": tf, "fileBytes": tb } })
}
```

`usage_by_user` SELECT (both arms share the text):

```rust
        let sql = "SELECT user_id,
                     COUNT(*) AS requests,
                     COALESCE(SUM(chars_in),0) AS chars_in,
                     COALESCE(SUM(chars_out),0) AS chars_out,
                     COALESCE(SUM(files),0) AS files,
                     COALESCE(SUM(file_bytes),0) AS file_bytes,
                     MAX(time_out) AS last_used
                   FROM chat_question
                   WHERE status IN ('answered','confirmed') AND user_id IS NOT NULL
                   GROUP BY user_id";
```

`DailyRow` → new fields + `compose_daily_reply` + `usage_daily` SELECT:

```rust
#[derive(Debug, Clone, sqlx::FromRow)]
struct DailyRow {
    user_id: Option<String>,
    day: String,
    chars_in: i64,
    chars_out: i64,
    file_bytes: i64,
}

fn compose_daily_reply(rows: &[DailyRow]) -> serde_json::Value {
    let days: Vec<serde_json::Value> = rows.iter().map(|r| serde_json::json!({
        "userId": r.user_id, "day": r.day,
        "charsIn": r.chars_in, "charsOut": r.chars_out, "fileBytes": r.file_bytes,
    })).collect();
    serde_json::json!({ "ok": true, "days": days })
}
```

```rust
        let sql = "SELECT user_id, substr(time_in,1,10) AS day,
                     COALESCE(SUM(chars_in),0) AS chars_in,
                     COALESCE(SUM(chars_out),0) AS chars_out,
                     COALESCE(SUM(file_bytes),0) AS file_bytes
                   FROM chat_question
                   WHERE status IN ('answered','confirmed')
                     AND user_id IS NOT NULL AND time_in >= ?
                   GROUP BY user_id, day
                   ORDER BY day";
```

(Keep the existing Postgres `?`→`$1` replace in `usage_daily`.) The `record_usage`/`UsageRow` machinery and the token columns are now unused by the aggregates — leave `record_usage`/`UsageRow` in place only if still referenced; otherwise delete them and their tests to keep the build warning-free.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p llm-chat-manager`
Expected: PASS (remove any now-dead `record_usage`/`UsageRow` + its test if the compiler flags it unused).

- [ ] **Step 5: Commit**

```bash
git add manager/src/main.rs
git commit -m "feat(manager): aggregate self-counted chars/files/bytes in /control usage[-daily]"
```

---

### Task 3: admin-web — surface the self-counted metrics

**Files:**
- Modify: `admin-web/lib/types.ts`, `admin-web/lib/usage-daily.ts` (+ `usage-daily.test.ts`), `admin-web/components/users/columns.tsx`, `admin-web/components/users/usage-trend.tsx` (+ `usage-trend.test.tsx`), `admin-web/app/(dash)/users/page.tsx`
- Test: `admin-web/components/users/columns.test.tsx` (formatters), `usage-daily.test.ts`, `usage-trend.test.tsx`

**Interfaces:**
- Consumes (Task 2): `/api/usage` → `{users:[{userId,requests,charsIn,charsOut,files,fileBytes}],totals}`; `/api/usage-daily` → `{days:[{userId,day,charsIn,charsOut,fileBytes}]}`.
- Produces: `fmtCount` (renamed from `fmtTokens`), `fmtBytes`; `UsageRow`/`DailyRow`/`DaySeries` with the new fields; Chars in/out columns; detail = Requests/Chars in/Files/File bytes/Chars out; chart = chars-in/out (left) + file-bytes (right).

- [ ] **Step 1: Write the failing tests**

`columns.test.tsx` — replace the `fmtCost` test with `fmtBytes`, and rename `fmtTokens`→`fmtCount`:

```tsx
import { fmtCount, fmtBytes } from "@/components/users/columns";
it("fmtCount formats thousands", () => {
  expect(fmtCount(123456)).toBe("123,456");
  expect(fmtCount(undefined)).toBe("—");
});
it("fmtBytes formats KB/MB", () => {
  expect(fmtBytes(undefined)).toBe("—");
  expect(fmtBytes(0)).toBe("0 B");
  expect(fmtBytes(1536)).toBe("1.5 KB");
  expect(fmtBytes(5_242_880)).toBe("5.0 MB");
});
```

`usage-daily.test.ts` — update `DailyRow` fixtures + assertions to `charsIn/charsOut/fileBytes` (e.g. `{ userId:"u1", day:"2026-06-21", charsIn:100, charsOut:50, fileBytes:2048 }`, assert `byDay["2026-06-21"].charsIn === 100`, zero-fill → `{day,charsIn:0,charsOut:0,fileBytes:0}`).

`usage-trend.test.tsx` — update the active-day row assertion to chars/bytes (`{ userId:"u1", day:"2026-06-21", charsIn:34564, charsOut:120, fileBytes:2048 }`; assert `screen.getByText("34,564")` and `screen.getByText("2.0 KB")`).

- [ ] **Step 2: Run tests to verify they fail**

Run: `corepack pnpm -C admin-web test -- columns usage-daily usage-trend`
Expected: FAIL — `fmtCount`/`fmtBytes` not exported; field names mismatch.

- [ ] **Step 3: Write the implementation**

`lib/types.ts` — replace `UsageRow` + `DailyRow`:

```ts
export interface UsageRow {
  userId: string | null;
  requests: number;
  charsIn: number;
  charsOut: number;
  files: number;
  fileBytes: number;
}
export interface DailyRow {
  userId: string | null;
  day: string;
  charsIn: number;
  charsOut: number;
  fileBytes: number;
}
```

(Keep `UsageResponse`/`UsageDailyResponse` as-is — they reference `UsageRow[]`/`DailyRow[]`.)

`components/users/columns.tsx` — rename the export `fmtTokens` → `fmtCount` (same body), delete `fmtCost`, add `fmtBytes`:

```ts
export function fmtCount(n: number | undefined): string {
  if (n === undefined || n === null) return "—";
  return n.toLocaleString("en-US");
}
export function fmtBytes(n: number | undefined): string {
  if (n === undefined || n === null) return "—";
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / 1024 / 1024).toFixed(1)} MB`;
}
```

In `buildColumns(...)`, replace the three token/cost columns with two char columns (drop the Cost column):

```tsx
    {
      id: "charsIn", header: "Chars in",
      meta: { description: "Total **characters this user sent** (their request text), counted by the platform — not claude tokens." },
      cell: ({ row }) => <span className="tabular-nums">{fmtCount(usageByUser?.get(row.original.id)?.charsIn)}</span>,
    },
    {
      id: "charsOut", header: "Chars out",
      meta: { description: "Total **characters returned to this user** (answer text), counted by the platform." },
      cell: ({ row }) => <span className="tabular-nums">{fmtCount(usageByUser?.get(row.original.id)?.charsOut)}</span>,
    },
```

`lib/usage-daily.ts` — `DaySeries` + `buildDailySeries` carry the new fields:

```ts
export interface DaySeries { day: string; charsIn: number; charsOut: number; fileBytes: number; }
// in buildDailySeries: out.push({ day: key,
//   charsIn: r?.charsIn ?? 0, charsOut: r?.charsOut ?? 0, fileBytes: r?.fileBytes ?? 0 });
```

`components/users/usage-trend.tsx` — import `fmtCount, fmtBytes`; active filter on `charsIn||charsOut||fileBytes`; left-axis `Line`s `charsIn`/`charsOut` with `kfmt` (rename helper using `fmtCount`-style), right-axis `Line` `fileBytes` with a compact bytes tick (`(v)=>fmtBytes(v)`), tooltip formatting `Files bytes → fmtBytes` else `fmtCount`; per-day table columns `Day · Chars in · Chars out · File bytes` using `fmtCount`/`fmtBytes`.

`app/(dash)/users/page.tsx` — the `usageByUser`/`dailyByUser` fetch blocks are unchanged (they just hold the new shapes). Replace the detail-panel "Token usage" `PanelSection` body with the new fields (and rename the section title to "Usage"):

```tsx
                      <PanelField label="Requests">{u ? fmtCount(u.requests) : "—"}</PanelField>
                      <PanelField label="Chars in">{u ? fmtCount(u.charsIn) : "—"}</PanelField>
                      <PanelField label="Files">{u ? fmtCount(u.files) : "—"}</PanelField>
                      <PanelField label="File bytes">{u ? fmtBytes(u.fileBytes) : "—"}</PanelField>
                      <PanelField label="Chars out">{u ? fmtCount(u.charsOut) : "—"}</PanelField>
```

Update the import in `page.tsx`: `fmtTokens, fmtCost` → `fmtCount, fmtBytes`.

- [ ] **Step 4: Run tests + build**

Run: `corepack pnpm -C admin-web test` then `corepack pnpm -C admin-web build`
Expected: tests PASS; build compiles (no lingering `fmtTokens`/`fmtCost`/`tokensIn` references — grep to confirm: `git grep -nE "fmtTokens|fmtCost|tokensIn|tokensOut|costUsd" admin-web/` returns nothing).

- [ ] **Step 5: Commit**

```bash
git add admin-web/lib/types.ts admin-web/lib/usage-daily.ts admin-web/lib/usage-daily.test.ts \
  admin-web/components/users/columns.tsx admin-web/components/users/usage-trend.tsx \
  admin-web/components/users/usage-trend.test.tsx "admin-web/app/(dash)/users/page.tsx"
git commit -m "feat(admin-web): show self-counted chars + file bytes per user"
```

---

## Final integration check (after Task 3)

- [ ] `cargo test -p llm-chat-admin-api` still green (handlers unchanged, shapes pass through).
- [ ] Rebuild + recreate manager + admin-web (worker unchanged): `docker compose build manager admin-web && docker compose up -d --no-deps --force-recreate manager admin-web` (verify fresh image timestamps; `--no-cache` if stale).
- [ ] Drive a text question and a question with an image attachment via a client; `/control "usage"` shows the asking user's `charsIn`/`charsOut` and (for the image) non-zero `files`/`fileBytes`.
- [ ] Console Users page shows **Chars in/out** columns; the detail panel shows Requests/Chars in/Files/File bytes/Chars out and the daily chart plots chars + file-bytes. No "Tokens"/"Cost" remain.
