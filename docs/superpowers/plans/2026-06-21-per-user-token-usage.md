# Per-user Token-Usage Statistics — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Record claude's per-Q&A token usage, attribute it to the authenticated user, aggregate it in the manager, and show per-user tokens-in / tokens-out / cost on the Console Users page.

**Architecture:** The worker reads `usage` from claude's stream-json `result` event (already parsed for answer text) and forwards it in the `/qa` payload. The manager persists usage per `chat_question` row keyed by the authenticated `user_id` (source of truth) and exposes a `chat.admin`-gated `/control "usage"` aggregate. admin-api proxies it at `GET /api/usage`; admin-web joins it into the Users table.

**Tech Stack:** Rust (worker `llm-chat`, manager `llm-chat-manager`, `llm-chat-admin-api`; `sqlx` SQLite+Postgres, `serde_json`, `axum`), TypeScript/Next.js (`admin-web`, TanStack Table, vitest).

## Global Constraints

- Spec: `docs/superpowers/specs/token-usage-stats.md` (authoritative).
- **tokens_in displayed** = `input_tokens + cache_read_input_tokens + cache_creation_input_tokens`; the three are stored **raw and separate**; the sum is composed in the `/control` reply. **tokens_out** = `output_tokens`. **cost_usd** = `total_cost_usd`.
- Reporting only — no quotas/enforcement; cumulative all-time; covers human AND machine accounts.
- Fail-closed telemetry: missing/partial usage must **never** fail an answer — store NULL/zero and move on.
- `user_id` is the authenticated JWT `sub` the manager already holds — never client-supplied.
- Every Rust DB change must implement **both** the SQLite and Postgres arms of `ChatDb`.
- Worker headless build/test: `cargo test -p llm-chat --no-default-features`. Manager: `cargo test -p llm-chat-manager`. admin-api: `cargo test -p llm-chat-admin-api`. admin-web: `corepack pnpm -C admin-web test`.

---

## File Structure

- `worker/src/lib.rs` — add `parse_usage` (pure) + attach `usage` to the qa payload in the stream-json reader (~795).
- `manager/src/main.rs` — schema columns (`init_schema_sqlite` ~287 / `init_schema_postgres` ~324); `insert_pending` gains `user_id`; new `record_usage` method + `UsageRow` (pure `from_qa`); new `usage_by_user` query + `compose_usage_reply` (pure) + `/control "usage"` arm (model: `"queue"` ~1327).
- `admin-api/src/manager.rs` — `usage()` over the header-based `control_query`.
- `admin-api/src/api/mod.rs` — `GET /api/usage` (Operator gate).
- `admin-web/lib/types.ts`, `admin-web/app/(dash)/users/page.tsx`, `admin-web/components/users/columns.tsx` — fetch + join + columns + detail.

---

### Task 1: Worker — parse claude usage and forward it in the qa payload

**Files:**
- Modify: `worker/src/lib.rs` (stream-json reader branch ~795; add a pure `parse_usage` fn near the top-level helpers)
- Test: `worker/src/lib.rs` (`#[cfg(test)] mod usage_parse_tests`)

**Interfaces:**
- Produces: `fn parse_usage(result_event: &serde_json::Value) -> serde_json::Value` — returns a JSON object `{inputTokens,outputTokens,cacheReadTokens,cacheCreationTokens,costUsd,model}` (numbers default 0, `costUsd` may be null, `model` may be null), or `serde_json::Value::Null` when the event has no `usage` object. The qa payload gains a `"usage"` field carrying that value.

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod usage_parse_tests {
    use super::parse_usage;
    use serde_json::json;

    #[test]
    fn parses_usage_and_cost_and_model() {
        let ev = json!({
            "type": "result", "subtype": "success", "result": "hi",
            "usage": { "input_tokens": 4335, "output_tokens": 4,
                       "cache_read_input_tokens": 19447,
                       "cache_creation_input_tokens": 7060 },
            "total_cost_usd": 0.102,
            "modelUsage": { "claude-opus-4-8": { "inputTokens": 4335 } }
        });
        let u = parse_usage(&ev);
        assert_eq!(u["inputTokens"], 4335);
        assert_eq!(u["outputTokens"], 4);
        assert_eq!(u["cacheReadTokens"], 19447);
        assert_eq!(u["cacheCreationTokens"], 7060);
        assert_eq!(u["costUsd"], 0.102);
        assert_eq!(u["model"], "claude-opus-4-8");
    }

    #[test]
    fn no_usage_object_yields_null() {
        let ev = json!({ "type": "result", "result": "hi" });
        assert!(parse_usage(&ev).is_null());
    }

    #[test]
    fn missing_fields_default_to_zero() {
        let ev = json!({ "usage": { "input_tokens": 10 } });
        let u = parse_usage(&ev);
        assert_eq!(u["inputTokens"], 10);
        assert_eq!(u["outputTokens"], 0);
        assert_eq!(u["cacheReadTokens"], 0);
        assert_eq!(u["cacheCreationTokens"], 0);
        assert!(u["costUsd"].is_null());
        assert!(u["model"].is_null());
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p llm-chat --no-default-features usage_parse`
Expected: FAIL — `cannot find function parse_usage`.

- [ ] **Step 3: Write minimal implementation**

Add this free function near the other module-level helpers in `worker/src/lib.rs` (e.g. just above the `json_session` module):

```rust
/// PURE: pull the token usage out of a claude stream-json `result` event.
/// Returns a flat JSON object the manager can store, or Null when the event
/// carries no `usage` (e.g. the legacy PTY transport). Never panics on a
/// missing field — counts default to 0, cost/model to null.
fn parse_usage(result_event: &serde_json::Value) -> serde_json::Value {
    let Some(u) = result_event.get("usage").and_then(|v| v.as_object()) else {
        return serde_json::Value::Null;
    };
    let n = |k: &str| u.get(k).and_then(|v| v.as_i64()).unwrap_or(0);
    let model = result_event
        .get("modelUsage")
        .and_then(|m| m.as_object())
        .and_then(|m| m.keys().next().cloned());
    serde_json::json!({
        "inputTokens": n("input_tokens"),
        "outputTokens": n("output_tokens"),
        "cacheReadTokens": n("cache_read_input_tokens"),
        "cacheCreationTokens": n("cache_creation_input_tokens"),
        "costUsd": result_event.get("total_cost_usd").and_then(|v| v.as_f64()),
        "model": model,
    })
}
```

Then in the stream-json reader's `if is_result && ok { … }` branch (~795), add a `"usage"` field to the `payload` json (right after `"final": true,`):

```rust
                                "final": true,
                                "usage": parse_usage(&v),
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p llm-chat --no-default-features usage_parse`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add worker/src/lib.rs
git commit -m "feat(worker): forward claude token usage in the qa payload"
```

---

### Task 2: Manager — add usage columns to chat_question

**Files:**
- Modify: `manager/src/main.rs` (`init_schema_sqlite` ~287, `init_schema_postgres` ~324)
- Test: `manager/src/main.rs` (`#[cfg(test)] mod schema_tests`)

**Interfaces:**
- Produces: `chat_question` rows gain nullable columns `user_id TEXT`, `tokens_in INTEGER`, `tokens_out INTEGER`, `cache_read_tokens INTEGER`, `cache_creation_tokens INTEGER`, `cost_usd REAL`, `model TEXT`. Existing rows keep NULLs.

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod schema_tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    #[tokio::test]
    async fn chat_question_has_usage_columns() {
        let pool = SqlitePoolOptions::new().connect("sqlite::memory:").await.unwrap();
        init_schema_sqlite(&pool).await.unwrap();
        // Inserting the new columns must succeed.
        sqlx::query(
            "INSERT INTO chat_question
             (connection_id, sid, q_id, text, time_in, status,
              user_id, tokens_in, tokens_out, cache_read_tokens,
              cache_creation_tokens, cost_usd, model)
             VALUES ('c','s','q','t','now','answered',
                     'u1', 10, 5, 100, 20, 0.5, 'claude-opus-4-8')",
        )
        .execute(&pool).await.unwrap();
        let row: (Option<String>, Option<i64>, Option<f64>) = sqlx::query_as(
            "SELECT user_id, tokens_out, cost_usd FROM chat_question WHERE q_id='q'",
        )
        .fetch_one(&pool).await.unwrap();
        assert_eq!(row.0.as_deref(), Some("u1"));
        assert_eq!(row.1, Some(5));
        assert_eq!(row.2, Some(0.5));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p llm-chat-manager schema_tests`
Expected: FAIL — `no column named user_id` (insert error).

- [ ] **Step 3: Write minimal implementation**

In `init_schema_sqlite` (after the existing `ALTER TABLE … ADD COLUMN attachment_paths` block ~313), append — SQLite has no `IF NOT EXISTS` for `ADD COLUMN`, so swallow the duplicate-column error exactly like the existing lines do (`let _ =`):

```rust
    for col in [
        "ALTER TABLE chat_question ADD COLUMN user_id TEXT;",
        "ALTER TABLE chat_question ADD COLUMN tokens_in INTEGER;",
        "ALTER TABLE chat_question ADD COLUMN tokens_out INTEGER;",
        "ALTER TABLE chat_question ADD COLUMN cache_read_tokens INTEGER;",
        "ALTER TABLE chat_question ADD COLUMN cache_creation_tokens INTEGER;",
        "ALTER TABLE chat_question ADD COLUMN cost_usd REAL;",
        "ALTER TABLE chat_question ADD COLUMN model TEXT;",
    ] {
        let _ = sqlx::query(col).execute(pool).await;
    }
```

In `init_schema_postgres` (after the existing `ADD COLUMN IF NOT EXISTS attachment_paths` ~344), append — Postgres supports `IF NOT EXISTS`, so propagate errors with `?`:

```rust
    for col in [
        "ALTER TABLE chat_question ADD COLUMN IF NOT EXISTS user_id TEXT;",
        "ALTER TABLE chat_question ADD COLUMN IF NOT EXISTS tokens_in BIGINT;",
        "ALTER TABLE chat_question ADD COLUMN IF NOT EXISTS tokens_out BIGINT;",
        "ALTER TABLE chat_question ADD COLUMN IF NOT EXISTS cache_read_tokens BIGINT;",
        "ALTER TABLE chat_question ADD COLUMN IF NOT EXISTS cache_creation_tokens BIGINT;",
        "ALTER TABLE chat_question ADD COLUMN IF NOT EXISTS cost_usd DOUBLE PRECISION;",
        "ALTER TABLE chat_question ADD COLUMN IF NOT EXISTS model TEXT;",
    ] {
        sqlx::query(col).execute(pool).await?;
    }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p llm-chat-manager schema_tests`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add manager/src/main.rs
git commit -m "feat(manager): add per-user token-usage columns to chat_question"
```

---

### Task 3: Manager — attribute user_id at INSERT and record usage at answer

**Files:**
- Modify: `manager/src/main.rs` (`insert_pending` ~78; new `UsageRow` + `record_usage` method; caller ~2155; pairing site ~1761)
- Test: `manager/src/main.rs` (`#[cfg(test)] mod usage_row_tests`, extend `schema_tests`)

**Interfaces:**
- Consumes (Task 2): the usage columns on `chat_question`.
- Produces:
  - `insert_pending(&self, connection_id, sid, q_id, text, time_in, attachment_paths_json, user_id: Option<&str>) -> Result<i64, sqlx::Error>` (new trailing `user_id` arg).
  - `struct UsageRow { input: i64, output: i64, cache_read: i64, cache_creation: i64, cost: Option<f64>, model: Option<String> }` with `fn from_qa(payload: &serde_json::Value) -> Option<UsageRow>` (pure; reads the worker's `usage` object; None when absent/Null).
  - `record_usage(&self, seq: i64, u: &UsageRow) -> Result<(), sqlx::Error>` (UPDATE the token columns by seq).

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod usage_row_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn from_qa_reads_worker_usage() {
        let qa = json!({ "num": 1, "answer": "hi", "final": true,
            "usage": { "inputTokens": 10, "outputTokens": 5,
                       "cacheReadTokens": 100, "cacheCreationTokens": 20,
                       "costUsd": 0.5, "model": "claude-opus-4-8" } });
        let u = UsageRow::from_qa(&qa).expect("usage present");
        assert_eq!((u.input, u.output, u.cache_read, u.cache_creation), (10, 5, 100, 20));
        assert_eq!(u.cost, Some(0.5));
        assert_eq!(u.model.as_deref(), Some("claude-opus-4-8"));
    }

    #[test]
    fn from_qa_none_when_usage_absent_or_null() {
        assert!(UsageRow::from_qa(&json!({ "num": 1 })).is_none());
        assert!(UsageRow::from_qa(&json!({ "usage": null })).is_none());
    }

    #[tokio::test]
    async fn record_usage_writes_columns() {
        use sqlx::sqlite::SqlitePoolOptions;
        let pool = SqlitePoolOptions::new().connect("sqlite::memory:").await.unwrap();
        init_schema_sqlite(&pool).await.unwrap();
        let db = ChatDb::Sqlite(pool);
        let seq = db.insert_pending("c", "s", "q", "t", "now", None, Some("u1")).await.unwrap();
        let u = UsageRow { input: 10, output: 5, cache_read: 100,
                           cache_creation: 20, cost: Some(0.5),
                           model: Some("claude-opus-4-8".into()) };
        db.record_usage(seq, &u).await.unwrap();
        let got: (Option<String>, Option<i64>, Option<i64>, Option<f64>) =
            match &db { ChatDb::Sqlite(p) => sqlx::query_as(
                "SELECT user_id, tokens_in, tokens_out, cost_usd FROM chat_question WHERE seq=?")
                .bind(seq).fetch_one(p).await.unwrap(), _ => unreachable!() };
        assert_eq!(got, (Some("u1".into()), Some(10), Some(5), Some(0.5)));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p llm-chat-manager usage_row`
Expected: FAIL — `UsageRow` / `record_usage` not found; `insert_pending` arity mismatch.

- [ ] **Step 3: Write minimal implementation**

Add the struct + parser near `ChatDb` (above `impl ChatDb`):

```rust
/// One answer's token usage, parsed from the worker's qa `usage` object.
#[derive(Debug, Clone, PartialEq)]
struct UsageRow {
    input: i64,
    output: i64,
    cache_read: i64,
    cache_creation: i64,
    cost: Option<f64>,
    model: Option<String>,
}

impl UsageRow {
    /// PURE: read the worker's `usage` object off a qa payload. None when the
    /// payload has no usage (Null/absent — e.g. the PTY transport).
    fn from_qa(payload: &serde_json::Value) -> Option<UsageRow> {
        let u = payload.get("usage")?;
        if u.is_null() { return None; }
        let n = |k: &str| u.get(k).and_then(|v| v.as_i64()).unwrap_or(0);
        Some(UsageRow {
            input: n("inputTokens"),
            output: n("outputTokens"),
            cache_read: n("cacheReadTokens"),
            cache_creation: n("cacheCreationTokens"),
            cost: u.get("costUsd").and_then(|v| v.as_f64()),
            model: u.get("model").and_then(|v| v.as_str()).map(|s| s.to_string()),
        })
    }
}
```

Extend `insert_pending` (~78): add a trailing `user_id: Option<&str>` parameter, add `user_id` to BOTH the column list and the `VALUES`/`.bind` (SQLite and Postgres arms). E.g. the SQLite arm becomes:

```rust
                let r = sqlx::query(
                    "INSERT INTO chat_question
                     (connection_id, sid, q_id, text, time_in, status, attachment_paths, user_id)
                     VALUES (?, ?, ?, ?, ?, 'pending', ?, ?)",
                )
                .bind(connection_id).bind(sid).bind(q_id).bind(text)
                .bind(time_in).bind(attachment_paths_json).bind(user_id)
                .execute(p).await?;
```

(Postgres arm: add `, user_id` to the columns and `, $7` before `RETURNING seq`, then `.bind(user_id)`.)

Add `record_usage` as a new `ChatDb` method (next to `mark_answered`):

```rust
    /// UPDATE the token-usage columns for an answered row. Best-effort: a
    /// failure here must not fail the answer (caller logs and continues).
    async fn record_usage(&self, seq: i64, u: &UsageRow) -> Result<(), sqlx::Error> {
        match self {
            ChatDb::Sqlite(p) => {
                sqlx::query(
                    "UPDATE chat_question SET tokens_in=?, tokens_out=?,
                     cache_read_tokens=?, cache_creation_tokens=?, cost_usd=?, model=?
                     WHERE seq=?",
                )
                .bind(u.input).bind(u.output).bind(u.cache_read)
                .bind(u.cache_creation).bind(u.cost).bind(&u.model).bind(seq)
                .execute(p).await?;
            }
            ChatDb::Postgres(p) => {
                sqlx::query(
                    "UPDATE chat_question SET tokens_in=$1, tokens_out=$2,
                     cache_read_tokens=$3, cache_creation_tokens=$4, cost_usd=$5, model=$6
                     WHERE seq=$7",
                )
                .bind(u.input).bind(u.output).bind(u.cache_read)
                .bind(u.cache_creation).bind(u.cost).bind(&u.model).bind(seq)
                .execute(p).await?;
            }
        }
        Ok(())
    }
```

Wire the INSERT caller (~2155): pass the connection's authenticated user id. The chat handler already holds it for this connection (the same value recorded as the session owner). Thread it as `Some(uid.as_str())` (or `None` in shared-token mode where there is no per-user identity) into the new `insert_pending` argument.

Wire the answer site (~1761): right after the `mark_answered` call, record usage if the qa payload carried it. The qa payload `serde_json::Value` is in scope at the pairing site (it produced `num`/`final_text`); name it `qa` if not already:

```rust
    if let Some(usage) = UsageRow::from_qa(&qa) {
        if let Err(e) = db.record_usage(seq, &usage).await {
            tracing::warn!(target: "manager::chat::qa", seq, error = %e,
                "record_usage failed — row keeps NULL usage");
        }
    }
```

(If the pairing function does not currently keep the raw qa `Value`, thread it in from where `num`/`final`/`answer` are extracted — that is the same JSON object.)

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p llm-chat-manager usage_row && cargo test -p llm-chat-manager`
Expected: PASS (new tests + the existing suite — fix any other `insert_pending` call sites the compiler flags by passing `None`/the uid).

- [ ] **Step 5: Commit**

```bash
git add manager/src/main.rs
git commit -m "feat(manager): attribute user_id and persist token usage per answer"
```

---

### Task 4: Manager — /control "usage" aggregate command

**Files:**
- Modify: `manager/src/main.rs` (new `usage_by_user` query method; pure `compose_usage_reply`; `/control` match arm, model `"queue"` ~1327)
- Test: `manager/src/main.rs` (`#[cfg(test)] mod usage_agg_tests`)

**Interfaces:**
- Consumes (Task 3): populated usage columns.
- Produces:
  - `struct UserUsage { user_id: Option<String>, requests: i64, input: i64, output: i64, cache_read: i64, cache_creation: i64, cost: f64, last_used: Option<String> }`.
  - `usage_by_user(&self) -> Result<Vec<UserUsage>, sqlx::Error>`.
  - `fn compose_usage_reply(rows: &[UserUsage]) -> serde_json::Value` (pure) — per-row `tokensIn = input + cache_read + cache_creation`, plus a `totals` object. This is the exact reply shape the spec defines.
  - `/control` accepts `{"cmd":"usage"}` → that reply.

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod usage_agg_tests {
    use super::*;

    #[test]
    fn compose_sums_components_and_totals() {
        let rows = vec![
            UserUsage { user_id: Some("u1".into()), requests: 2, input: 10, output: 5,
                        cache_read: 100, cache_creation: 20, cost: 0.5, last_used: Some("t2".into()) },
            UserUsage { user_id: Some("u2".into()), requests: 1, input: 1, output: 1,
                        cache_read: 0, cache_creation: 0, cost: 0.1, last_used: Some("t1".into()) },
        ];
        let v = compose_usage_reply(&rows);
        assert_eq!(v["ok"], true);
        assert_eq!(v["users"][0]["userId"], "u1");
        assert_eq!(v["users"][0]["tokensIn"], 130);   // 10 + 100 + 20
        assert_eq!(v["users"][0]["tokensOut"], 5);
        assert_eq!(v["totals"]["requests"], 3);
        assert_eq!(v["totals"]["tokensIn"], 132);      // 130 + 2
        assert_eq!(v["totals"]["tokensOut"], 6);
    }

    #[tokio::test]
    async fn usage_by_user_groups_and_excludes_pending() {
        use sqlx::sqlite::SqlitePoolOptions;
        let pool = SqlitePoolOptions::new().connect("sqlite::memory:").await.unwrap();
        init_schema_sqlite(&pool).await.unwrap();
        let db = ChatDb::Sqlite(pool);
        // two answered rows for u1, one still pending (excluded)
        for (q, status, tin, tout) in [("q1","answered",10,5),("q2","confirmed",20,7),("q3","pending",99,99)] {
            let seq = db.insert_pending("c","s",q,"t","now",None,Some("u1")).await.unwrap();
            if status != "pending" {
                db.update_status(seq, status).await.unwrap();
                db.record_usage(seq, &UsageRow{input:tin,output:tout,cache_read:0,
                    cache_creation:0,cost:Some(0.1),model:None}).await.unwrap();
            }
        }
        let rows = db.usage_by_user().await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].requests, 2);
        assert_eq!(rows[0].input, 30);
        assert_eq!(rows[0].output, 12);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p llm-chat-manager usage_agg`
Expected: FAIL — `UserUsage` / `compose_usage_reply` / `usage_by_user` not found.

- [ ] **Step 3: Write minimal implementation**

Add the struct + pure composer (near `ChatDb`):

```rust
#[derive(Debug, Clone, sqlx::FromRow)]
struct UserUsage {
    user_id: Option<String>,
    requests: i64,
    input: i64,
    output: i64,
    cache_read: i64,
    cache_creation: i64,
    cost: f64,
    last_used: Option<String>,
}

/// PURE: build the /control "usage" reply. tokensIn folds the cache components
/// into the input footprint; totals sum across users.
fn compose_usage_reply(rows: &[UserUsage]) -> serde_json::Value {
    let mut users = Vec::with_capacity(rows.len());
    let (mut treq, mut tin, mut tout, mut tcost) = (0i64, 0i64, 0i64, 0f64);
    for r in rows {
        let tokens_in = r.input + r.cache_read + r.cache_creation;
        treq += r.requests; tin += tokens_in; tout += r.output; tcost += r.cost;
        users.push(serde_json::json!({
            "userId": r.user_id, "requests": r.requests,
            "tokensIn": tokens_in, "tokensOut": r.output,
            "cacheReadTokens": r.cache_read, "cacheCreationTokens": r.cache_creation,
            "costUsd": r.cost, "lastUsed": r.last_used,
        }));
    }
    serde_json::json!({
        "ok": true, "users": users,
        "totals": { "requests": treq, "tokensIn": tin, "tokensOut": tout, "costUsd": tcost },
    })
}
```

Add the query method to `impl ChatDb` (one SELECT per dialect; identical text):

```rust
    async fn usage_by_user(&self) -> Result<Vec<UserUsage>, sqlx::Error> {
        let sql = "SELECT user_id,
                     COUNT(*) AS requests,
                     COALESCE(SUM(tokens_in),0) AS input,
                     COALESCE(SUM(tokens_out),0) AS output,
                     COALESCE(SUM(cache_read_tokens),0) AS cache_read,
                     COALESCE(SUM(cache_creation_tokens),0) AS cache_creation,
                     COALESCE(SUM(cost_usd),0) AS cost,
                     MAX(time_out) AS last_used
                   FROM chat_question
                   WHERE status IN ('answered','confirmed')
                   GROUP BY user_id";
        match self {
            ChatDb::Sqlite(p) => sqlx::query_as::<_, UserUsage>(sql).fetch_all(p).await,
            ChatDb::Postgres(p) => sqlx::query_as::<_, UserUsage>(sql).fetch_all(p).await,
        }
    }
```

Add the `/control` arm (in the `match cmd` block, alongside `"queue"` ~1327). The `db`/state handle used by `"queue"` is in scope here:

```rust
            "usage" => match db.usage_by_user().await {
                Ok(rows) => compose_usage_reply(&rows),
                Err(e) => serde_json::json!({"ok": false, "error": format!("usage query: {e}")}),
            },
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p llm-chat-manager usage_agg && cargo test -p llm-chat-manager`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add manager/src/main.rs
git commit -m "feat(manager): /control usage — per-user token aggregate"
```

---

### Task 5: admin-api — GET /api/usage

**Files:**
- Modify: `admin-api/src/api/mod.rs` (route registration ~67; `usage` handler next to `chat_sessions` ~228)
- Test: `admin-api/src/api/mod.rs` (gate test, mirroring the existing `/api/*` gate tests)

**Interfaces:**
- Consumes (Task 4): `/control {"cmd":"usage"}` → the reply object.
- Produces: `GET /api/usage`, behind the `Operator` (chat.admin) extractor, returning the manager `usage` reply verbatim. Reuses the existing `crate::manager::control_query` (no new wrapper) and the `manager_control_url` capability gate, exactly like `chat_sessions`.

- [ ] **Step 1: Write the failing test**

In `admin-api/src/api/mod.rs` tests (mirror however `/api/chat-sessions` is tested — it already proxies `/control`). Add:

```rust
    #[tokio::test]
    async fn usage_route_requires_operator() {
        // building the router and calling GET /api/usage without a session cookie
        // returns 401 (same harness the other gated routes use).
        let app = test_router_no_session();
        let res = app.oneshot(
            axum::http::Request::builder().uri("/api/usage").body(axum::body::Body::empty()).unwrap()
        ).await.unwrap();
        assert_eq!(res.status(), axum::http::StatusCode::UNAUTHORIZED);
    }
```

(Use the same `test_router_no_session()` helper the existing `/api/*` gate tests use; if none exists, copy the pattern from the nearest gated-route test.)

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p llm-chat-admin-api usage_route`
Expected: FAIL — route not found → 404 (not 401).

- [ ] **Step 3: Write minimal implementation**

`admin-api/src/api/mod.rs` — register the route next to `chat-sessions` (~line 67):

```rust
        .route("/api/usage", get(usage))
```

Add the handler next to `chat_sessions` (~228), mirroring it exactly (same `Operator` gate, same `manager_control_url` capability check, same `mint_chat_token`, same `control_query` + degrade-on-error):

```rust
/// Per-user token usage from the manager's /control "usage" (chat.admin-gated).
/// Capability-gated on MANAGER_CONTROL_URL, exactly like chat_sessions.
async fn usage(_op: Operator, State(st): State<AppState>) -> Result<Json<Value>, ApiError> {
    let Some(url) = st.cfg.manager_control_url.clone() else {
        return Ok(Json(json!({ "configured": false, "users": [], "totals": {} })));
    };
    let token = st.zitadel.mint_chat_token().await?;
    Ok(Json(
        crate::manager::control_query(&url, &token, "usage")
            .await
            .unwrap_or_else(|e| json!({ "ok": false, "error": e })),
    ))
}
```

(`json!`, `get`, `Operator`, `AppState`, `ApiError`, `Json`, `Value` are already imported in this file — `chat_sessions` uses them all.)

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p llm-chat-admin-api usage_route && cargo test -p llm-chat-admin-api`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add admin-api/src/manager.rs admin-api/src/api/mod.rs
git commit -m "feat(admin-api): GET /api/usage proxying manager /control usage"
```

---

### Task 6: admin-web — Users-page token columns + detail

**Files:**
- Modify: `admin-web/lib/types.ts` (a `UsageRow` type), `admin-web/app/(dash)/users/page.tsx` (fetch + index), `admin-web/components/users/columns.tsx` (columns + tooltips)
- Test: `admin-web/components/users/columns.test.tsx` (or the project's column test pattern; run with `corepack pnpm -C admin-web test`)

**Interfaces:**
- Consumes (Task 5): `GET /api/usage` → `{ ok, users: UsageRow[], totals }`.
- Produces: a `usageByUser: Map<string, UsageRow>` passed into `buildColumns`; three right-aligned columns + detail breakdown.

- [ ] **Step 1: Write the failing test**

```tsx
import { describe, it, expect } from "vitest";
import { fmtTokens } from "@/components/users/columns";

describe("token usage formatting", () => {
  it("formats thousands and dashes missing", () => {
    expect(fmtTokens(123456)).toBe("123,456");
    expect(fmtTokens(undefined)).toBe("—");
    expect(fmtTokens(0)).toBe("0");
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `corepack pnpm -C admin-web test -- columns`
Expected: FAIL — `fmtTokens` is not exported.

- [ ] **Step 3: Write minimal implementation**

`admin-web/lib/types.ts`:

```ts
export interface UsageRow {
  userId: string | null;
  requests: number;
  tokensIn: number;
  tokensOut: number;
  cacheReadTokens: number;
  cacheCreationTokens: number;
  costUsd: number;
  lastUsed: string | null;
}
```

`admin-web/components/users/columns.tsx` — add the exported formatter and three columns (joined by `user.id` via a `usageByUser` map passed to `buildColumns`; follow the existing `meta.description` tooltip pattern):

```ts
export function fmtTokens(n: number | undefined): string {
  if (n === undefined || n === null) return "—";
  return n.toLocaleString("en-US");
}
export function fmtCost(n: number | undefined): string {
  if (n === undefined || n === null) return "—";
  return `$${n.toFixed(n < 1 ? 4 : 2)}`;
}
```

Add to `buildColumns(h, rolesByUser?, usageByUser?: Map<string, UsageRow>)` three columns, e.g.:

```tsx
    {
      id: "tokensIn", header: "Tokens in",
      meta: { description: "Total **input** tokens (prompt + cache read + cache creation) across this user's answered questions." },
      cell: ({ row }) => <span className="tabular-nums">{fmtTokens(usageByUser?.get(row.original.id)?.tokensIn)}</span>,
    },
    {
      id: "tokensOut", header: "Tokens out",
      meta: { description: "Total **output** tokens claude generated for this user." },
      cell: ({ row }) => <span className="tabular-nums">{fmtTokens(usageByUser?.get(row.original.id)?.tokensOut)}</span>,
    },
    {
      id: "cost", header: "Cost",
      meta: { description: "claude's reported `total_cost_usd`, summed — informational, not billing-grade." },
      cell: ({ row }) => <span className="tabular-nums">{fmtCost(usageByUser?.get(row.original.id)?.costUsd)}</span>,
    },
```

`admin-web/app/(dash)/users/page.tsx` — fetch `/api/usage` alongside the existing loads, build `usageByUser` from `result.users` keyed by `userId`, pass it to `buildColumns`. Best-effort: a failed fetch leaves the map empty (columns show "—"), never blanks the page (mirror the `holdersByKey` pattern). Add the breakdown (input / cache-read / cache-creation / output / requests / cost / last used) to the user detail panel.

- [ ] **Step 4: Run test to verify it passes**

Run: `corepack pnpm -C admin-web test -- columns`
Expected: PASS. Also `corepack pnpm -C admin-web build` to confirm the page compiles.

- [ ] **Step 5: Commit**

```bash
git add admin-web/lib/types.ts admin-web/app/(dash)/users/page.tsx admin-web/components/users/columns.tsx admin-web/components/users/columns.test.tsx
git commit -m "feat(admin-web): per-user token usage columns on the Users page"
```

---

## Final integration check (after Task 6)

- [ ] Rebuild + recreate the stack images that changed: `docker compose build manager admin-api admin-web && docker compose up -d --no-deps manager admin-api admin-web` (worker is native — rebuild `cargo build -p llm-chat --bin llm-chat-headless --no-default-features` and restart it).
- [ ] Drive one question (`llm-chat ask --send "hi" --manager ws://127.0.0.1:7777/chat`), then `GET /api/usage` (via the Console Users page) shows non-zero tokens for the kabytech user.
- [ ] Confirm a `chat.user`-only token cannot reach `/api/usage` (401/403) — the gate holds.
