// llm-chat-manager — spawns N llm-chat backends and proxies a unified
// WebSocket API to them. Routes per-session traffic to the backend that owns
// that session.
//
// Endpoints (manager port, default 7777):
//   /                — JSON list of all session ids across all backends
//   /control         — JSON command channel (same vocabulary as a backend's
//                       /control, plus "instances" to inspect backends)
//   /s/<sessionId>   — bidirectional raw PTY bridge to the owning backend
//   /qa/<sessionId>  — read-only Q&A stream from the owning backend
//
// Configuration (env vars):
//   MANAGER_PORT          — manager WS port (default 7777)
//   MANAGER_INSTANCES     — number of backends to spawn (default 2)
//   MANAGER_START_PORT    — first backend port; consecutive ports are used
//                            (default 7878)
//   LLM_CHAT_EXE          — path to the worker exe (default
//                            "../worker/target/debug/llm-chat-worker.exe"
//                            relative to manager.exe location)

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use sqlx::postgres::PgPool;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{
        client::IntoClientRequest,
        handshake::server::{ErrorResponse, Request, Response},
        http, Message,
    },
};

// ---------- FIFO queue for /chat questions ----------
//
// Two backends, runtime-selected via $MANAGER_DB_URL:
//   - sqlite (default):
//       file at $XDG_DATA_HOME/com.llm-chat.app/manager.sqlite
//       (override with $MANAGER_DB_PATH)
//   - postgres:
//       set $MANAGER_DB_URL=postgres://user:pass@host/dbname
//       (e.g. postgres://llmchat:llmchat@127.0.0.1/LLMService)
//
// Single logical table everywhere — the column types differ slightly because
// SQLite uses INTEGER AUTOINCREMENT while Postgres uses BIGSERIAL:
//
//   chat_question(seq, connection_id, sid, q_id, text, time_in,
//                 status, answer_text, time_out)
//
// `seq` is the FIFO key. `status` transitions:
//   pending → sent → answered (happy path)
//   pending → sent → error    (PTY died, parser timeout, etc.)

/// Per-user self-counted usage aggregate, returned by `ChatDb::usage_by_user`.
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

/// PURE: build the /control "usage" reply from the self-counted per-user rows.
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
    serde_json::json!({
        "ok": true, "users": users,
        "totals": { "requests": treq, "charsIn": tin, "charsOut": tout, "files": tf, "fileBytes": tb },
    })
}

/// One (user, day) self-counted usage bucket, returned by `ChatDb::usage_daily`.
#[derive(Debug, Clone, sqlx::FromRow)]
struct DailyRow {
    user_id: Option<String>,
    day: String,
    chars_in: i64,
    chars_out: i64,
    file_bytes: i64,
}

/// PURE: build the /control "usage-daily" reply, per (user, day).
fn compose_daily_reply(rows: &[DailyRow]) -> serde_json::Value {
    let days: Vec<serde_json::Value> = rows.iter().map(|r| serde_json::json!({
        "userId": r.user_id,
        "day": r.day,
        "charsIn": r.chars_in,
        "charsOut": r.chars_out,
        "fileBytes": r.file_bytes,
    })).collect();
    serde_json::json!({ "ok": true, "days": days })
}

/// THIS user's own lifetime self-counted totals — no `user_id` column, since the
/// caller is known from the authenticated connection. Returned by `usage_for`.
#[derive(Debug, Clone, sqlx::FromRow)]
struct OwnUsage {
    requests: i64,
    chars_in: i64,
    chars_out: i64,
    files: i64,
    file_bytes: i64,
    last_used: Option<String>,
}

/// One day of THIS user's own usage. Returned by `usage_daily_for`.
#[derive(Debug, Clone, sqlx::FromRow)]
struct OwnDaily {
    day: String,
    requests: i64,
    chars_in: i64,
    chars_out: i64,
    files: i64,
    file_bytes: i64,
}

/// PURE: the `/chat` "usage" reply for ONE authenticated user — own lifetime
/// totals plus a per-day breakdown (the manager passes the last 7 days).
fn compose_own_usage_reply(user_id: &str, total: &OwnUsage, daily: &[OwnDaily]) -> serde_json::Value {
    let days: Vec<serde_json::Value> = daily.iter().map(|d| serde_json::json!({
        "day": d.day, "requests": d.requests,
        "charsIn": d.chars_in, "charsOut": d.chars_out,
        "files": d.files, "fileBytes": d.file_bytes,
    })).collect();
    serde_json::json!({
        "type": "usage",
        "userId": user_id,
        "requests": total.requests,
        "charsIn": total.chars_in,
        "charsOut": total.chars_out,
        "files": total.files,
        "fileBytes": total.file_bytes,
        "lastUsed": total.last_used,
        "daily": days,
    })
}

/// Runtime-dispatched chat queue backend. Each variant holds an open pool;
/// every operation matches and uses the dialect-appropriate query string.
#[derive(Clone)]
enum ChatDb {
    Sqlite(SqlitePool),
    Postgres(PgPool),
}

impl ChatDb {
    fn dialect(&self) -> &'static str {
        match self {
            ChatDb::Sqlite(_) => "sqlite",
            ChatDb::Postgres(_) => "postgres",
        }
    }

    /// INSERT a 'pending' question. Returns the autoincrement seq.
    /// `attachment_paths_json` is an optional JSON array string of absolute
    /// file paths the backend saved for this question (e.g. `["/path/a.png"]`).
    /// `user_id` is the authenticated JWT `sub` of the asking user (None in
    /// shared-token / dev mode where there is no per-user identity).
    async fn insert_pending(
        &self,
        connection_id: &str,
        sid: &str,
        q_id: &str,
        text: &str,
        time_in: &str,
        attachment_paths_json: Option<&str>,
        user_id: Option<&str>,
        chars_in: i64,
        files: i64,
        file_bytes: i64,
    ) -> Result<i64, sqlx::Error> {
        match self {
            ChatDb::Sqlite(p) => {
                let r = sqlx::query(
                    "INSERT INTO chat_question
                     (connection_id, sid, q_id, text, time_in, status, attachment_paths, user_id,
                      chars_in, files, file_bytes)
                     VALUES (?, ?, ?, ?, ?, 'pending', ?, ?, ?, ?, ?)",
                )
                .bind(connection_id)
                .bind(sid)
                .bind(q_id)
                .bind(text)
                .bind(time_in)
                .bind(attachment_paths_json)
                .bind(user_id)
                .bind(chars_in)
                .bind(files)
                .bind(file_bytes)
                .execute(p)
                .await?;
                Ok(r.last_insert_rowid())
            }
            ChatDb::Postgres(p) => {
                let row: (i64,) = sqlx::query_as(
                    "INSERT INTO chat_question
                     (connection_id, sid, q_id, text, time_in, status, attachment_paths, user_id,
                      chars_in, files, file_bytes)
                     VALUES ($1, $2, $3, $4, $5, 'pending', $6, $7, $8, $9, $10) RETURNING seq",
                )
                .bind(connection_id)
                .bind(sid)
                .bind(q_id)
                .bind(text)
                .bind(time_in)
                .bind(attachment_paths_json)
                .bind(user_id)
                .bind(chars_in)
                .bind(files)
                .bind(file_bytes)
                .fetch_one(p)
                .await?;
                Ok(row.0)
            }
        }
    }

    /// UPDATE a row's status (e.g. 'sent', 'error'). No-op on row not found.
    async fn update_status(&self, seq: i64, status: &str) -> Result<(), sqlx::Error> {
        match self {
            ChatDb::Sqlite(p) => {
                sqlx::query("UPDATE chat_question SET status = ? WHERE seq = ?")
                    .bind(status)
                    .bind(seq)
                    .execute(p)
                    .await?;
            }
            ChatDb::Postgres(p) => {
                sqlx::query("UPDATE chat_question SET status = $1 WHERE seq = $2")
                    .bind(status)
                    .bind(seq)
                    .execute(p)
                    .await?;
            }
        }
        Ok(())
    }

    /// Pop the oldest 'sent' row for this connection (FIFO). Returns
    /// (seq, q_id, time_in) of the matched row.
    async fn pop_sent(
        &self,
        connection_id: &str,
    ) -> Result<Option<(i64, String, String)>, sqlx::Error> {
        match self {
            ChatDb::Sqlite(p) => {
                sqlx::query_as(
                    "SELECT seq, q_id, time_in FROM chat_question
                     WHERE connection_id = ? AND status = 'sent'
                     ORDER BY seq ASC LIMIT 1",
                )
                .bind(connection_id)
                .fetch_optional(p)
                .await
            }
            ChatDb::Postgres(p) => {
                sqlx::query_as(
                    "SELECT seq, q_id, time_in FROM chat_question
                     WHERE connection_id = $1 AND status = 'sent'
                     ORDER BY seq ASC LIMIT 1",
                )
                .bind(connection_id)
                .fetch_optional(p)
                .await
            }
        }
    }

    /// Mark a row 'confirmed' (client received the answer) with time_confirmed.
    /// Only updates rows currently in 'answered' state — protects against
    /// stale/replayed confirms after we've already moved on or errored out.
    async fn mark_confirmed(&self, seq: i64, time_confirmed: &str) -> Result<bool, sqlx::Error> {
        // Pull rows_affected inside each arm — sqlx returns dialect-specific
        // QueryResult types that don't share a common trait we can match on.
        let affected = match self {
            ChatDb::Sqlite(p) => sqlx::query(
                "UPDATE chat_question SET status = 'confirmed', time_confirmed = ?
                 WHERE seq = ? AND status = 'answered'",
            )
            .bind(time_confirmed)
            .bind(seq)
            .execute(p)
            .await?
            .rows_affected(),
            ChatDb::Postgres(p) => sqlx::query(
                "UPDATE chat_question SET status = 'confirmed', time_confirmed = $1
                 WHERE seq = $2 AND status = 'answered'",
            )
            .bind(time_confirmed)
            .bind(seq)
            .execute(p)
            .await?
            .rows_affected(),
        };
        Ok(affected > 0)
    }

    /// Mark a row 'answered' with answer text + time_out.
    async fn mark_answered(
        &self,
        seq: i64,
        answer_text: &str,
        time_out: &str,
    ) -> Result<(), sqlx::Error> {
        match self {
            ChatDb::Sqlite(p) => {
                sqlx::query(
                    "UPDATE chat_question SET answer_text = ?, status = 'answered',
                     time_out = ?, chars_out = ? WHERE seq = ?",
                )
                .bind(answer_text)
                .bind(time_out)
                .bind(answer_text.chars().count() as i64)
                .bind(seq)
                .execute(p)
                .await?;
            }
            ChatDb::Postgres(p) => {
                sqlx::query(
                    "UPDATE chat_question SET answer_text = $1, status = 'answered',
                     time_out = $2, chars_out = $3 WHERE seq = $4",
                )
                .bind(answer_text)
                .bind(time_out)
                .bind(answer_text.chars().count() as i64)
                .bind(seq)
                .execute(p)
                .await?;
            }
        }
        Ok(())
    }

    /// Aggregate self-counted usage per user, excluding non-answered + null-user rows.
    async fn usage_by_user(&self) -> Result<Vec<UserUsage>, sqlx::Error> {
        // Pre-feature (historical) rows have NULL user_id and are unattributable,
        // so they are excluded from both the per-user rows and the totals.
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
        match self {
            ChatDb::Sqlite(p) => sqlx::query_as::<_, UserUsage>(sql).fetch_all(p).await,
            ChatDb::Postgres(p) => sqlx::query_as::<_, UserUsage>(sql).fetch_all(p).await,
        }
    }

    /// Per-user per-day self-counted aggregate since `cutoff` (an ISO-8601
    /// string; `time_in` is also ISO-8601, so the string comparison is correct
    /// and dialect-portable). `substr(time_in,1,10)` is the YYYY-MM-DD day.
    async fn usage_daily(&self, cutoff: &str) -> Result<Vec<DailyRow>, sqlx::Error> {
        let sql = "SELECT user_id, substr(time_in,1,10) AS day,
                     COALESCE(SUM(chars_in),0) AS chars_in,
                     COALESCE(SUM(chars_out),0) AS chars_out,
                     COALESCE(SUM(file_bytes),0) AS file_bytes
                   FROM chat_question
                   WHERE status IN ('answered','confirmed')
                     AND user_id IS NOT NULL AND time_in >= ?
                   GROUP BY user_id, day
                   ORDER BY day";
        match self {
            ChatDb::Sqlite(p) => sqlx::query_as::<_, DailyRow>(sql).bind(cutoff).fetch_all(p).await,
            ChatDb::Postgres(p) => {
                let pg = sql.replace("time_in >= ?", "time_in >= $1");
                sqlx::query_as::<_, DailyRow>(&pg).bind(cutoff).fetch_all(p).await
            }
        }
    }

    /// THIS user's own lifetime self-counted totals (always exactly one row;
    /// zeros if the user has no answered questions). SCOPED by `user_id` — a
    /// caller can only ever pass their own authenticated JWT sub.
    async fn usage_for(&self, user_id: &str) -> Result<OwnUsage, sqlx::Error> {
        let sql = "SELECT COUNT(*) AS requests,
                     COALESCE(SUM(chars_in),0) AS chars_in,
                     COALESCE(SUM(chars_out),0) AS chars_out,
                     COALESCE(SUM(files),0) AS files,
                     COALESCE(SUM(file_bytes),0) AS file_bytes,
                     MAX(time_out) AS last_used
                   FROM chat_question
                   WHERE status IN ('answered','confirmed') AND user_id = ?";
        match self {
            ChatDb::Sqlite(p) => sqlx::query_as::<_, OwnUsage>(sql).bind(user_id).fetch_one(p).await,
            ChatDb::Postgres(p) => {
                let pg = sql.replace("user_id = ?", "user_id = $1");
                sqlx::query_as::<_, OwnUsage>(&pg).bind(user_id).fetch_one(p).await
            }
        }
    }

    /// THIS user's own per-day usage since `cutoff` (ISO-8601). SCOPED by
    /// `user_id`; `substr(time_in,1,10)` is the YYYY-MM-DD day.
    async fn usage_daily_for(&self, user_id: &str, cutoff: &str) -> Result<Vec<OwnDaily>, sqlx::Error> {
        let sql = "SELECT substr(time_in,1,10) AS day,
                     COUNT(*) AS requests,
                     COALESCE(SUM(chars_in),0) AS chars_in,
                     COALESCE(SUM(chars_out),0) AS chars_out,
                     COALESCE(SUM(files),0) AS files,
                     COALESCE(SUM(file_bytes),0) AS file_bytes
                   FROM chat_question
                   WHERE status IN ('answered','confirmed')
                     AND user_id = ? AND time_in >= ?
                   GROUP BY day ORDER BY day";
        match self {
            ChatDb::Sqlite(p) => {
                sqlx::query_as::<_, OwnDaily>(sql).bind(user_id).bind(cutoff).fetch_all(p).await
            }
            ChatDb::Postgres(p) => {
                let pg = sql
                    .replace("user_id = ?", "user_id = $1")
                    .replace("time_in >= ?", "time_in >= $2");
                sqlx::query_as::<_, OwnDaily>(&pg).bind(user_id).bind(cutoff).fetch_all(p).await
            }
        }
    }
}

async fn open_chat_db() -> Result<(ChatDb, String), sqlx::Error> {
    if let Ok(url) = std::env::var("MANAGER_DB_URL") {
        if url.starts_with("postgres://") || url.starts_with("postgresql://") {
            let pool = PgPool::connect(&url).await?;
            init_schema_postgres(&pool).await?;
            // Strip credentials for logging.
            let safe = sanitize_db_url(&url);
            return Ok((ChatDb::Postgres(pool), safe));
        }
        // Treat anything else as a sqlite path (URL or bare path).
        let path = url
            .strip_prefix("sqlite://")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from(url));
        let pool = open_sqlite_pool(&path).await?;
        init_schema_sqlite(&pool).await?;
        return Ok((ChatDb::Sqlite(pool), path.display().to_string()));
    }
    // No URL → default sqlite at the XDG path.
    let path = manager_db_path();
    let pool = open_sqlite_pool(&path).await?;
    init_schema_sqlite(&pool).await?;
    Ok((ChatDb::Sqlite(pool), path.display().to_string()))
}

fn sanitize_db_url(url: &str) -> String {
    // postgres://user:pass@host/db → postgres://user:***@host/db for logs.
    if let Some((scheme, rest)) = url.split_once("://") {
        if let Some((userpass, hostpath)) = rest.split_once('@') {
            if let Some((user, _pass)) = userpass.split_once(':') {
                return format!("{}://{}:***@{}", scheme, user, hostpath);
            }
        }
    }
    url.to_string()
}

async fn open_sqlite_pool(path: &std::path::Path) -> Result<SqlitePool, sqlx::Error> {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let opts = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .synchronous(sqlx::sqlite::SqliteSynchronous::Normal);
    SqlitePool::connect_with(opts).await
}

async fn init_schema_sqlite(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS chat_question (
            seq INTEGER PRIMARY KEY AUTOINCREMENT,
            connection_id TEXT NOT NULL,
            sid TEXT NOT NULL,
            q_id TEXT NOT NULL,
            text TEXT NOT NULL,
            time_in TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'pending',
            answer_text TEXT,
            time_out TEXT,
            time_confirmed TEXT,
            attachment_paths TEXT
        );
        "#,
    )
    .execute(pool)
    .await?;
    // SQLite has no ADD COLUMN IF NOT EXISTS — ignore the duplicate-column
    // errors. These migrations run once per startup and are idempotent
    // either way (the column either exists already or we just added it).
    let _ = sqlx::query("ALTER TABLE chat_question ADD COLUMN time_confirmed TEXT;")
        .execute(pool)
        .await;
    let _ = sqlx::query("ALTER TABLE chat_question ADD COLUMN attachment_paths TEXT;")
        .execute(pool)
        .await;
    for col in [
        "ALTER TABLE chat_question ADD COLUMN user_id TEXT;",
        "ALTER TABLE chat_question ADD COLUMN tokens_in INTEGER;",
        "ALTER TABLE chat_question ADD COLUMN tokens_out INTEGER;",
        "ALTER TABLE chat_question ADD COLUMN cache_read_tokens INTEGER;",
        "ALTER TABLE chat_question ADD COLUMN cache_creation_tokens INTEGER;",
        "ALTER TABLE chat_question ADD COLUMN cost_usd REAL;",
        "ALTER TABLE chat_question ADD COLUMN model TEXT;",
        "ALTER TABLE chat_question ADD COLUMN chars_in INTEGER;",
        "ALTER TABLE chat_question ADD COLUMN chars_out INTEGER;",
        "ALTER TABLE chat_question ADD COLUMN files INTEGER;",
        "ALTER TABLE chat_question ADD COLUMN file_bytes INTEGER;",
    ] {
        let _ = sqlx::query(col).execute(pool).await;
    }
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_chat_question_status_seq ON chat_question(status, seq);",
    )
    .execute(pool)
    .await?;
    Ok(())
}

async fn init_schema_postgres(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS chat_question (
            seq BIGSERIAL PRIMARY KEY,
            connection_id TEXT NOT NULL,
            sid TEXT NOT NULL,
            q_id TEXT NOT NULL,
            text TEXT NOT NULL,
            time_in TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'pending',
            answer_text TEXT,
            time_out TEXT,
            time_confirmed TEXT,
            attachment_paths TEXT
        );
        "#,
    )
    .execute(pool)
    .await?;
    sqlx::query("ALTER TABLE chat_question ADD COLUMN IF NOT EXISTS time_confirmed TEXT;")
        .execute(pool)
        .await?;
    sqlx::query("ALTER TABLE chat_question ADD COLUMN IF NOT EXISTS attachment_paths TEXT;")
        .execute(pool)
        .await?;
    for col in [
        "ALTER TABLE chat_question ADD COLUMN IF NOT EXISTS user_id TEXT;",
        "ALTER TABLE chat_question ADD COLUMN IF NOT EXISTS tokens_in BIGINT;",
        "ALTER TABLE chat_question ADD COLUMN IF NOT EXISTS tokens_out BIGINT;",
        "ALTER TABLE chat_question ADD COLUMN IF NOT EXISTS cache_read_tokens BIGINT;",
        "ALTER TABLE chat_question ADD COLUMN IF NOT EXISTS cache_creation_tokens BIGINT;",
        "ALTER TABLE chat_question ADD COLUMN IF NOT EXISTS cost_usd DOUBLE PRECISION;",
        "ALTER TABLE chat_question ADD COLUMN IF NOT EXISTS model TEXT;",
        "ALTER TABLE chat_question ADD COLUMN IF NOT EXISTS chars_in BIGINT;",
        "ALTER TABLE chat_question ADD COLUMN IF NOT EXISTS chars_out BIGINT;",
        "ALTER TABLE chat_question ADD COLUMN IF NOT EXISTS files BIGINT;",
        "ALTER TABLE chat_question ADD COLUMN IF NOT EXISTS file_bytes BIGINT;",
    ] {
        sqlx::query(col).execute(pool).await?;
    }
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_chat_question_status_seq ON chat_question(status, seq);",
    )
    .execute(pool)
    .await?;
    Ok(())
}

fn manager_db_path() -> std::path::PathBuf {
    if let Ok(p) = std::env::var("MANAGER_DB_PATH") {
        return std::path::PathBuf::from(p);
    }
    auth_token_path().with_file_name("manager.sqlite")
}

/// Read recent rows from chat_question. Returns the most recent first
/// (DESC seq). Filters are AND-combined; None means "no constraint".
/// Dialect-aware: SQLite uses ? placeholders, Postgres uses $1, $2, ...
async fn query_chat_queue(
    db: &ChatDb,
    connection_id: Option<&str>,
    sid: Option<&str>,
    status: Option<&str>,
    limit: i64,
) -> Result<Vec<serde_json::Value>, sqlx::Error> {
    type Row = (
        i64,
        String,
        String,
        String,
        String,
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
    );
    let rows: Vec<Row> = match db {
        ChatDb::Sqlite(pool) => {
            let mut sql = String::from(
                "SELECT seq, connection_id, sid, q_id, text, time_in, status, answer_text, time_out, attachment_paths
                 FROM chat_question WHERE 1=1",
            );
            if connection_id.is_some() {
                sql.push_str(" AND connection_id = ?");
            }
            if sid.is_some() {
                sql.push_str(" AND sid = ?");
            }
            if status.is_some() {
                sql.push_str(" AND status = ?");
            }
            sql.push_str(" ORDER BY seq DESC LIMIT ?");
            let mut q = sqlx::query_as::<_, Row>(&sql);
            if let Some(c) = connection_id {
                q = q.bind(c);
            }
            if let Some(s) = sid {
                q = q.bind(s);
            }
            if let Some(s) = status {
                q = q.bind(s);
            }
            q.bind(limit).fetch_all(pool).await?
        }
        ChatDb::Postgres(pool) => {
            let mut sql = String::from(
                "SELECT seq, connection_id, sid, q_id, text, time_in, status, answer_text, time_out, attachment_paths
                 FROM chat_question WHERE 1=1",
            );
            let mut idx = 1;
            if connection_id.is_some() {
                sql.push_str(&format!(" AND connection_id = ${}", idx));
                idx += 1;
            }
            if sid.is_some() {
                sql.push_str(&format!(" AND sid = ${}", idx));
                idx += 1;
            }
            if status.is_some() {
                sql.push_str(&format!(" AND status = ${}", idx));
                idx += 1;
            }
            sql.push_str(&format!(" ORDER BY seq DESC LIMIT ${}", idx));
            let mut q = sqlx::query_as::<_, Row>(&sql);
            if let Some(c) = connection_id {
                q = q.bind(c);
            }
            if let Some(s) = sid {
                q = q.bind(s);
            }
            if let Some(s) = status {
                q = q.bind(s);
            }
            q.bind(limit).fetch_all(pool).await?
        }
    };
    Ok(rows
        .into_iter()
        .map(|(seq, conn, sid, qid, text, ti, st, atxt, to, atts)| {
            // attachment_paths is stored as a JSON array string. Parse it back
            // to a JSON value so consumers don't get a double-encoded string.
            let attachments_json: serde_json::Value = atts
                .as_deref()
                .and_then(|s| serde_json::from_str(s).ok())
                .unwrap_or(serde_json::Value::Null);
            serde_json::json!({
                "seq": seq,
                "connectionId": conn,
                "sid": sid,
                "qId": qid,
                "text": text,
                "timeIn": ti,
                "status": st,
                "answerText": atxt,
                "timeOut": to,
                "attachmentPaths": attachments_json,
            })
        })
        .collect())
}

fn env_or<T: std::str::FromStr>(key: &str, default: T) -> T {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn random_token() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Pure, reusable: require a non-empty address env var. Trims surrounding
/// whitespace. Returns Err(format!("{var_name} must be set (no default)")) when
/// None/empty/whitespace-only; Ok(trimmed) otherwise. Shared by MANAGER_BIND
/// and MANAGER_BACKEND_HOST — these are REQUIRED, there is no code default.
fn require_addr(var_name: &str, raw: Option<String>) -> Result<String, String> {
    match raw.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
        Some(v) => Ok(v),
        None => Err(format!("{var_name} must be set (no default)")),
    }
}

/// Thin wrapper (not unit-tested): read MANAGER_BACKEND_HOST.
///
/// SAFE to unwrap: main() resolves MANAGER_BACKEND_HOST via require_addr at
/// startup and fails fast if it is missing, so by the time any request-time
/// dial site runs the var is guaranteed present. The five request-time sites
/// (call_backend, /s/, /qa/, bridge_to_backend, handle_root) are outside
/// main()'s scope and call this wrapper inline per request; MANAGER_BACKEND_HOST
/// is immutable for the process lifetime, so every read returns the same
/// already-validated value. Accepted per-request read (one-shot, not a hot
/// loop). No ManagerState field, no call_backend signature change.
fn backend_host() -> String {
    require_addr("MANAGER_BACKEND_HOST", std::env::var("MANAGER_BACKEND_HOST").ok())
        .expect("validated at startup")
}

/// Pure: parse a comma-separated MANAGER_BACKEND_PORTS list. Returns Some(ports)
/// when at least one token parses to a u16; None when unset/empty/all-unparseable.
/// PRESENCE is the mode toggle (unchanged semantics): None == "spawn local
/// workers" (today's behavior); Some == external-backend mode. This is NOT an
/// address value, so it is intentionally NOT made required.
fn parse_backend_ports(raw: Option<String>) -> Option<Vec<u16>> {
    let raw = raw?;
    let ports: Vec<u16> = raw
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse::<u16>().ok())
        .collect();
    if ports.is_empty() {
        None
    } else {
        Some(ports)
    }
}

/// Thin wrapper (not unit-tested): read MANAGER_BACKEND_PORTS.
fn external_backend_ports() -> Option<Vec<u16>> {
    parse_backend_ports(std::env::var("MANAGER_BACKEND_PORTS").ok())
}

/// Pure: choose the auth token — env value if non-empty, else `gen()`.
/// PRESENCE is the toggle (unchanged semantics, NOT an address value, NOT made
/// required): absent/empty -> generate a random token (today's behavior);
/// present -> use it. `gen` is injected so the parser is testable without RNG.
fn resolve_auth_token(env: Option<String>, gen: &dyn Fn() -> String) -> String {
    env.filter(|t| !t.is_empty()).unwrap_or_else(gen)
}

fn auth_token_path() -> std::path::PathBuf {
    // Prefer %LOCALAPPDATA%\com.llm-chat.app\ on Windows — per-user, not swept
    // by temp cleaners. On unix, the XDG equivalent is $XDG_DATA_HOME (or
    // $HOME/.local/share). Falling back to temp on either platform is a last
    // resort since /tmp is world-readable on Linux.
    #[cfg(windows)]
    {
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            let dir = std::path::Path::new(&local).join("com.llm-chat.app");
            if std::fs::create_dir_all(&dir).is_ok() {
                return dir.join("auth.token");
            }
        }
    }
    #[cfg(unix)]
    {
        let base = std::env::var_os("XDG_DATA_HOME")
            .map(std::path::PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/share"))
            });
        if let Some(base) = base {
            let dir = base.join("com.llm-chat.app");
            if std::fs::create_dir_all(&dir).is_ok() {
                return dir.join("auth.token");
            }
        }
    }
    let dir = std::env::temp_dir().join("llm-chat-qa");
    let _ = std::fs::create_dir_all(&dir);
    dir.join("auth.token")
}

/// Restrict the auth-token file's ACL so only the current user can read or
/// write it. No-op on non-Windows. Best-effort — failures are logged, not
/// fatal (the file is still per-user-readable by default ACL).
fn lock_token_acl(path: &std::path::Path) {
    #[cfg(windows)]
    {
        let username = match std::env::var("USERNAME") {
            Ok(u) if !u.is_empty() => u,
            _ => return,
        };
        let result = std::process::Command::new("icacls")
            .arg(path)
            .arg("/inheritance:r")
            .arg("/grant:r")
            .arg(format!("{}:F", username))
            .output();
        match result {
            Ok(o) if o.status.success() => {
                tracing::info!(target: "manager::auth", user = %username, "token ACL locked (icacls)");
            }
            Ok(o) => {
                tracing::warn!(
                    target: "manager::auth",
                    stderr = %String::from_utf8_lossy(&o.stderr),
                    "icacls non-zero exit"
                );
            }
            Err(e) => tracing::warn!(target: "manager::auth", error = %e, "icacls spawn failed"),
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        match std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)) {
            Ok(()) => tracing::info!(target: "manager::auth", "token chmod 0600"),
            Err(e) => tracing::warn!(target: "manager::auth", error = %e, "chmod 0600 failed"),
        }
    }
}

fn check_token_eq(provided: &str, expected: &str) -> bool {
    use subtle::ConstantTimeEq;
    provided.as_bytes().ct_eq(expected.as_bytes()).into()
}

/// Pull `key=value` out of a `&`-separated query string and percent-decode
/// the value. Returns None if the key is absent or the value is empty.
/// Tolerant of malformed percent escapes — those bytes pass through as-is.
fn parse_query_param(query: &str, key: &str) -> Option<String> {
    let prefix = format!("{}=", key);
    for kv in query.split('&') {
        if let Some(v) = kv.strip_prefix(&prefix) {
            if v.is_empty() {
                return None;
            }
            return Some(percent_decode(v));
        }
    }
    None
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
            if let Ok(b) = u8::from_str_radix(hex, 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        if bytes[i] == b'+' {
            out.push(b' ');
        } else {
            out.push(bytes[i]);
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn extract_token(req: &Request) -> Option<String> {
    if let Some(auth) = req.headers().get("authorization") {
        if let Ok(s) = auth.to_str() {
            if let Some(t) = s.strip_prefix("Bearer ") {
                return Some(t.trim().to_string());
            }
        }
    }
    if let Some(query) = req.uri().query() {
        for kv in query.split('&') {
            if let Some(t) = kv.strip_prefix("token=") {
                return Some(t.to_string());
            }
        }
    }
    None
}

/// Redact any `token=`/`access_token=` value from a query string before it is
/// logged, so a bearer/shared token presented in the URL never lands in logs
/// (defense-in-depth — the JWT path is header-only, but a client could still
/// append `?token=` and the legacy shared-token path still reads it).
fn redact_query(q: &str) -> String {
    q.split('&')
        .map(|kv| {
            let name = kv.split('=').next().unwrap_or("");
            if name.eq_ignore_ascii_case("token") || name.eq_ignore_ascii_case("access_token") {
                format!("{name}=<redacted>")
            } else {
                kv.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("&")
}

/// Live registry entry for one connected /chat client. Mutated as the client
/// sends questions; removed when the connection ends. Purely in-memory — the
/// chat_question table has the persistent record of what they sent.
#[derive(Clone, serde::Serialize)]
struct ClientInfo {
    #[serde(rename = "connectionId")]
    connection_id: String,
    sid: String,
    /// The AUTHENTICATED owner (JWT sub) — who this session belongs to.
    /// Surfaced via /control "clients" (chat.admin-only) for the admin Console.
    #[serde(rename = "userId")]
    user_id: String,
    #[serde(rename = "backendPort")]
    backend_port: u16,
    #[serde(rename = "connectedAt")]
    connected_at: String,
    #[serde(rename = "lastQAt")]
    last_q_at: Option<String>,
    #[serde(rename = "questionsSent")]
    questions_sent: u32,
}

// Not Default — chat_db requires an open SqlitePool, constructed in main().
struct ManagerState {
    /// Backend ports, in spawn order.
    instance_ports: Vec<u16>,
    /// sessionId -> backend port that owns it.
    session_to_port: HashMap<String, u16>,
    /// sessionId -> AUTHENTICATED owner (JWT sub) who opened it. The authz key
    /// for attaching to a live session: `/s/<sid>` and `/qa/<sid>` may only be
    /// reached by this owner (or a chat.admin operator). Populated in cmd_open,
    /// cleared in cmd_close — same lifecycle as `session_to_port`.
    session_to_owner: HashMap<String, String>,
    /// Internal shared secret used for the manager↔backend hop only
    /// (loopback). Inbound client auth is now Zitadel JWT — see `jwks`.
    auth_token: String,
    /// Zitadel JWKS cache for verifying inbound client JWTs. Refreshed in
    /// the background. None means external auth is not configured (the
    /// manager will then refuse all inbound requests).
    jwks: Option<zitadel_auth::JwksCache>,
    /// FIFO of /chat questions, backed by SQLite OR Postgres depending on
    /// $MANAGER_DB_URL. Source of truth for the queue; outlives any single
    /// connection so a crash mid-question is recoverable.
    chat_db: ChatDb,
    /// Live /chat clients keyed by connection_id. Inserted on connect,
    /// updated on each q, removed on disconnect. /control "clients" reads
    /// this for a real-time view of who's online.
    clients: HashMap<String, ClientInfo>,
}

type SharedState = Arc<Mutex<ManagerState>>;

/// Initialise tracing once at process start. Default level is INFO; override
/// per-component via `RUST_LOG=manager=debug,manager::ws=trace`. Output goes
/// to stderr in pretty format unless `LOG_JSON=1` flips it to JSON for log
/// aggregation pipelines.
fn init_tracing() {
    use tracing_subscriber::EnvFilter;
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let json = matches!(
        std::env::var("LOG_JSON").ok().as_deref(),
        Some("1") | Some("true")
    );
    if json {
        tracing_subscriber::fmt()
            .json()
            .with_env_filter(filter)
            .with_writer(std::io::stderr)
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_target(true)
            .with_writer(std::io::stderr)
            .init();
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_tracing();

    let manager_port: u16 = env_or("MANAGER_PORT", 7777u16);
    let n_instances: usize = env_or("MANAGER_INSTANCES", 2usize);
    let start_port: u16 = env_or("MANAGER_START_PORT", 7878u16);
    let exe_path = std::env::var("LLM_CHAT_EXE").unwrap_or_else(|_| {
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        // Default backend lives at sibling crate's target dir. We're usually
        // built in release, but fall back to debug if release isn't present.
        let backend_name = if cfg!(windows) {
            "llm-chat-worker.exe"
        } else {
            "llm-chat-worker"
        };
        let project_root = exe_dir.join("..").join("..").join("..");
        let release = project_root
            .join("worker")
            .join("target")
            .join("release")
            .join(backend_name);
        let debug = project_root
            .join("worker")
            .join("target")
            .join("debug")
            .join(backend_name);
        let chosen = if release.exists() { release } else { debug };
        chosen.to_string_lossy().into_owned()
    });

    tracing::info!(
        target: "manager",
        manager_port,
        n_instances,
        start_port,
        exe = %exe_path,
        "manager starting"
    );

    // Generate a single per-process auth token used for every WS connection
    // (manager↔backend and client↔manager). Persist it so local clients can
    // read it from a known file.
    let auth_token = resolve_auth_token(
        std::env::var("LLM_CHAT_AUTH_TOKEN").ok(),
        &random_token,
    );
    let token_path = auth_token_path();
    std::fs::write(&token_path, &auth_token)?;
    lock_token_acl(&token_path);
    tracing::info!(
        target: "manager::auth",
        token_path = %token_path.display(),
        "auth token persisted"
    );
    // Token itself only at DEBUG — INFO log shouldn't leak credentials.
    tracing::debug!(target: "manager::auth", token = %auth_token, "auth token");

    // Resolve BOTH required addresses up front and FAIL FAST if either is
    // missing — there is no code default. This must happen BEFORE any side
    // effect (spawning worker instances, launching claude, opening the DB):
    // a missing required var should abort cleanly, not after we've already
    // started backend processes. backend_host is threaded into the
    // spawn_instance forwarding, wait_for_tcp, and the spawn-skip log; bind_host
    // is used for the listen socket far below. Both are validated here so every
    // site observes one validated value and the failure surfaces immediately.
    let backend_host = require_addr(
        "MANAGER_BACKEND_HOST",
        std::env::var("MANAGER_BACKEND_HOST").ok(),
    )?;
    let bind_host = require_addr("MANAGER_BIND", std::env::var("MANAGER_BIND").ok())?;

    let stealth: bool = matches!(
        std::env::var("MANAGER_STEALTH").ok().as_deref(),
        Some("1") | Some("true")
    );

    let ports: Vec<u16> = match external_backend_ports() {
        Some(external) => {
            tracing::info!(
                target: "manager",
                backend_host = %backend_host,
                ports = ?external,
                "external backend mode — waiting for pre-started worker(s), not spawning"
            );
            external
        }
        None => {
            let mut spawned = Vec::new();
            for i in 0..n_instances {
                let port = start_port + i as u16;
                spawn_instance(&exe_path, port, &auth_token, &backend_host, stealth)?;
                spawned.push(port);
            }
            spawned
        }
    };

    tracing::info!(target: "manager", count = ports.len(), "waiting for backends");
    for &p in &ports {
        wait_for_tcp(&backend_host, p, 90).await?;
        tracing::info!(target: "manager", instance_port = p, "backend ready");
    }

    let (chat_db, db_descr) = open_chat_db().await?;
    tracing::info!(
        target: "manager::db",
        backend = chat_db.dialect(),
        location = %db_descr,
        "chat queue DB opened"
    );

    // Zitadel JWT auth — optional. If ZITADEL_ISSUER is unset, fall back to the
    // legacy shared-token check (handle_client decides based on `jwks`).
    let jwks = match zitadel_auth::ZitadelConfig::from_env() {
        Ok(cfg) => {
            tracing::info!(target: "manager::auth",
                issuer = %cfg.issuer,
                audience = ?cfg.audience,
                project_id = %cfg.project_id,
                "Zitadel auth enabled");
            let cache = zitadel_auth::JwksCache::new(cfg);
            match cache.refresh().await {
                Ok(n) => tracing::info!(target: "manager::auth", keys = n, "JWKS preloaded"),
                Err(e) => {
                    tracing::error!(target: "manager::auth", error = %e,
                        "JWKS preload failed — clients will be rejected until refresh succeeds");
                }
            }
            // Background refresher every hour.
            let bg = cache.clone();
            tokio::spawn(async move {
                let mut t = tokio::time::interval(Duration::from_secs(3600));
                t.tick().await; // skip the immediate tick
                loop {
                    t.tick().await;
                    if let Err(e) = bg.refresh().await {
                        tracing::warn!(target: "manager::auth", error = %e, "JWKS refresh failed");
                    } else {
                        tracing::debug!(target: "manager::auth", "JWKS refreshed");
                    }
                }
            });
            Some(cache)
        }
        Err(reason) => {
            tracing::warn!(target: "manager::auth",
                reason = %reason,
                "Zitadel auth NOT configured — falling back to shared-token auth");
            None
        }
    };

    let state: SharedState = Arc::new(Mutex::new(ManagerState {
        instance_ports: ports.clone(),
        session_to_port: HashMap::new(),
        session_to_owner: HashMap::new(),
        auth_token: auth_token.clone(),
        jwks,
        chat_db,
        clients: HashMap::new(),
    }));

    // bind_host was resolved+validated up front (before any spawning).
    let listener = TcpListener::bind((bind_host.as_str(), manager_port)).await?;
    tracing::info!(
        target: "manager",
        addr = %format!("ws://{}:{}", bind_host, manager_port),
        "manager listening"
    );

    loop {
        let (stream, peer) = listener.accept().await?;
        let state = state.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_client(stream, state).await {
                tracing::warn!(
                    target: "manager::ws",
                    peer = %peer,
                    error = %e,
                    "client error"
                );
            }
        });
    }
}

fn spawn_instance(exe: &str, port: u16, auth_token: &str, backend_host: &str, stealth: bool) -> std::io::Result<()> {
    use std::process::Command;
    let path = std::path::Path::new(exe);
    let canon = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());

    // On Linux the Tauri backend needs a display server even in stealth mode
    // (WebKitGTK still initializes a GtkWindow before we hide it). If neither
    // DISPLAY nor WAYLAND_DISPLAY is set, transparently wrap with `xvfb-run -a`
    // so headless boxes Just Work for the manager use case.
    #[cfg(unix)]
    let xvfb_wrap = {
        let no_display =
            std::env::var_os("DISPLAY").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_none();
        no_display && which_on_path("xvfb-run").is_some()
    };
    #[cfg(not(unix))]
    let xvfb_wrap = false;

    let mut cmd = if xvfb_wrap {
        let mut c = Command::new("xvfb-run");
        c.arg("-a").arg(&canon);
        c
    } else {
        Command::new(&canon)
    };
    cmd.env("LLM_CHAT_WS_PORT", port.to_string())
        .env("LLM_CHAT_AUTH_TOKEN", auth_token)
        // The manager dials backends at backend_host, so the spawned worker
        // must bind that same host. This also supplies the worker's now-
        // required LLM_CHAT_WS_BIND with no hardcoded default.
        .env("LLM_CHAT_WS_BIND", backend_host);
    if stealth {
        cmd.env("LLM_CHAT_STEALTH", "1");
    }
    let child = cmd.spawn()?;
    tracing::info!(
        target: "manager",
        instance_port = port,
        backend_pid = child.id(),
        exe = %canon.display(),
        stealth,
        xvfb = xvfb_wrap,
        "instance spawned"
    );
    // We deliberately drop `child` — backends run independently and we don't
    // want the parent to block on them. Tracking via PID in logs is enough.
    let _ = child;
    Ok(())
}

#[cfg(unix)]
fn which_on_path(name: &str) -> Option<std::path::PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

async fn wait_for_tcp(host: &str, port: u16, retries: u32) -> Result<(), std::io::Error> {
    for _ in 0..retries {
        if TcpStream::connect((host, port)).await.is_ok() {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::TimedOut,
        format!("backend on {}:{} did not come up", host, port),
    ))
}

async fn handle_client(
    stream: TcpStream,
    state: SharedState,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (expected_token, jwks) = {
        let st = state.lock().await;
        (st.auth_token.clone(), st.jwks.clone())
    };
    let path_holder = Arc::new(std::sync::Mutex::new(String::new()));
    let query_holder = Arc::new(std::sync::Mutex::new(String::new()));
    let user_id_holder = Arc::new(std::sync::Mutex::new(None::<String>));
    let roles_holder = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let path_capture = path_holder.clone();
    let query_capture = query_holder.clone();
    let user_id_capture = user_id_holder.clone();
    let roles_capture = roles_holder.clone();
    let cb = move |req: &Request, resp: Response| -> Result<Response, ErrorResponse> {
        *path_capture.lock().unwrap() = req.uri().path().to_string();
        *query_capture.lock().unwrap() = req.uri().query().unwrap_or("").to_string();
        if let Some(origin) = req.headers().get("origin") {
            let s = origin.to_str().unwrap_or("");
            if s.starts_with("http://") || s.starts_with("https://") {
                return Err(http::Response::builder()
                    .status(http::StatusCode::FORBIDDEN)
                    .body(Some("origin not allowed".to_string()))
                    .unwrap());
            }
        }

        // External auth: prefer Zitadel JWT when configured. Fall back to the
        // legacy shared-token check only if Zitadel isn't set up.
        if let Some(jwks) = &jwks {
            let token = match zitadel_auth::extract_bearer(req) {
                Ok(t) => t,
                Err(e) => {
                    return Err(http::Response::builder()
                        .status(http::StatusCode::UNAUTHORIZED)
                        .body(Some(format!("auth: {}", e)))
                        .unwrap());
                }
            };
            let principal = match jwks.verify_sync(&token) {
                Ok(p) => p,
                Err(e) => {
                    return Err(http::Response::builder()
                        .status(http::StatusCode::UNAUTHORIZED)
                        .body(Some(format!("auth: {}", e)))
                        .unwrap());
                }
            };
            // Authorization: every endpoint on the manager requires `chat.user`
            // (admin operations would gate on `chat.admin` later if needed).
            if !principal.has("chat.user") {
                return Err(http::Response::builder()
                    .status(http::StatusCode::FORBIDDEN)
                    .body(Some(format!(
                        "missing role chat.user (principal {} has roles {:?})",
                        principal.user_id, principal.roles
                    )))
                    .unwrap());
            }
            tracing::info!(target: "manager::auth", user_id = %principal.user_id, roles = ?principal.roles, "JWT verified; capturing user id");
            *user_id_capture.lock().unwrap() = Some(principal.user_id.clone());
            *roles_capture.lock().unwrap() = principal.roles.clone();
            return Ok(resp);
        }

        let provided = match extract_token(req) {
            Some(t) => t,
            None => {
                return Err(http::Response::builder()
                    .status(http::StatusCode::UNAUTHORIZED)
                    .body(Some("missing auth token".to_string()))
                    .unwrap());
            }
        };
        if !check_token_eq(&provided, &expected_token) {
            return Err(http::Response::builder()
                .status(http::StatusCode::UNAUTHORIZED)
                .body(Some("invalid auth token".to_string()))
                .unwrap());
        }
        Ok(resp)
    };
    let ws = tokio_tungstenite::accept_hdr_async(stream, cb).await?;
    let req_path = path_holder.lock().unwrap().clone();
    let req_query = query_holder.lock().unwrap().clone();
    let user_id = user_id_holder.lock().unwrap().clone();
    tracing::info!(target: "manager", path = %req_path, query = %redact_query(&req_query), user_id = ?user_id, "post-handshake routing");

    if req_path == "/control" {
        let uid = match user_id {
            Some(u) => u,
            None => return reject_no_user(ws).await,
        };
        // /control is an OPS surface: `list` exposes every user's session ids,
        // `close` kills any session, `history` reads any session's transcript.
        // chat.user alone must NOT reach it — require chat.admin (fail closed).
        let roles = roles_holder.lock().unwrap().clone();
        if !roles.iter().any(|r| r == "chat.admin") {
            return reject_forbidden(ws, "control requires the chat.admin role").await;
        }
        return handle_control(ws, state, uid).await;
    }
    if req_path == "/chat" {
        // /chat accepts an optional `?cwd=<urlencoded-path>` so the client
        // can ask claude to run in a specific directory. The worker
        // canonicalizes and trust-marks the path before spawn.
        let uid = match user_id {
            Some(u) => u,
            None => return reject_no_user(ws).await,
        };
        let cwd = parse_query_param(&req_query, "cwd");
        return handle_chat(ws, state, uid, cwd).await;
    }
    if req_path == "/s/new" {
        let uid = match user_id {
            Some(u) => u,
            None => return reject_no_user(ws).await,
        };
        return bridge_session_auto(ws, state, uid).await;
    }
    if req_path.starts_with("/s/") {
        // Attaching to a live PTY (read output AND inject input) — require an
        // authenticated caller who owns the session, or chat.admin. Fail closed.
        let uid = match user_id {
            Some(u) => u,
            None => return reject_no_user(ws).await,
        };
        let sid = req_path[3..].to_string();
        let is_admin = roles_holder.lock().unwrap().iter().any(|r| r == "chat.admin");
        if !caller_may_bridge(&state, &sid, &uid, is_admin).await {
            return reject_forbidden(ws, "not the owner of this session").await;
        }
        return bridge_session(ws, state, &sid, "/s/").await;
    }
    if req_path.starts_with("/qa/") {
        // Reading another user's parsed Q&A stream is a confidentiality leak —
        // same owner-or-admin gate as /s/. Fail closed.
        let uid = match user_id {
            Some(u) => u,
            None => return reject_no_user(ws).await,
        };
        let sid = req_path[4..].to_string();
        let is_admin = roles_holder.lock().unwrap().iter().any(|r| r == "chat.admin");
        if !caller_may_bridge(&state, &sid, &uid, is_admin).await {
            return reject_forbidden(ws, "not the owner of this session").await;
        }
        return bridge_session(ws, state, &sid, "/qa/").await;
    }
    if req_path == "/" || req_path.is_empty() {
        let is_admin = roles_holder.lock().unwrap().iter().any(|r| r == "chat.admin");
        return handle_root(ws, state, user_id, is_admin).await;
    }

    let (mut sink, _) = ws.split();
    let _ = sink
        .send(Message::Text(format!("unknown path: {}", req_path)))
        .await;
    Ok(())
}

// ---------- /control ----------

async fn handle_control(
    ws: tokio_tungstenite::WebSocketStream<TcpStream>,
    state: SharedState,
    user_id: String,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (mut sink, mut stream) = ws.split();
    let _ = sink
        .send(Message::Text(
            serde_json::json!({"ok":true,"hello":"manager-control"}).to_string(),
        ))
        .await;

    while let Some(msg) = stream.next().await {
        let text = match msg {
            Ok(Message::Text(t)) => t,
            Ok(Message::Close(_)) => break,
            Ok(_) => continue,
            Err(_) => break,
        };
        let req: serde_json::Value = serde_json::from_str(&text).unwrap_or(serde_json::json!({}));
        let cmd = req.get("cmd").and_then(|v| v.as_str()).unwrap_or("");
        let reply = match cmd {
            "instances" => {
                let st = state.lock().await;
                let ports = st.instance_ports.clone();
                let mut counts = serde_json::Map::new();
                for p in &ports {
                    let c = st.session_to_port.values().filter(|x| **x == *p).count();
                    counts.insert(p.to_string(), serde_json::json!(c));
                }
                serde_json::json!({"ok":true,"ports":ports,"sessionsPerPort":counts})
            }
            "open" => match cmd_open(&state, &user_id, None).await {
                Ok((sid, port, transport)) => {
                    serde_json::json!({"ok":true,"sessionId":sid,"backendPort":port,"transport":transport})
                }
                Err(e) => serde_json::json!({"ok":false,"error":e.to_string()}),
            },
            "close" => {
                let sid = req
                    .get("sessionId")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                match cmd_close(&state, &sid).await {
                    Ok(_) => serde_json::json!({"ok":true,"sessionId":sid}),
                    Err(e) => serde_json::json!({"ok":false,"error":e.to_string()}),
                }
            }
            "list" | "info" => {
                let ports = state.lock().await.instance_ports.clone();
                let mut all: Vec<String> = Vec::new();
                let mut per_backend = serde_json::Map::new();
                for p in &ports {
                    let resp = call_backend(*p, serde_json::json!({"cmd":"list"})).await;
                    match resp {
                        Ok(v) => {
                            if let Some(arr) = v.get("sessions").and_then(|x| x.as_array()) {
                                for s in arr {
                                    if let Some(s) = s.as_str() {
                                        all.push(s.to_string());
                                    }
                                }
                                per_backend.insert(p.to_string(), v);
                            }
                        }
                        Err(e) => {
                            per_backend
                                .insert(p.to_string(), serde_json::json!({"error":e.to_string()}));
                        }
                    }
                }
                serde_json::json!({
                    "ok": true,
                    "count": all.len(),
                    "sessions": all,
                    "byBackend": per_backend,
                })
            }
            "history" => {
                let sid = req.get("sessionId").and_then(|v| v.as_str());
                let mut req_to_backend = serde_json::json!({"cmd": "history"});
                if let Some(sid) = sid {
                    let port = lookup_port(&state, sid).await;
                    match port {
                        Some(p) => {
                            req_to_backend["sessionId"] = serde_json::json!(sid);
                            match call_backend(p, req_to_backend).await {
                                Ok(v) => v,
                                Err(e) => serde_json::json!({"ok":false,"error":e.to_string()}),
                            }
                        }
                        None => {
                            serde_json::json!({"ok":false,"error":"unknown sessionId"})
                        }
                    }
                } else {
                    // Aggregate from every backend
                    let ports = state.lock().await.instance_ports.clone();
                    let mut all = serde_json::Map::new();
                    for p in &ports {
                        if let Ok(v) = call_backend(*p, serde_json::json!({"cmd":"history"})).await
                        {
                            if let Some(map) = v.get("histories").and_then(|x| x.as_object()) {
                                for (k, val) in map {
                                    all.insert(k.clone(), val.clone());
                                }
                            }
                        }
                    }
                    serde_json::json!({"ok":true,"histories":all})
                }
            }
            "clients" => {
                // Live registry: who's currently connected to /chat right
                // now (separate from chat_question, which is the historical
                // record of what they sent).
                let st = state.lock().await;
                let mut list: Vec<ClientInfo> = st.clients.values().cloned().collect();
                list.sort_by(|a, b| a.connected_at.cmp(&b.connected_at));
                serde_json::json!({"ok": true, "count": list.len(), "clients": list})
            }
            "queue" => {
                // Inspect the manager's OWN /chat queue (chat_question table
                // in manager.sqlite). Filters: connectionId, sid, status, limit.
                let connection_id = req
                    .get("connectionId")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let sid_filter = req
                    .get("sid")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let status_filter = req
                    .get("status")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let limit = req
                    .get("limit")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(100)
                    .clamp(1, 1000);
                let db = state.lock().await.chat_db.clone();
                match query_chat_queue(
                    &db,
                    connection_id.as_deref(),
                    sid_filter.as_deref(),
                    status_filter.as_deref(),
                    limit,
                )
                .await
                {
                    Ok(rows) => serde_json::json!({"ok":true,"count":rows.len(),"rows":rows}),
                    Err(e) => serde_json::json!({"ok":false,"error":format!("queue query: {}", e)}),
                }
            }
            "usage" => {
                let db = state.lock().await.chat_db.clone();
                match db.usage_by_user().await {
                    Ok(rows) => compose_usage_reply(&rows),
                    Err(e) => serde_json::json!({"ok": false, "error": format!("usage query: {e}")}),
                }
            }
            "usage-daily" => {
                let cutoff = (chrono::Utc::now() - chrono::Duration::days(30))
                    .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
                let db = state.lock().await.chat_db.clone();
                match db.usage_daily(&cutoff).await {
                    Ok(rows) => compose_daily_reply(&rows),
                    Err(e) => serde_json::json!({"ok": false, "error": format!("usage-daily query: {e}")}),
                }
            }
            "fifo" => {
                // Inspect a backend's PTY input FIFO. Target by `port` or
                // `sessionId`; with neither, aggregate across all backends.
                // Optional filters forwarded verbatim: sid, status, limit.
                let target_port: Option<u16> = match req.get("port").and_then(|v| v.as_u64()) {
                    Some(p) => Some(p as u16),
                    None => match req.get("sessionId").and_then(|v| v.as_str()) {
                        Some(sid) => lookup_port(&state, sid).await,
                        None => None,
                    },
                };
                let mut backend_req = serde_json::json!({"cmd":"fifo"});
                for k in ["sid", "status", "limit"] {
                    if let Some(v) = req.get(k) {
                        backend_req[k] = v.clone();
                    }
                }
                if let Some(p) = target_port {
                    match call_backend(p, backend_req).await {
                        Ok(v) => v,
                        Err(e) => serde_json::json!({"ok":false,"error":e.to_string()}),
                    }
                } else {
                    // Aggregate across all backends.
                    let ports = state.lock().await.instance_ports.clone();
                    let mut by_backend = serde_json::Map::new();
                    for p in &ports {
                        let r = call_backend(*p, backend_req.clone()).await.unwrap_or_else(
                            |e| serde_json::json!({"ok":false,"error":e.to_string()}),
                        );
                        by_backend.insert(p.to_string(), r);
                    }
                    serde_json::json!({"ok":true,"byBackend":by_backend})
                }
            }
            "screenshot" => {
                // Capture one backend (by port or sessionId), or all backends
                // when nothing is specified. Reply lists the PNG paths each
                // backend wrote.
                let target_port: Option<u16> = match req.get("port").and_then(|v| v.as_u64()) {
                    Some(p) => Some(p as u16),
                    None => match req.get("sessionId").and_then(|v| v.as_str()) {
                        Some(sid) => lookup_port(&state, sid).await,
                        None => None,
                    },
                };
                let ports: Vec<u16> = match target_port {
                    Some(p) => vec![p],
                    None => state.lock().await.instance_ports.clone(),
                };
                let mut shots = serde_json::Map::new();
                for p in &ports {
                    let r = call_backend(*p, serde_json::json!({"cmd":"screenshot"})).await;
                    let entry = match r {
                        Ok(v) => v,
                        Err(e) => serde_json::json!({"ok":false,"error":e.to_string()}),
                    };
                    shots.insert(p.to_string(), entry);
                }
                serde_json::json!({"ok":true,"byPort":shots})
            }
            "switch" | "clear" | "current" | "log" => {
                let sid = req
                    .get("sessionId")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let port_opt = match &sid {
                    Some(s) => lookup_port(&state, s).await,
                    None => state.lock().await.instance_ports.first().copied(),
                };
                match port_opt {
                    Some(p) => match call_backend(p, req.clone()).await {
                        Ok(v) => v,
                        Err(e) => serde_json::json!({"ok":false,"error":e.to_string()}),
                    },
                    None => serde_json::json!({"ok":false,"error":"no backend available"}),
                }
            }
            other => serde_json::json!({"ok":false,"error":format!("unknown cmd: {}", other)}),
        };
        if sink.send(Message::Text(reply.to_string())).await.is_err() {
            break;
        }
    }
    Ok(())
}

async fn lookup_port(state: &SharedState, sid: &str) -> Option<u16> {
    state.lock().await.session_to_port.get(sid).copied()
}

async fn pick_least_loaded_port(state: &SharedState) -> Option<u16> {
    let st = state.lock().await;
    let mut counts: HashMap<u16, usize> = st.instance_ports.iter().map(|p| (*p, 0usize)).collect();
    for &p in st.session_to_port.values() {
        *counts.entry(p).or_insert(0) += 1;
    }
    counts.into_iter().min_by_key(|(_, c)| *c).map(|(p, _)| p)
}

/// Build the worker `open` command body. The user id is REQUIRED (the worker
/// confines every spawn under {base}/{userId}); the relative subpath is added
/// only when present.
fn open_request_body(user_id: &str, subpath: Option<&str>) -> serde_json::Value {
    let mut body = serde_json::json!({"cmd":"open","userId": user_id});
    if let Some(p) = subpath {
        body["cwd"] = serde_json::Value::String(p.to_string());
    }
    body
}

async fn cmd_open(
    state: &SharedState,
    user_id: &str,
    subpath: Option<&str>,
) -> Result<(String, u16, String), Box<dyn std::error::Error + Send + Sync>> {
    let port = pick_least_loaded_port(state)
        .await
        .ok_or("no backends configured")?;
    let body = open_request_body(user_id, subpath);
    tracing::info!(target: "manager", port, user_id, subpath = ?subpath, "cmd_open → worker (open)");
    let resp = call_backend(port, body).await?;
    tracing::info!(target: "manager", resp = %resp, "cmd_open ← worker response");
    let sid = resp
        .get("sessionId")
        .and_then(|v| v.as_str())
        .ok_or("backend did not return sessionId")?
        .to_string();
    // The backend reports its transport so we can skip PTY-era timing hacks
    // (warmup/settle) for the stream-json path. Default to stream-json — that
    // is the backend default, and an older backend that omits the field is the
    // only case we'd guess on.
    let transport = resp
        .get("transport")
        .and_then(|v| v.as_str())
        .unwrap_or("stream-json")
        .to_string();
    {
        let mut st = state.lock().await;
        st.session_to_port.insert(sid.clone(), port);
        // Bind the session to the authenticated opener so /s/ and /qa/ attach
        // can verify ownership (fail-closed authz for cross-user isolation).
        st.session_to_owner.insert(sid.clone(), user_id.to_string());
    }
    Ok((sid, port, transport))
}

async fn cmd_close(
    state: &SharedState,
    sid: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let port = lookup_port(state, sid).await.ok_or("unknown sessionId")?;
    let _ = call_backend(port, serde_json::json!({"cmd":"close","sessionId":sid})).await?;
    {
        let mut st = state.lock().await;
        st.session_to_port.remove(sid);
        st.session_to_owner.remove(sid);
    }
    Ok(())
}

/// Build a tungstenite request to a backend with the auth token attached.
fn auth_request(
    url: &str,
    token: &str,
) -> Result<
    tokio_tungstenite::tungstenite::handshake::client::Request,
    Box<dyn std::error::Error + Send + Sync>,
> {
    let mut req = url.into_client_request()?;
    req.headers_mut()
        .insert("Authorization", format!("Bearer {}", token).parse()?);
    Ok(req)
}

/// Open a fresh /control WS to a backend, send one command, read one reply,
/// close. Useful for one-shot RPC calls from the manager.
async fn call_backend(
    port: u16,
    req: serde_json::Value,
) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
    // Read the token from disk every call — manager wrote it at startup.
    // (For perf this could be cached on AppState; left simple for now.)
    let token = std::fs::read_to_string(auth_token_path())?
        .trim()
        .to_string();
    let url = format!("ws://{}:{}/control", backend_host(), port);
    let (mut ws, _) = connect_async(auth_request(&url, &token)?).await?;
    // Discard the initial hello banner
    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), ws.next()).await;
    ws.send(Message::Text(req.to_string())).await?;
    let reply = match tokio::time::timeout(std::time::Duration::from_secs(15), ws.next()).await {
        Ok(Some(Ok(Message::Text(t)))) => serde_json::from_str(&t)?,
        Ok(Some(Ok(other))) => return Err(format!("non-text reply: {:?}", other).into()),
        Ok(Some(Err(e))) => return Err(Box::new(e)),
        Ok(None) => return Err("backend closed".into()),
        Err(_) => return Err("backend timeout".into()),
    };
    let _ = ws.send(Message::Close(None)).await;
    Ok(reply)
}

// ---------- /s/<sid> and /qa/<sid> bridge ----------

/// Authorize a caller to attach to an EXISTING session's `/s/` or `/qa/`
/// stream. A `chat.admin` operator may attach to any session (same ops posture
/// as `/control`); every other caller MUST be the session's authenticated
/// owner. Fails closed: an unknown session, or one with no recorded owner, is
/// rejected — never attachable by a non-owner. Without this gate any
/// authenticated `chat.user` could attach to (read `/qa/`, or inject input on
/// `/s/`) another user's Claude session, defeating per-user confinement.
async fn caller_may_bridge(state: &SharedState, sid: &str, uid: &str, is_admin: bool) -> bool {
    if is_admin {
        return true;
    }
    match state.lock().await.session_to_owner.get(sid) {
        Some(owner) => owner == uid,
        None => false,
    }
}

async fn bridge_session(
    ws: tokio_tungstenite::WebSocketStream<TcpStream>,
    state: SharedState,
    sid: &str,
    base: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let port = match lookup_port(&state, sid).await {
        Some(p) => p,
        None => {
            let (mut sink, _) = ws.split();
            let _ = sink
                .send(Message::Text(format!("unknown sessionId: {}", sid)))
                .await;
            return Ok(());
        }
    };
    let token = state.lock().await.auth_token.clone();
    bridge_to_backend(ws, port, sid, base, &token).await
}

/// `/s/new` — auto-spawn one session in the least-loaded backend, send the
/// real sid back to the client as the first text frame, bridge the PTY both
/// ways, then auto-close the session when the client disconnects. Lets a
/// client get a private session with a single WebSocket connection.
async fn bridge_session_auto(
    mut ws: tokio_tungstenite::WebSocketStream<TcpStream>,
    state: SharedState,
    user_id: String,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (sid, port, _transport) = match cmd_open(&state, &user_id, None).await {
        Ok(x) => x,
        Err(e) => {
            let _ = ws
                .send(Message::Text(format!("auto-open failed: {}", e)))
                .await;
            return Ok(());
        }
    };
    tracing::info!(
        target: "manager::s_new",
        sid = %sid,
        backend_port = port,
        "auto-spawn session"
    );

    let hello = serde_json::json!({"sid": sid, "backendPort": port}).to_string();
    if ws.send(Message::Text(hello)).await.is_err() {
        // Client gave up before the bridge even started.
        let _ = cmd_close(&state, &sid).await;
        return Ok(());
    }
    // Force the hello onto the wire BEFORE we connect to the backend so the
    // client receives it as the first frame, ahead of the backend's own
    // "connected to session …" banner.
    let _ = ws.flush().await;

    let token = state.lock().await.auth_token.clone();
    let bridge_result = bridge_to_backend(ws, port, &sid, "/s/", &token).await;
    tracing::info!(
        target: "manager::s_new",
        sid = %sid,
        "client disconnected → auto-close"
    );
    let _ = cmd_close(&state, &sid).await;
    bridge_result
}

// ---------- /chat (typed JSON protocol with FIFO id pairing) ----------
//
// Single-connection chat endpoint. Manager auto-spawns one session in the
// least-loaded backend per /chat connection, then exchanges typed JSON:
//
//   server → {"type":"initialized","sid":"s17773...","timeOut":"…"}        (once)
//   client → {"type":"q","id":"<opaque>","text":"<question>"}              (N times)
//   server → {"type":"ack","id":"<opaque>","seq":<server-seq>,
//             "timeIn":"<RFC3339 of when q arrived>"}                       (immediately, once per q)
//   server → {"type":"a","id":"<opaque>","seq":<server-seq>,
//             "text":"<answer>",
//             "timeIn":"<RFC3339 of when q arrived>",
//             "timeOut":"<RFC3339 of when a was sent>"}                    (once per q, after parser settles)
//   client → {"type":"confirm","id":"<opaque?>","seq":<server-seq>}        (after `a` arrives)
//   server (silent) → UPDATE row: status='confirmed', time_confirmed=now
//   server → {"type":"err","id":"<opaque?>","text":"<reason>","timeOut":"…"} (on failure)
//
// Status state machine on each row:
//   pending → sent → answered → confirmed   (full happy path)
//   any → error                              (failure)
// A row stuck at `answered` means the answer was sent to the client but the
// client never confirmed receipt — useful for at-least-once delivery audits.
//
// `seq` is the server-assigned, monotonically increasing receipt id (the row's
// PK in the chat_question queue). It appears in both `ack` and `a` so a client
// can correlate either its own opaque `id` or the server's `seq`.
//
// FIFO pairing: questions are inserted into the queue in arrival order. The
// Nth final answer the parser emits is paired with the Nth queued row and
// returned to the client stamped with seq + timeIn + timeOut. All timestamps
// are ISO 8601 / RFC 3339 in UTC with millisecond precision.
//
// On client disconnect, manager closes the session.

/// Current UTC time in RFC 3339 / ISO 8601 with ms precision.
/// PURE: decoded byte length of a base64 string, computed from its length (no
/// decode, no dependency). Exact for well-formed base64; 0 for empty.
fn b64_decoded_len(s: &str) -> i64 {
    if s.is_empty() {
        return 0;
    }
    let pad = s.bytes().rev().take_while(|&b| b == b'=').count();
    (s.len() / 4 * 3 - pad) as i64
}

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// Milliseconds between two RFC3339 timestamps (`time_out - time_in`). Returns
/// -1 if either fails to parse, so a bad value shows up in logs rather than
/// being silently coerced to 0.
fn latency_ms(time_in: &str, time_out: &str) -> i64 {
    match (
        chrono::DateTime::parse_from_rfc3339(time_in),
        chrono::DateTime::parse_from_rfc3339(time_out),
    ) {
        (Ok(a), Ok(b)) => (b - a).num_milliseconds(),
        _ => -1,
    }
}

/// Pair one finalized answer with the oldest outstanding question for this
/// connection (FIFO), persist it, and forward it to the client — logging the
/// derived `latency_ms` (question received → answer forwarded).
///
/// Factored out so the stream-json path can call it INLINE in the qa loop
/// (serializing `pop_sent` + forward, which also closes a latent race: the
/// SELECT-then-UPDATE in pop_sent/mark_answered is not atomic, so two
/// concurrent flushes could otherwise pop the same oldest row). The legacy PTY
/// path still calls it after a debounce. `num` is for log correlation only.
async fn flush_answer(
    db: &ChatDb,
    connection_id: &str,
    num: u32,
    final_text: String,
    answer_tx: &tokio::sync::mpsc::Sender<serde_json::Value>,
) {
    // Pop the oldest 'sent' row for this connection (FIFO via ORDER BY seq ASC).
    // Distinguish a DB error from a genuinely empty queue: a swallowed error
    // would leave the row 'sent' and silently mispair the NEXT answer.
    let row = match db.pop_sent(connection_id).await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!(
                target: "manager::chat",
                num,
                connection_id = %connection_id,
                error = %e,
                "pop_sent failed — cannot pair this answer (row stays 'sent')"
            );
            return;
        }
    };
    let (seq, q_id, time_in) = match row {
        Some(r) => r,
        None => {
            tracing::warn!(
                target: "manager::chat",
                num,
                connection_id = %connection_id,
                "answer with no matching pending question"
            );
            return;
        }
    };
    let time_out = now_iso();
    let lat_ms = latency_ms(&time_in, &time_out);
    tracing::info!(
        target: "manager::chat::qa",
        num,
        seq,
        id = %q_id,
        len = final_text.len(),
        latency_ms = lat_ms,
        text = %log_preview(&final_text, 160),
        "answer paired with question (FIFO) — forwarding to client"
    );
    // A failed UPDATE leaves the row 'sent', so the next answer would re-pop and
    // mispair it. Log loudly — the SELECT-then-UPDATE here is not atomic (a fully
    // race-free fix is a single claim-and-answer UPDATE...RETURNING, tracked as
    // follow-up); inline execution in the qa loop avoids the concurrent case.
    if let Err(e) = db.mark_answered(seq, &final_text, &time_out).await {
        tracing::error!(
            target: "manager::chat",
            num,
            seq,
            error = %e,
            "mark_answered failed — row stays 'sent'; next answer may mispair"
        );
    }
    let out = serde_json::json!({
        "type": "a",
        "id": q_id,
        "seq": seq,
        "text": final_text,
        "timeIn": time_in,
        "timeOut": time_out,
        "latencyMs": lat_ms,
    });
    let _ = answer_tx.send(out).await;
}

/// Single-line, length-bounded rendering of arbitrary text for logs. Collapses
/// newlines/CR to visible escapes so a multi-line PTY payload stays on one log
/// line, and truncates to `max` chars with an explicit ellipsis so we never
/// dump an unbounded prompt/answer into the trace.
fn log_preview(s: &str, max: usize) -> String {
    let flat = s.replace('\\', "\\\\").replace('\r', "\\r").replace('\n', "\\n");
    let mut out: String = flat.chars().take(max).collect();
    if flat.chars().count() > max {
        out.push('…');
    }
    out
}
/// Reject a session that has no authenticated user id (fail closed — the
/// per-user environment requires one; no fallback). Sends a typed err frame
/// and closes.
async fn reject_no_user(
    mut ws: tokio_tungstenite::WebSocketStream<TcpStream>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let _ = ws
        .send(Message::Text(
            serde_json::json!({
                "type": "err",
                "text": "per-user environment requires an authenticated user id"
            })
            .to_string(),
        ))
        .await;
    let _ = ws.close(None).await;
    Ok(())
}

/// Reject an authenticated-but-unauthorized connection (e.g. /control without
/// chat.admin). Mirrors reject_no_user: one typed err frame, then close.
async fn reject_forbidden(
    mut ws: tokio_tungstenite::WebSocketStream<TcpStream>,
    why: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let _ = ws
        .send(Message::Text(
            serde_json::json!({ "type": "err", "text": why }).to_string(),
        ))
        .await;
    let _ = ws.close(None).await;
    Ok(())
}

async fn handle_chat(
    ws: tokio_tungstenite::WebSocketStream<TcpStream>,
    state: SharedState,
    user_id: String,
    cwd: Option<String>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use std::time::Duration;
    use tokio::sync::mpsc;

    // 1. Spawn a fresh session in the least-loaded backend. If the client
    //    asked for a specific working directory via `?cwd=…`, the worker
    //    canonicalizes + trust-marks it and runs claude there.
    let (sid, port, transport) = match cmd_open(&state, &user_id, cwd.as_deref()).await {
        Ok(x) => x,
        Err(e) => {
            let mut ws = ws;
            let err = serde_json::json!({
                "type": "err",
                "text": format!("open failed: {}", e),
                "timeOut": now_iso(),
            });
            let _ = ws.send(Message::Text(err.to_string())).await;
            return Ok(());
        }
    };
    let connection_id = uuid::Uuid::new_v4().to_string();
    tracing::info!(
        target: "manager::chat",
        sid = %sid,
        backend_port = port,
        connection_id = %connection_id,
        "session opened"
    );

    // Register in the live clients map so /control "clients" sees us.
    {
        let mut st = state.lock().await;
        st.clients.insert(
            connection_id.clone(),
            ClientInfo {
                connection_id: connection_id.clone(),
                sid: sid.clone(),
                user_id: user_id.clone(),
                backend_port: port,
                connected_at: now_iso(),
                last_q_at: None,
                questions_sent: 0,
            },
        );
    }

    let (token, db) = {
        let st = state.lock().await;
        (st.auth_token.clone(), st.chat_db.clone())
    };
    let (mut ws_sink, mut ws_stream) = ws.split();

    // 2. Initialized — let the client know the sid + connection_id (so they
    //    can later inspect their own queue rows in the SQLite file if they want).
    let initialized = serde_json::json!({
        "type": "initialized",
        "sid": sid,
        "backendPort": port,
        "connectionId": connection_id,
        "timeOut": now_iso(),
    })
    .to_string();
    if ws_sink.send(Message::Text(initialized)).await.is_err() {
        let _ = cmd_close(&state, &sid).await;
        return Ok(());
    }

    // 3. Connect to backend's /s/<sid> for sending PTY input AND to /qa/<sid>
    //    for receiving parsed Q&A events.
    let s_url = format!("ws://{}:{}/s/{}", backend_host(), port, sid);
    let s_req = auth_request(&s_url, &token)?;
    let (s_ws, _) = match connect_async(s_req).await {
        Ok(p) => p,
        Err(e) => {
            let err = serde_json::json!({
                "type":"err",
                "text":format!("backend /s/ connect: {}", e),
                "timeOut": now_iso(),
            });
            let _ = ws_sink.send(Message::Text(err.to_string())).await;
            let _ = cmd_close(&state, &sid).await;
            return Ok(());
        }
    };
    let (mut s_sink, _s_stream_unused) = s_ws.split();

    let qa_url = format!("ws://{}:{}/qa/{}", backend_host(), port, sid);
    let qa_req = auth_request(&qa_url, &token)?;
    let (qa_ws, _) = match connect_async(qa_req).await {
        Ok(p) => p,
        Err(e) => {
            let err = serde_json::json!({
                "type":"err",
                "text":format!("backend /qa/ connect: {}", e),
                "timeOut": now_iso(),
            });
            let _ = ws_sink.send(Message::Text(err.to_string())).await;
            let _ = cmd_close(&state, &sid).await;
            return Ok(());
        }
    };
    let (_qa_sink_unused, mut qa_stream) = qa_ws.split();

    // Cold-start grace period. This is a PTY/TUI-era hack: the claude CLI takes
    // ~5–8 s to reach its prompt after PTY spawn, AND the JS parser needs to
    // register an onData hook on the new xterm tab before PTY traffic starts —
    // otherwise the first q is typed into claude's welcome screen and no
    // `>` / `●` markers ever emit on /qa. NONE of that applies to the
    // stream-json transport: it is pure Rust with no webview, and claude reads
    // stdin from the start (input buffers until it is ready), so warmup is 0
    // there. PTY keeps the 8 s default. An explicit MANAGER_CHAT_WARMUP_SECS
    // overrides both (0 also lets tests skip a wait they've handled another way).
    let warmup = std::env::var("MANAGER_CHAT_WARMUP_SECS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(if transport == "pty" { 8 } else { 0 });
    if warmup > 0 {
        tracing::debug!(target: "manager::chat", sid = %sid, warmup_secs = warmup,
            "warming up newly-spawned session before processing first q");
        tokio::time::sleep(Duration::from_secs(warmup)).await;
    }
    tracing::info!(target: "manager::chat", sid = %sid, warmup_secs = warmup,
        "warmup complete — ready to process first q");

    // 4. Channel that carries finalized answers/errors from the qa+reader tasks
    //    to the writer task that flushes to the client WS.
    let (answer_tx, mut answer_rx) = mpsc::channel::<serde_json::Value>(16);

    // ---- Reader task ----
    // Client `q` → INSERT row (status='pending', auto-incremented seq) → write
    // to backend PTY → UPDATE status='sent'. SQLite is the durable FIFO; the
    // qa task pops the next 'sent' row in seq order.
    let db_for_in = db.clone();
    let connection_id_for_in = connection_id.clone();
    let sid_for_in = sid.clone();
    let answer_tx_for_in = answer_tx.clone();
    let state_for_in = state.clone();
    let user_id_for_in = user_id.clone();
    let reader = tokio::spawn(async move {
        while let Some(msg) = ws_stream.next().await {
            let text = match msg {
                Ok(Message::Text(t)) => t,
                Ok(Message::Close(_)) => break,
                Ok(_) => continue,
                Err(_) => break,
            };
            let time_in = now_iso();
            let v: serde_json::Value = match serde_json::from_str(&text) {
                Ok(v) => v,
                Err(e) => {
                    let _ = answer_tx_for_in
                        .send(serde_json::json!({
                            "type":"err",
                            "text":format!("bad JSON: {}", e),
                            "timeIn": time_in,
                            "timeOut": now_iso(),
                        }))
                        .await;
                    continue;
                }
            };
            let msg_type = v.get("type").and_then(|x| x.as_str()).unwrap_or("");
            if msg_type == "confirm" {
                // Client confirms it received an `a` for this seq. Mark the
                // row 'confirmed' with the time we received the confirmation.
                let seq = v.get("seq").and_then(|x| x.as_i64()).unwrap_or(-1);
                if seq < 0 {
                    let _ = answer_tx_for_in
                        .send(serde_json::json!({
                            "type":"err","text":"confirm without seq",
                            "timeIn": time_in,"timeOut": now_iso(),
                        }))
                        .await;
                    continue;
                }
                match db_for_in.mark_confirmed(seq, &time_in).await {
                    Ok(true) => tracing::debug!(target:"manager::chat", seq, "confirmed"),
                    Ok(false) => {
                        tracing::warn!(target:"manager::chat", seq, "confirm for non-answered or unknown seq")
                    }
                    Err(e) => {
                        tracing::error!(target:"manager::chat::db", error=%e, seq, "mark_confirmed failed")
                    }
                }
                continue;
            }
            if msg_type == "usage" {
                // Reply with THIS connection's authenticated user's OWN usage
                // (lifetime totals + last 7 days). chat.user-scoped: the caller
                // can never see another user's data — user_id_for_in is the
                // verified JWT sub captured at handshake, not client-supplied.
                let cutoff = (chrono::Utc::now() - chrono::Duration::days(7))
                    .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
                let reply = match db_for_in.usage_for(&user_id_for_in).await {
                    Ok(total) => match db_for_in.usage_daily_for(&user_id_for_in, &cutoff).await {
                        Ok(daily) => compose_own_usage_reply(&user_id_for_in, &total, &daily),
                        Err(e) => serde_json::json!({"type":"err",
                            "text":format!("usage daily query failed: {e}"),
                            "timeIn":time_in,"timeOut":now_iso()}),
                    },
                    Err(e) => serde_json::json!({"type":"err",
                        "text":format!("usage query failed: {e}"),
                        "timeIn":time_in,"timeOut":now_iso()}),
                };
                let _ = answer_tx_for_in.send(reply).await;
                continue;
            }
            if msg_type != "q" {
                let _ = answer_tx_for_in
                    .send(serde_json::json!({
                        "type":"err",
                        "text":format!("unknown type \"{}\"; expected \"q\", \"confirm\", or \"usage\"", msg_type),
                        "timeIn": time_in,
                        "timeOut": now_iso(),
                    }))
                    .await;
                continue;
            }
            let id = v
                .get("id")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            let q_text = v
                .get("text")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            tracing::info!(
                target: "manager::chat",
                sid = %sid_for_in,
                id = %id,
                q_len = q_text.len(),
                q = %log_preview(&q_text, 120),
                has_attachments = v.get("attachments").is_some(),
                "q received from client"
            );
            if q_text.is_empty() {
                let _ = answer_tx_for_in
                    .send(serde_json::json!({
                        "type":"err","id":id,"text":"empty question",
                        "timeIn": time_in,"timeOut": now_iso(),
                    }))
                    .await;
                continue;
            }

            // Forward any attachments to the backend FIRST so we get back
            // the absolute on-disk paths, then prepend `@<path>` markers to
            // the text claude will actually see. Failures stop the whole q.
            let attachments = v.get("attachments").and_then(|x| x.as_array()).cloned();
            let mut saved_paths: Vec<String> = Vec::new();
            if let Some(arr) = attachments {
                let mut hard_err: Option<String> = None;
                for att in arr {
                    let name = att
                        .get("name")
                        .and_then(|x| x.as_str())
                        .unwrap_or("attachment")
                        .to_string();
                    let mime = att
                        .get("mime")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .to_string();
                    let data = att
                        .get("data")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .to_string();
                    if mime.is_empty() || data.is_empty() {
                        hard_err = Some(format!("attachment '{}' missing mime or data", name));
                        break;
                    }
                    let req_payload = serde_json::json!({
                        "cmd": "save_attachment",
                        "sid": sid_for_in,
                        "name": name,
                        "mime": mime,
                        "data": data,
                    });
                    match call_backend(port, req_payload).await {
                        Ok(reply) => {
                            if reply.get("ok").and_then(|v| v.as_bool()) == Some(true) {
                                if let Some(p) = reply.get("path").and_then(|v| v.as_str()) {
                                    saved_paths.push(p.to_string());
                                } else {
                                    hard_err =
                                        Some("backend save_attachment returned no path".into());
                                    break;
                                }
                            } else {
                                let err = reply
                                    .get("error")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("unknown error")
                                    .to_string();
                                hard_err = Some(format!("backend save_attachment: {}", err));
                                break;
                            }
                        }
                        Err(e) => {
                            hard_err = Some(format!("backend call: {}", e));
                            break;
                        }
                    }
                }
                if let Some(err) = hard_err {
                    let _ = answer_tx_for_in
                        .send(serde_json::json!({
                            "type":"err","id":id,"text":err,
                            "timeIn":time_in,"timeOut":now_iso(),
                        }))
                        .await;
                    continue;
                }
            }

            // Tell claude about each attachment via plain text + an
            // explicit instruction to use its Read tool. The naive `@<path>`
            // approach doesn't work in our PTY-driven flow because `@` opens
            // claude's interactive file-picker menu (which expects keyboard
            // navigation, not a typed path) — the prompt then never submits.
            // Read-tool wording is reliable: claude reads the file as part
            // of normal chat handling.
            let final_text = if saved_paths.is_empty() {
                q_text.clone()
            } else {
                let prefix = saved_paths
                    .iter()
                    .map(|p| format!("Read the file at {}.", p))
                    .collect::<Vec<_>>()
                    .join(" ");
                format!("{} {}", prefix, q_text)
            };
            let attachment_paths_json = if saved_paths.is_empty() {
                None
            } else {
                serde_json::to_string(&saved_paths).ok()
            };

            // Self-counted per-user usage (NOT claude's account-level usage):
            // chars of the user's question, attachment count + decoded bytes.
            let chars_in = q_text.chars().count() as i64;
            let files = saved_paths.len() as i64;
            let file_bytes: i64 = v
                .get("attachments")
                .and_then(|x| x.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|att| att.get("data").and_then(|d| d.as_str()))
                        .map(b64_decoded_len)
                        .sum()
                })
                .unwrap_or(0);

            // INSERT into the FIFO with status='pending'. seq is the FIFO key
            // AND the server-assigned receipt id we return in `ack` and `a`.
            let seq = match db_for_in
                .insert_pending(
                    &connection_id_for_in,
                    &sid_for_in,
                    &id,
                    &final_text,
                    &time_in,
                    attachment_paths_json.as_deref(),
                    Some(user_id_for_in.as_str()),
                    chars_in,
                    files,
                    file_bytes,
                )
                .await
            {
                Ok(s) => s,
                Err(e) => {
                    tracing::error!(target:"manager::chat::db", error=%e, "INSERT pending failed");
                    let _ = answer_tx_for_in
                        .send(serde_json::json!({
                            "type":"err","id":id,"text":format!("db insert: {}", e),
                            "timeIn": time_in,"timeOut": now_iso(),
                        }))
                        .await;
                    continue;
                }
            };
            // Acknowledge receipt IMMEDIATELY — before the PTY write. Lets the
            // client correlate q→ack (and later q→a) without waiting for the
            // backend to do anything. seq is the server-side identifier; id
            // is the client's own opaque value echoed back.
            let _ = answer_tx_for_in
                .send(serde_json::json!({
                    "type": "ack",
                    "id": id,
                    "seq": seq,
                    "timeIn": time_in,
                }))
                .await;
            // Write to backend PTY in TWO frames: the text body, a brief
            // pause, then the Enter (\r) keystroke. Claude's TUI input box
            // sometimes fails to submit when text + \r arrive in a single
            // write (especially long prompts that wrap to multiple visual
            // lines) — the human-typing pattern works reliably.
            tracing::info!(
                target: "manager::chat",
                sid = %sid_for_in,
                id = %id,
                seq,
                len = final_text.len(),
                text = %log_preview(&final_text, 160),
                "typing question into backend PTY (body frame)"
            );
            if s_sink
                .send(Message::Text(final_text.clone()))
                .await
                .is_err()
            {
                let _ = db_for_in.update_status(seq, "error").await;
                let _ = answer_tx_for_in
                    .send(serde_json::json!({
                        "type":"err","id":id,"seq":seq,"text":"backend PTY closed",
                        "timeIn": time_in,"timeOut": now_iso(),
                    }))
                    .await;
                break;
            }
            tokio::time::sleep(Duration::from_millis(150)).await;
            tracing::debug!(target: "manager::chat", sid = %sid_for_in, id = %id, seq,
                "sending Enter (\\r) to submit");
            if s_sink.send(Message::Text("\r".to_string())).await.is_err() {
                let _ = db_for_in.update_status(seq, "error").await;
                let _ = answer_tx_for_in
                    .send(serde_json::json!({
                        "type":"err","id":id,"seq":seq,"text":"backend PTY closed (enter)",
                        "timeIn": time_in,"timeOut": now_iso(),
                    }))
                    .await;
                break;
            }
            let _ = db_for_in.update_status(seq, "sent").await;
            // Bump the client's live counter so /control "clients" reflects activity.
            {
                let mut st = state_for_in.lock().await;
                if let Some(info) = st.clients.get_mut(&connection_id_for_in) {
                    info.last_q_at = Some(time_in.clone());
                    info.questions_sent += 1;
                }
            }
            tracing::debug!(target: "manager::chat", id=%id, seq, len=final_text.len(),
                text=%log_preview(&final_text, 80), "queued + sent");
        }
    });

    // ---- QA task ----
    // Each backend /qa event carries `final`. stream-json sets final:true (one
    // complete `result` per num) → flush IMMEDIATELY via flush_answer. The
    // legacy PTY path streams partial repaints (no final) → debounce ~settle so
    // we commit only the FINAL text per num, then flush. flush_answer pops the
    // oldest 'sent' row for THIS connection (FIFO), marks it answered, and
    // forwards {a, id, text, timeIn, timeOut, latencyMs} to the client.
    let db_for_qa = db.clone();
    let connection_id_for_qa = connection_id.clone();
    let answer_tx_for_qa = answer_tx.clone();
    let qa = tokio::spawn(async move {
        use std::collections::HashMap;
        let pending_text: Arc<Mutex<HashMap<u32, String>>> = Arc::new(Mutex::new(HashMap::new()));
        let versions: Arc<Mutex<HashMap<u32, u64>>> = Arc::new(Mutex::new(HashMap::new()));
        // PTY-only debounce window (tunable). Unused by the stream-json path,
        // which flushes on final:true without waiting.
        let settle = Duration::from_millis(
            std::env::var("MANAGER_CHAT_SETTLE_MS")
                .ok()
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(3000),
        );

        while let Some(msg) = qa_stream.next().await {
            let text = match msg {
                Ok(Message::Text(t)) => t,
                Ok(Message::Binary(b)) => match String::from_utf8(b) {
                    Ok(s) => s,
                    Err(_) => continue,
                },
                Ok(Message::Close(_)) => break,
                Ok(_) => continue,
                Err(_) => break,
            };
            let v: serde_json::Value = match serde_json::from_str(&text) {
                Ok(x) => x,
                Err(_) => continue,
            };
            if v.get("type").and_then(|x| x.as_str()) == Some("subscribed") {
                continue;
            }
            let num = match v.get("num").and_then(|x| x.as_u64()) {
                Some(n) => n as u32,
                None => continue,
            };
            let raw = v
                .get("answer")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            // Backend signals finality: stream-json sends one complete `result`
            // per num (final:true); the legacy PTY path streams partial repaints
            // (no final) that must be debounced.
            let is_final = v.get("final").and_then(|x| x.as_bool()).unwrap_or(false);
            tracing::info!(
                target: "manager::chat::qa",
                num,
                raw_len = raw.len(),
                is_final,
                raw = %log_preview(&raw, 160),
                question = %log_preview(v.get("question").and_then(|x| x.as_str()).unwrap_or(""), 120),
                "raw answer received from backend /qa stream"
            );

            if is_final {
                // Final on arrival → flush inline (no debounce). Running this in
                // the qa loop serializes pop_sent + forward, so back-to-back
                // answers can't race on the same oldest 'sent' row.
                //
                // Flush even when the text is empty: stream-json emits EXACTLY
                // one final result per submitted question, so skipping an empty
                // one would leave that question's row 'sent' forever and mispair
                // every later answer (FIFO desync). An empty answer is delivered
                // verbatim — claude genuinely produced it.
                flush_answer(
                    &db_for_qa,
                    &connection_id_for_qa,
                    num,
                    raw,
                    &answer_tx_for_qa,
                )
                .await;
                continue;
            }

            // Legacy PTY path: debounce repaints, keep only the latest version.
            pending_text.lock().await.insert(num, raw);
            let new_version = {
                let mut versions = versions.lock().await;
                let ver = versions.entry(num).or_insert(0);
                *ver += 1;
                *ver
            };

            let pending_text_for_flush = pending_text.clone();
            let versions_for_flush = versions.clone();
            let answer_tx = answer_tx_for_qa.clone();
            let db = db_for_qa.clone();
            let connection_id = connection_id_for_qa.clone();
            tokio::spawn(async move {
                tokio::time::sleep(settle).await;
                let still_latest = {
                    let versions = versions_for_flush.lock().await;
                    versions.get(&num).copied() == Some(new_version)
                };
                if !still_latest {
                    return;
                }
                let final_text = match pending_text_for_flush.lock().await.remove(&num) {
                    Some(t) if !t.is_empty() => t,
                    _ => return,
                };
                flush_answer(&db, &connection_id, num, final_text, &answer_tx).await;
            });
        }
    });

    // ---- Writer task: drain answer channel, send to client as JSON frames ----
    let writer = tokio::spawn(async move {
        while let Some(msg) = answer_rx.recv().await {
            tracing::debug!(
                target: "manager::chat",
                kind = %msg.get("type").and_then(|x| x.as_str()).unwrap_or("?"),
                seq = msg.get("seq").and_then(|x| x.as_i64()).unwrap_or(-1),
                id = %msg.get("id").and_then(|x| x.as_str()).unwrap_or(""),
                text = %log_preview(msg.get("text").and_then(|x| x.as_str()).unwrap_or(""), 120),
                "frame → client"
            );
            if ws_sink.send(Message::Text(msg.to_string())).await.is_err() {
                break;
            }
        }
        let _ = ws_sink.close().await;
    });

    // Wait for the READER (the client connection) to end — that's our signal
    // that the client is gone. Then ABORT the qa + writer tasks; otherwise
    // they'd block forever (qa waits on backend's /qa/ stream which we don't
    // close until cmd_close, and writer waits on answer_rx which only closes
    // when qa drops its sender). The previous tokio::join! deadlocked here,
    // which left the client's ws.close() hanging for the websockets default
    // 10s close-handshake timeout.
    let _ = reader.await;
    qa.abort();
    writer.abort();
    let _ = qa.await;
    let _ = writer.await;

    tracing::info!(
        target: "manager::chat",
        sid = %sid,
        connection_id = %connection_id,
        "client disconnected → auto-close"
    );
    // Drop from the live registry (chat_question rows persist independently).
    state.lock().await.clients.remove(&connection_id);
    let _ = cmd_close(&state, &sid).await;
    Ok(())
}

/// Pump frames between a client WebSocket and a backend WebSocket. Used for
/// both /s/<sid> (caller already knows the session exists) and /s/new (caller
/// just spawned one). Returns when either side disconnects.
async fn bridge_to_backend(
    ws: tokio_tungstenite::WebSocketStream<TcpStream>,
    backend_port: u16,
    sid: &str,
    base: &str,
    token: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (mut client_sink, mut client_stream) = ws.split();
    let url = format!("ws://{}:{}{}{}", backend_host(), backend_port, base, sid);
    let req_with_auth = match auth_request(&url, token) {
        Ok(r) => r,
        Err(e) => {
            let _ = client_sink
                .send(Message::Text(format!("auth request build failed: {}", e)))
                .await;
            return Ok(());
        }
    };
    let (backend_ws, _) = match connect_async(req_with_auth).await {
        Ok(p) => p,
        Err(e) => {
            let _ = client_sink
                .send(Message::Text(format!("backend connect failed: {}", e)))
                .await;
            return Ok(());
        }
    };
    let (mut backend_sink, mut backend_stream) = backend_ws.split();

    // forward client -> backend
    let c2b = tokio::spawn(async move {
        while let Some(msg) = client_stream.next().await {
            match msg {
                Ok(Message::Close(_)) => break,
                Ok(m) => {
                    if backend_sink.send(m).await.is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        let _ = backend_sink.close().await;
    });

    // forward backend -> client
    let b2c = tokio::spawn(async move {
        while let Some(msg) = backend_stream.next().await {
            match msg {
                Ok(Message::Close(_)) => break,
                Ok(m) => {
                    if client_sink.send(m).await.is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        let _ = client_sink.close().await;
    });

    let _ = tokio::join!(c2b, b2c);
    Ok(())
}

// ---------- / (root) ----------

/// Scope a flat session-id list to the caller. PURE (design §auth): `chat.admin`
/// sees every session (the ops view, like /control); a shared-token/dev caller
/// (no per-user identity, `caller_uid == None`) sees all; otherwise a plain
/// `chat.user` sees ONLY sessions they own — so `/` can't enumerate other users'
/// session ids/count/timing.
fn scope_sessions_to_caller(
    all: Vec<String>,
    caller_uid: Option<&str>,
    is_admin: bool,
    owners: &std::collections::HashMap<String, String>,
) -> Vec<String> {
    match (caller_uid, is_admin) {
        (_, true) | (None, _) => all,
        (Some(uid), false) => all
            .into_iter()
            .filter(|sid| owners.get(sid).map(|o| o == uid).unwrap_or(false))
            .collect(),
    }
}

async fn handle_root(
    ws: tokio_tungstenite::WebSocketStream<TcpStream>,
    state: SharedState,
    caller_uid: Option<String>,
    is_admin: bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (mut sink, _) = ws.split();
    let (ports, token, owners) = {
        let st = state.lock().await;
        (
            st.instance_ports.clone(),
            st.auth_token.clone(),
            st.session_to_owner.clone(),
        )
    };
    let mut all: Vec<String> = Vec::new();
    for p in &ports {
        let url = format!("ws://{}:{}/", backend_host(), p);
        let req = match auth_request(&url, &token) {
            Ok(r) => r,
            Err(_) => continue,
        };
        if let Ok((mut bws, _)) = connect_async(req).await {
            if let Ok(Some(Ok(Message::Text(t)))) =
                tokio::time::timeout(std::time::Duration::from_secs(2), bws.next()).await
            {
                if let Ok(arr) = serde_json::from_str::<Vec<String>>(&t) {
                    all.extend(arr);
                }
            }
            let _ = bws.send(Message::Close(None)).await;
        }
    }
    let all = scope_sessions_to_caller(all, caller_uid.as_deref(), is_admin, &owners);
    let _ = sink
        .send(Message::Text(
            serde_json::to_string(&all).unwrap_or_default(),
        ))
        .await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compose_own_usage_reply_shape() {
        let total = OwnUsage {
            requests: 42, chars_in: 12345, chars_out: 67890,
            files: 3, file_bytes: 1024000, last_used: Some("2026-06-26T17:30:00.000Z".into()),
        };
        let daily = vec![OwnDaily {
            day: "2026-06-26".into(), requests: 12, chars_in: 3456,
            chars_out: 12345, files: 1, file_bytes: 256000,
        }];
        let v = compose_own_usage_reply("u-9", &total, &daily);
        assert_eq!(v["type"], "usage");
        assert_eq!(v["userId"], "u-9");
        assert_eq!(v["requests"], 42);
        assert_eq!(v["charsIn"], 12345);
        assert_eq!(v["charsOut"], 67890);
        assert_eq!(v["files"], 3);
        assert_eq!(v["fileBytes"], 1024000);
        assert_eq!(v["lastUsed"], "2026-06-26T17:30:00.000Z");
        assert_eq!(v["daily"][0]["day"], "2026-06-26");
        assert_eq!(v["daily"][0]["requests"], 12);
        assert_eq!(v["daily"][0]["fileBytes"], 256000);
    }

    #[test]
    fn scope_sessions_chat_user_sees_only_own() {
        let mut owners = std::collections::HashMap::new();
        owners.insert("s1".to_string(), "alice".to_string());
        owners.insert("s2".to_string(), "bob".to_string());
        let all = vec!["s1".to_string(), "s2".to_string(), "s3".to_string()];
        // alice (chat.user) sees only her own session, never bob's or the
        // owner-less s3.
        assert_eq!(
            scope_sessions_to_caller(all.clone(), Some("alice"), false, &owners),
            vec!["s1".to_string()]
        );
        // chat.admin sees everything (ops view).
        assert_eq!(
            scope_sessions_to_caller(all.clone(), Some("alice"), true, &owners),
            all
        );
        // shared-token / dev mode (no per-user identity) sees everything.
        assert_eq!(scope_sessions_to_caller(all.clone(), None, false, &owners), all);
    }

    #[test]
    fn redact_query_hides_token_keeps_other_params() {
        assert_eq!(redact_query("token=secret.jwt.sig&cwd=svc"), "token=<redacted>&cwd=svc");
        assert_eq!(redact_query("Access_Token=abc"), "Access_Token=<redacted>");
        assert_eq!(redact_query("cwd=svc"), "cwd=svc");
    }

    #[test]
    fn require_addr_errors_when_none() {
        let err = require_addr("MANAGER_BACKEND_HOST", None).unwrap_err();
        assert!(err.contains("MANAGER_BACKEND_HOST"), "names the var: {err}");
    }
    #[test]
    fn require_addr_errors_when_empty() {
        let err = require_addr("MANAGER_BACKEND_HOST", Some(String::new())).unwrap_err();
        assert!(err.contains("MANAGER_BACKEND_HOST"), "names the var: {err}");
    }
    #[test]
    fn require_addr_errors_when_whitespace() {
        let err = require_addr("MANAGER_BACKEND_HOST", Some("  ".to_string())).unwrap_err();
        assert!(err.contains("MANAGER_BACKEND_HOST"), "names the var: {err}");
    }
    #[test]
    fn require_addr_honors_loopback() {
        assert_eq!(require_addr("MANAGER_BACKEND_HOST", Some("127.0.0.1".to_string())).unwrap(),
                   "127.0.0.1");
    }
    #[test]
    fn require_addr_honors_docker_host() {
        assert_eq!(require_addr("MANAGER_BACKEND_HOST",
                                Some("host.docker.internal".to_string())).unwrap(),
                   "host.docker.internal");
    }
    #[test]
    fn require_addr_bind_errors_when_none() {
        let err = require_addr("MANAGER_BIND", None).unwrap_err();
        assert!(err.contains("MANAGER_BIND"), "names the var: {err}");
    }
    #[test]
    fn require_addr_bind_errors_when_empty() {
        let err = require_addr("MANAGER_BIND", Some(String::new())).unwrap_err();
        assert!(err.contains("MANAGER_BIND"), "names the var: {err}");
    }
    #[test]
    fn require_addr_bind_honors_all_interfaces() {
        assert_eq!(require_addr("MANAGER_BIND", Some("0.0.0.0".to_string())).unwrap(),
                   "0.0.0.0");
    }

    #[test]
    fn parse_ports_none_is_none() {
        assert_eq!(parse_backend_ports(None), None);
    }
    #[test]
    fn parse_ports_empty_is_none() {
        assert_eq!(parse_backend_ports(Some(String::new())), None);
        assert_eq!(parse_backend_ports(Some("   ".to_string())), None);
    }
    #[test]
    fn parse_ports_single() {
        assert_eq!(parse_backend_ports(Some("7878".to_string())), Some(vec![7878]));
    }
    #[test]
    fn parse_ports_multi() {
        assert_eq!(parse_backend_ports(Some("7878,7879".to_string())),
                   Some(vec![7878, 7879]));
    }
    #[test]
    fn parse_ports_skips_blank_and_bad_keeps_good() {
        assert_eq!(parse_backend_ports(Some(" 7878 , bad , 7879 ".to_string())),
                   Some(vec![7878, 7879]));
    }
    #[test]
    fn parse_ports_all_bad_is_none() {
        assert_eq!(parse_backend_ports(Some("bad,nope".to_string())), None);
    }

    #[test]
    fn auth_token_uses_env_when_set() {
        let gen = || "GENERATED".to_string();
        assert_eq!(resolve_auth_token(Some("envtok".to_string()), &gen), "envtok");
    }
    #[test]
    fn auth_token_generates_when_none() {
        let gen = || "GENERATED".to_string();
        assert_eq!(resolve_auth_token(None, &gen), "GENERATED");
    }
    #[test]
    fn auth_token_generates_when_empty() {
        let gen = || "GENERATED".to_string();
        assert_eq!(resolve_auth_token(Some(String::new()), &gen), "GENERATED");
    }

    #[test]
    fn open_body_carries_user_id_and_relative_cwd() {
        let b = open_request_body("311867081814147073", Some("crm/acct-42"));
        assert_eq!(b["cmd"], "open");
        assert_eq!(b["userId"], "311867081814147073");
        assert_eq!(b["cwd"], "crm/acct-42");
    }
    #[test]
    fn open_body_omits_cwd_when_none() {
        let b = open_request_body("u1", None);
        assert_eq!(b["userId"], "u1");
        assert!(b.get("cwd").is_none());
    }
}

#[cfg(test)]
mod usage_agg_tests {
    use super::*;

    #[test]
    fn compose_usage_sums_chars_files_bytes_and_totals() {
        let rows = vec![
            UserUsage { user_id: Some("u1".into()), requests: 2, chars_in: 30, chars_out: 12,
                        files: 3, file_bytes: 4096, last_used: Some("t2".into()) },
            UserUsage { user_id: Some("u2".into()), requests: 1, chars_in: 5, chars_out: 1,
                        files: 0, file_bytes: 0, last_used: Some("t1".into()) },
        ];
        let v = compose_usage_reply(&rows);
        assert_eq!(v["ok"], true);
        assert_eq!(v["users"][0]["userId"], "u1");
        assert_eq!(v["users"][0]["charsIn"], 30);
        assert_eq!(v["users"][0]["charsOut"], 12);
        assert_eq!(v["users"][0]["files"], 3);
        assert_eq!(v["users"][0]["fileBytes"], 4096);
        assert_eq!(v["totals"]["requests"], 3);
        assert_eq!(v["totals"]["charsIn"], 35);
        assert_eq!(v["totals"]["charsOut"], 13);
        assert_eq!(v["totals"]["files"], 3);
        assert_eq!(v["totals"]["fileBytes"], 4096);
    }

    #[tokio::test]
    async fn usage_by_user_groups_and_excludes_pending() {
        use sqlx::sqlite::SqlitePoolOptions;
        let pool = SqlitePoolOptions::new().connect("sqlite::memory:").await.unwrap();
        init_schema_sqlite(&pool).await.unwrap();
        let db = ChatDb::Sqlite(pool);
        // two answered rows for u1, one still pending (excluded)
        for (q, status, cin, files, fbytes) in
            [("q1","answered",10,1,100),("q2","confirmed",20,2,200),("q3","pending",99,9,900)] {
            let seq = db.insert_pending("c","s",q,"t","now",None,Some("u1"), cin, files, fbytes).await.unwrap();
            if status != "pending" {
                db.mark_answered(seq, "ans", "now2").await.unwrap(); // sets status='answered', chars_out=3
                db.update_status(seq, status).await.unwrap();
            }
        }
        // Insert a NULL-user answered row — it must NOT appear in results or totals.
        let null_seq = db.insert_pending("c","s","qnull","t","now",None,None, 999, 9, 999).await.unwrap();
        db.mark_answered(null_seq, "ans", "now2").await.unwrap();

        let rows = db.usage_by_user().await.unwrap();
        assert_eq!(rows.len(), 1, "null-user bucket must not appear as a row");
        assert_eq!(rows[0].requests, 2);
        assert_eq!(rows[0].chars_in, 30);
        assert_eq!(rows[0].chars_out, 6);   // "ans" = 3 chars × 2 answered rows
        assert_eq!(rows[0].files, 3);
        assert_eq!(rows[0].file_bytes, 300);
    }
}

#[cfg(test)]
mod usage_daily_tests {
    use super::*;

    #[test]
    fn compose_daily_emits_chars_and_bytes() {
        let rows = vec![
            DailyRow { user_id: Some("u1".into()), day: "2026-06-21".into(),
                       chars_in: 30, chars_out: 5, file_bytes: 2048 },
        ];
        let v = compose_daily_reply(&rows);
        assert_eq!(v["ok"], true);
        assert_eq!(v["days"][0]["userId"], "u1");
        assert_eq!(v["days"][0]["day"], "2026-06-21");
        assert_eq!(v["days"][0]["charsIn"], 30);
        assert_eq!(v["days"][0]["charsOut"], 5);
        assert_eq!(v["days"][0]["fileBytes"], 2048);
    }

    #[tokio::test]
    async fn usage_daily_groups_by_day_honors_cutoff_excludes_null() {
        use sqlx::sqlite::SqlitePoolOptions;
        let pool = SqlitePoolOptions::new().connect("sqlite::memory:").await.unwrap();
        init_schema_sqlite(&pool).await.unwrap();
        let db = ChatDb::Sqlite(pool);
        async fn add(db: &ChatDb, q: &str, ti: &str, user: Option<&str>, cin: i64) {
            let seq = db.insert_pending("c", "s", q, "t", ti, None, user, cin, 0, 0).await.unwrap();
            db.mark_answered(seq, "a", "now2").await.unwrap();
        }
        add(&db, "q1", "2026-06-21T10:00:00.000Z", Some("u1"), 10).await;
        add(&db, "q2", "2026-06-21T12:00:00.000Z", Some("u1"), 20).await;
        add(&db, "q3", "2026-06-20T09:00:00.000Z", Some("u1"), 5).await;
        add(&db, "q4", "2026-01-01T00:00:00.000Z", Some("u1"), 999).await; // stale
        add(&db, "q5", "2026-06-21T10:00:00.000Z", None, 7).await;          // null user
        let rows = db.usage_daily("2026-06-15T00:00:00.000Z").await.unwrap();
        assert_eq!(rows.len(), 2);
        let d21 = rows.iter().find(|r| r.day == "2026-06-21").unwrap();
        assert_eq!(d21.chars_in, 30);
        assert!(rows.iter().all(|r| r.user_id.as_deref() == Some("u1")));
        assert!(rows.iter().all(|r| r.day != "2026-01-01"));
    }
}

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
