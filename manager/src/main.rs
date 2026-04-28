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
//   LLM_CHAT_EXE          — path to llm-chat.exe (default
//                            "../src-tauri/target/debug/llm-chat.exe"
//                            relative to manager.exe location)

mod auth_zitadel;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool};
use sqlx::postgres::PgPool;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{
        client::IntoClientRequest,
        handshake::server::{ErrorResponse, Request, Response},
        http,
        Message,
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
    async fn insert_pending(
        &self,
        connection_id: &str,
        sid: &str,
        q_id: &str,
        text: &str,
        time_in: &str,
        attachment_paths_json: Option<&str>,
    ) -> Result<i64, sqlx::Error> {
        match self {
            ChatDb::Sqlite(p) => {
                let r = sqlx::query(
                    "INSERT INTO chat_question
                     (connection_id, sid, q_id, text, time_in, status, attachment_paths)
                     VALUES (?, ?, ?, ?, ?, 'pending', ?)",
                )
                .bind(connection_id).bind(sid).bind(q_id).bind(text).bind(time_in)
                .bind(attachment_paths_json)
                .execute(p).await?;
                Ok(r.last_insert_rowid())
            }
            ChatDb::Postgres(p) => {
                let row: (i64,) = sqlx::query_as(
                    "INSERT INTO chat_question
                     (connection_id, sid, q_id, text, time_in, status, attachment_paths)
                     VALUES ($1, $2, $3, $4, $5, 'pending', $6) RETURNING seq",
                )
                .bind(connection_id).bind(sid).bind(q_id).bind(text).bind(time_in)
                .bind(attachment_paths_json)
                .fetch_one(p).await?;
                Ok(row.0)
            }
        }
    }

    /// UPDATE a row's status (e.g. 'sent', 'error'). No-op on row not found.
    async fn update_status(&self, seq: i64, status: &str) -> Result<(), sqlx::Error> {
        match self {
            ChatDb::Sqlite(p) => {
                sqlx::query("UPDATE chat_question SET status = ? WHERE seq = ?")
                    .bind(status).bind(seq).execute(p).await?;
            }
            ChatDb::Postgres(p) => {
                sqlx::query("UPDATE chat_question SET status = $1 WHERE seq = $2")
                    .bind(status).bind(seq).execute(p).await?;
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
                .bind(connection_id).fetch_optional(p).await
            }
            ChatDb::Postgres(p) => {
                sqlx::query_as(
                    "SELECT seq, q_id, time_in FROM chat_question
                     WHERE connection_id = $1 AND status = 'sent'
                     ORDER BY seq ASC LIMIT 1",
                )
                .bind(connection_id).fetch_optional(p).await
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
            .bind(time_confirmed).bind(seq).execute(p).await?.rows_affected(),
            ChatDb::Postgres(p) => sqlx::query(
                "UPDATE chat_question SET status = 'confirmed', time_confirmed = $1
                 WHERE seq = $2 AND status = 'answered'",
            )
            .bind(time_confirmed).bind(seq).execute(p).await?.rows_affected(),
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
                     time_out = ? WHERE seq = ?",
                )
                .bind(answer_text).bind(time_out).bind(seq).execute(p).await?;
            }
            ChatDb::Postgres(p) => {
                sqlx::query(
                    "UPDATE chat_question SET answer_text = $1, status = 'answered',
                     time_out = $2 WHERE seq = $3",
                )
                .bind(answer_text).bind(time_out).bind(seq).execute(p).await?;
            }
        }
        Ok(())
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
        .execute(pool).await;
    let _ = sqlx::query("ALTER TABLE chat_question ADD COLUMN attachment_paths TEXT;")
        .execute(pool).await;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_chat_question_status_seq ON chat_question(status, seq);")
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
        .execute(pool).await?;
    sqlx::query("ALTER TABLE chat_question ADD COLUMN IF NOT EXISTS attachment_paths TEXT;")
        .execute(pool).await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_chat_question_status_seq ON chat_question(status, seq);")
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
    type Row = (i64, String, String, String, String, String, String, Option<String>, Option<String>, Option<String>);
    let rows: Vec<Row> = match db {
        ChatDb::Sqlite(pool) => {
            let mut sql = String::from(
                "SELECT seq, connection_id, sid, q_id, text, time_in, status, answer_text, time_out, attachment_paths
                 FROM chat_question WHERE 1=1",
            );
            if connection_id.is_some() { sql.push_str(" AND connection_id = ?"); }
            if sid.is_some() { sql.push_str(" AND sid = ?"); }
            if status.is_some() { sql.push_str(" AND status = ?"); }
            sql.push_str(" ORDER BY seq DESC LIMIT ?");
            let mut q = sqlx::query_as::<_, Row>(&sql);
            if let Some(c) = connection_id { q = q.bind(c); }
            if let Some(s) = sid { q = q.bind(s); }
            if let Some(s) = status { q = q.bind(s); }
            q.bind(limit).fetch_all(pool).await?
        }
        ChatDb::Postgres(pool) => {
            let mut sql = String::from(
                "SELECT seq, connection_id, sid, q_id, text, time_in, status, answer_text, time_out, attachment_paths
                 FROM chat_question WHERE 1=1",
            );
            let mut idx = 1;
            if connection_id.is_some() { sql.push_str(&format!(" AND connection_id = ${}", idx)); idx += 1; }
            if sid.is_some() { sql.push_str(&format!(" AND sid = ${}", idx)); idx += 1; }
            if status.is_some() { sql.push_str(&format!(" AND status = ${}", idx)); idx += 1; }
            sql.push_str(&format!(" ORDER BY seq DESC LIMIT ${}", idx));
            let mut q = sqlx::query_as::<_, Row>(&sql);
            if let Some(c) = connection_id { q = q.bind(c); }
            if let Some(s) = sid { q = q.bind(s); }
            if let Some(s) = status { q = q.bind(s); }
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
            .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/share")));
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

/// Live registry entry for one connected /chat client. Mutated as the client
/// sends questions; removed when the connection ends. Purely in-memory — the
/// chat_question table has the persistent record of what they sent.
#[derive(Clone, serde::Serialize)]
struct ClientInfo {
    #[serde(rename = "connectionId")]
    connection_id: String,
    sid: String,
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
    /// Internal shared secret used for the manager↔backend hop only
    /// (loopback). Inbound client auth is now Zitadel JWT — see `jwks`.
    auth_token: String,
    /// Zitadel JWKS cache for verifying inbound client JWTs. Refreshed in
    /// the background. None means external auth is not configured (the
    /// manager will then refuse all inbound requests).
    jwks: Option<auth_zitadel::JwksCache>,
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
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"));
    let json = matches!(std::env::var("LOG_JSON").ok().as_deref(), Some("1") | Some("true"));
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
        let backend_name = if cfg!(windows) { "llm-chat.exe" } else { "llm-chat" };
        let project_root = exe_dir.join("..").join("..").join("..");
        let release = project_root.join("src-tauri").join("target").join("release").join(backend_name);
        let debug = project_root.join("src-tauri").join("target").join("debug").join(backend_name);
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
    let auth_token = random_token();
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

    let stealth: bool = matches!(
        std::env::var("MANAGER_STEALTH").ok().as_deref(),
        Some("1") | Some("true")
    );

    let mut ports = Vec::new();
    for i in 0..n_instances {
        let port = start_port + i as u16;
        spawn_instance(&exe_path, port, &auth_token, stealth)?;
        ports.push(port);
    }

    tracing::info!(target: "manager", count = ports.len(), "waiting for backends");
    for &p in &ports {
        wait_for_tcp(p, 90).await?;
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
    let jwks = match auth_zitadel::ZitadelConfig::from_env() {
        Ok(cfg) => {
            tracing::info!(target: "manager::auth",
                issuer = %cfg.issuer,
                audience = ?cfg.audience,
                project_id = %cfg.project_id,
                "Zitadel auth enabled");
            let cache = auth_zitadel::JwksCache::new(cfg);
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
        auth_token: auth_token.clone(),
        jwks,
        chat_db,
        clients: HashMap::new(),
    }));

    let listener = TcpListener::bind(("127.0.0.1", manager_port)).await?;
    tracing::info!(
        target: "manager",
        addr = %format!("ws://127.0.0.1:{}", manager_port),
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

fn spawn_instance(exe: &str, port: u16, auth_token: &str, stealth: bool) -> std::io::Result<()> {
    use std::process::Command;
    let path = std::path::Path::new(exe);
    let canon = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());

    // On Linux the Tauri backend needs a display server even in stealth mode
    // (WebKitGTK still initializes a GtkWindow before we hide it). If neither
    // DISPLAY nor WAYLAND_DISPLAY is set, transparently wrap with `xvfb-run -a`
    // so headless boxes Just Work for the manager use case.
    #[cfg(unix)]
    let xvfb_wrap = {
        let no_display = std::env::var_os("DISPLAY").is_none()
            && std::env::var_os("WAYLAND_DISPLAY").is_none();
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
        .env("LLM_CHAT_AUTH_TOKEN", auth_token);
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

async fn wait_for_tcp(port: u16, retries: u32) -> Result<(), std::io::Error> {
    for _ in 0..retries {
        if TcpStream::connect(("127.0.0.1", port)).await.is_ok() {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::TimedOut,
        format!("backend on port {} did not come up", port),
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
    let path_capture = path_holder.clone();
    let cb = move |req: &Request, resp: Response| -> Result<Response, ErrorResponse> {
        *path_capture.lock().unwrap() = req.uri().path().to_string();
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
            let token = match auth_zitadel::extract_bearer(req) {
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

    if req_path == "/control" {
        return handle_control(ws, state).await;
    }
    if req_path == "/chat" {
        return handle_chat(ws, state).await;
    }
    if req_path == "/s/new" {
        return bridge_session_auto(ws, state).await;
    }
    if req_path.starts_with("/s/") {
        let sid = &req_path[3..];
        return bridge_session(ws, state, sid, "/s/").await;
    }
    if req_path.starts_with("/qa/") {
        let sid = &req_path[4..];
        return bridge_session(ws, state, sid, "/qa/").await;
    }
    if req_path == "/" || req_path.is_empty() {
        return handle_root(ws, state).await;
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
                    let c = st
                        .session_to_port
                        .values()
                        .filter(|x| **x == *p)
                        .count();
                    counts.insert(p.to_string(), serde_json::json!(c));
                }
                serde_json::json!({"ok":true,"ports":ports,"sessionsPerPort":counts})
            }
            "open" => match cmd_open(&state).await {
                Ok((sid, port)) => serde_json::json!({"ok":true,"sessionId":sid,"backendPort":port}),
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
                            per_backend.insert(p.to_string(), serde_json::json!({"error":e.to_string()}));
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
                        if let Ok(v) = call_backend(*p, serde_json::json!({"cmd":"history"})).await {
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
                let connection_id = req.get("connectionId").and_then(|v| v.as_str()).map(|s| s.to_string());
                let sid_filter = req.get("sid").and_then(|v| v.as_str()).map(|s| s.to_string());
                let status_filter = req.get("status").and_then(|v| v.as_str()).map(|s| s.to_string());
                let limit = req.get("limit").and_then(|v| v.as_i64()).unwrap_or(100).clamp(1, 1000);
                let db = state.lock().await.chat_db.clone();
                match query_chat_queue(&db, connection_id.as_deref(), sid_filter.as_deref(), status_filter.as_deref(), limit).await {
                    Ok(rows) => serde_json::json!({"ok":true,"count":rows.len(),"rows":rows}),
                    Err(e) => serde_json::json!({"ok":false,"error":format!("queue query: {}", e)}),
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
                        let r = call_backend(*p, backend_req.clone()).await
                            .unwrap_or_else(|e| serde_json::json!({"ok":false,"error":e.to_string()}));
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
                let sid = req.get("sessionId").and_then(|v| v.as_str()).map(|s| s.to_string());
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
    let mut counts: HashMap<u16, usize> = st
        .instance_ports
        .iter()
        .map(|p| (*p, 0usize))
        .collect();
    for &p in st.session_to_port.values() {
        *counts.entry(p).or_insert(0) += 1;
    }
    counts.into_iter().min_by_key(|(_, c)| *c).map(|(p, _)| p)
}

async fn cmd_open(
    state: &SharedState,
) -> Result<(String, u16), Box<dyn std::error::Error + Send + Sync>> {
    let port = pick_least_loaded_port(state)
        .await
        .ok_or("no backends configured")?;
    let resp = call_backend(port, serde_json::json!({"cmd":"open"})).await?;
    let sid = resp
        .get("sessionId")
        .and_then(|v| v.as_str())
        .ok_or("backend did not return sessionId")?
        .to_string();
    state.lock().await.session_to_port.insert(sid.clone(), port);
    Ok((sid, port))
}

async fn cmd_close(
    state: &SharedState,
    sid: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let port = lookup_port(state, sid).await.ok_or("unknown sessionId")?;
    let _ = call_backend(
        port,
        serde_json::json!({"cmd":"close","sessionId":sid}),
    )
    .await?;
    state.lock().await.session_to_port.remove(sid);
    Ok(())
}

/// Build a tungstenite request to a backend with the auth token attached.
fn auth_request(
    url: &str,
    token: &str,
) -> Result<tokio_tungstenite::tungstenite::handshake::client::Request, Box<dyn std::error::Error + Send + Sync>>
{
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
    let url = format!("ws://127.0.0.1:{}/control", port);
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
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (sid, port) = match cmd_open(&state).await {
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
fn now_iso() -> String {
    chrono::Utc::now()
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
async fn handle_chat(
    ws: tokio_tungstenite::WebSocketStream<TcpStream>,
    state: SharedState,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use std::time::Duration;
    use tokio::sync::mpsc;

    // 1. Spawn a fresh session in the least-loaded backend.
    let (sid, port) = match cmd_open(&state).await {
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
    let s_url = format!("ws://127.0.0.1:{}/s/{}", port, sid);
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

    let qa_url = format!("ws://127.0.0.1:{}/qa/{}", port, sid);
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

    // Cold-start grace period: claude CLI takes ~5–8 s to reach its prompt
    // after PTY spawn, AND the JS parser needs to register an onData hook on
    // the new xterm tab before PTY traffic starts. Without this delay, the
    // first q gets typed while claude is still showing its welcome screen
    // (no `>` / `●` markers ever emit on /qa). 0 disables the wait for tests
    // that have already warmed up the session another way.
    let warmup = std::env::var("MANAGER_CHAT_WARMUP_SECS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(8);
    if warmup > 0 {
        tracing::debug!(target: "manager::chat", sid = %sid, warmup_secs = warmup,
            "warming up newly-spawned session before processing first q");
        tokio::time::sleep(Duration::from_secs(warmup)).await;
    }

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
                        })).await;
                    continue;
                }
                match db_for_in.mark_confirmed(seq, &time_in).await {
                    Ok(true) => tracing::debug!(target:"manager::chat", seq, "confirmed"),
                    Ok(false) => tracing::warn!(target:"manager::chat", seq, "confirm for non-answered or unknown seq"),
                    Err(e) => tracing::error!(target:"manager::chat::db", error=%e, seq, "mark_confirmed failed"),
                }
                continue;
            }
            if msg_type != "q" {
                let _ = answer_tx_for_in
                    .send(serde_json::json!({
                        "type":"err",
                        "text":format!("unknown type \"{}\"; expected \"q\" or \"confirm\"", msg_type),
                        "timeIn": time_in,
                        "timeOut": now_iso(),
                    }))
                    .await;
                continue;
            }
            let id = v.get("id").and_then(|x| x.as_str()).unwrap_or("").to_string();
            let q_text = v.get("text").and_then(|x| x.as_str()).unwrap_or("").to_string();
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
                    let name = att.get("name").and_then(|x| x.as_str()).unwrap_or("attachment").to_string();
                    let mime = att.get("mime").and_then(|x| x.as_str()).unwrap_or("").to_string();
                    let data = att.get("data").and_then(|x| x.as_str()).unwrap_or("").to_string();
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
                                    hard_err = Some("backend save_attachment returned no path".into());
                                    break;
                                }
                            } else {
                                let err = reply.get("error").and_then(|v| v.as_str())
                                    .unwrap_or("unknown error").to_string();
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
                        })).await;
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
                let prefix = saved_paths.iter()
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

            // INSERT into the FIFO with status='pending'. seq is the FIFO key
            // AND the server-assigned receipt id we return in `ack` and `a`.
            let seq = match db_for_in
                .insert_pending(
                    &connection_id_for_in, &sid_for_in, &id, &final_text, &time_in,
                    attachment_paths_json.as_deref(),
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
                        })).await;
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
            if s_sink.send(Message::Text(final_text.clone())).await.is_err() {
                let _ = db_for_in.update_status(seq, "error").await;
                let _ = answer_tx_for_in
                    .send(serde_json::json!({
                        "type":"err","id":id,"seq":seq,"text":"backend PTY closed",
                        "timeIn": time_in,"timeOut": now_iso(),
                    })).await;
                break;
            }
            tokio::time::sleep(Duration::from_millis(150)).await;
            if s_sink.send(Message::Text("\r".to_string())).await.is_err() {
                let _ = db_for_in.update_status(seq, "error").await;
                let _ = answer_tx_for_in
                    .send(serde_json::json!({
                        "type":"err","id":id,"seq":seq,"text":"backend PTY closed (enter)",
                        "timeIn": time_in,"timeOut": now_iso(),
                    })).await;
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
            tracing::debug!(target: "manager::chat", id=%id, seq, "queued + sent");
        }
    });

    // ---- QA task ----
    // For each parser event, debounce ~3s so we get the FINAL answer text per
    // num. Once settled, pop the oldest 'sent' row for THIS connection (FIFO
    // order via SELECT MIN(seq)), UPDATE it with answer + status='answered'
    // + time_out, and forward the {a, id, text, timeIn, timeOut} JSON to the
    // client.
    let db_for_qa = db.clone();
    let connection_id_for_qa = connection_id.clone();
    let answer_tx_for_qa = answer_tx.clone();
    let qa = tokio::spawn(async move {
        use std::collections::HashMap;
        let pending_text: Arc<Mutex<HashMap<u32, String>>> = Arc::new(Mutex::new(HashMap::new()));
        let versions: Arc<Mutex<HashMap<u32, u64>>> = Arc::new(Mutex::new(HashMap::new()));
        let settle = Duration::from_millis(3000);

        while let Some(msg) = qa_stream.next().await {
            let text = match msg {
                Ok(Message::Text(t)) => t,
                Ok(Message::Binary(b)) => match String::from_utf8(b) {
                    Ok(s) => s, Err(_) => continue,
                },
                Ok(Message::Close(_)) => break,
                Ok(_) => continue,
                Err(_) => break,
            };
            let v: serde_json::Value = match serde_json::from_str(&text) {
                Ok(x) => x, Err(_) => continue,
            };
            if v.get("type").and_then(|x| x.as_str()) == Some("subscribed") {
                continue;
            }
            let num = match v.get("num").and_then(|x| x.as_u64()) {
                Some(n) => n as u32, None => continue,
            };
            let raw = v.get("answer").and_then(|x| x.as_str()).unwrap_or("").to_string();
            // The backend (llm-chat) cleans TUI noise before broadcasting on
            // /qa/<sid>, so this stream is already normalized. Pass through.
            pending_text.lock().await.insert(num, raw);
            let new_version = {
                let mut versions = versions.lock().await;
                let v = versions.entry(num).or_insert(0);
                *v += 1; *v
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
                if !still_latest { return; }
                let final_text = match pending_text_for_flush.lock().await.remove(&num) {
                    Some(t) if !t.is_empty() => t,
                    _ => return,
                };
                // Pop the oldest 'sent' row for this connection (FIFO).
                let row = db.pop_sent(&connection_id).await.unwrap_or(None);
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
                let _ = db.mark_answered(seq, &final_text, &time_out).await;
                let out = serde_json::json!({
                    "type": "a",
                    "id": q_id,
                    "seq": seq,
                    "text": final_text,
                    "timeIn": time_in,
                    "timeOut": time_out,
                });
                let _ = answer_tx.send(out).await;
            });
        }
    });

    // ---- Writer task: drain answer channel, send to client as JSON frames ----
    let writer = tokio::spawn(async move {
        while let Some(msg) = answer_rx.recv().await {
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
    let url = format!("ws://127.0.0.1:{}{}{}", backend_port, base, sid);
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

async fn handle_root(
    ws: tokio_tungstenite::WebSocketStream<TcpStream>,
    state: SharedState,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (mut sink, _) = ws.split();
    let (ports, token) = {
        let st = state.lock().await;
        (st.instance_ports.clone(), st.auth_token.clone())
    };
    let mut all: Vec<String> = Vec::new();
    for p in &ports {
        let url = format!("ws://127.0.0.1:{}/", p);
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
    let _ = sink
        .send(Message::Text(serde_json::to_string(&all).unwrap_or_default()))
        .await;
    Ok(())
}
