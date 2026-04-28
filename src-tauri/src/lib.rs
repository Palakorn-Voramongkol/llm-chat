use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use tokio::sync::broadcast;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool};

// ---------- SQLite-backed PTY input FIFO ----------
//
// One file per backend instance:
//   $XDG_DATA_HOME/com.llm-chat.app/backend-{port}.sqlite
// (override via $LLM_CHAT_DB_PATH). Single table:
//
//   pty_input(seq, sid, payload, time_in, status, time_written)
//
// Every write to a session's PTY (via pty_write command OR the /s/<sid> WS
// forwarder) is recorded here in FIFO order. Status: pending → written/error.
// Source of truth — survives a backend restart for diagnostic/audit.
static PTY_INPUT_DB: OnceLock<SqlitePool> = OnceLock::new();

fn pty_db_path() -> std::path::PathBuf {
    if let Ok(p) = std::env::var("LLM_CHAT_DB_PATH") {
        return std::path::PathBuf::from(p);
    }
    let port: u16 = std::env::var("LLM_CHAT_WS_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(7878);
    auth_token_file_path().with_file_name(format!("backend-{}.sqlite", port))
}

async fn open_pty_db(path: &std::path::Path) -> Result<SqlitePool, sqlx::Error> {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let opts = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .synchronous(sqlx::sqlite::SqliteSynchronous::Normal);
    let pool = SqlitePool::connect_with(opts).await?;
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS pty_input (
            seq INTEGER PRIMARY KEY AUTOINCREMENT,
            sid TEXT NOT NULL,
            payload BLOB NOT NULL,
            time_in TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'pending',
            time_written TEXT
        );
        "#,
    )
    .execute(&pool)
    .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_pty_input_sid_seq ON pty_input(sid, seq);")
        .execute(&pool)
        .await?;
    Ok(pool)
}

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// Record one PTY write into the SQLite FIFO. Returns the row's `seq` so the
/// caller can mark it 'written' (or 'error') after attempting the actual PTY
/// write. Best-effort — DB unavailable is logged at WARN, never blocks.
async fn pty_input_record(sid: &str, bytes: &[u8]) -> Option<i64> {
    let pool = PTY_INPUT_DB.get()?;
    let res = sqlx::query(
        "INSERT INTO pty_input (sid, payload, time_in, status) VALUES (?, ?, ?, 'pending')",
    )
    .bind(sid)
    .bind(bytes)
    .bind(now_iso())
    .execute(pool)
    .await;
    match res {
        Ok(r) => Some(r.last_insert_rowid()),
        Err(e) => {
            tracing::warn!(target: "backend::db", error=%e, "INSERT pty_input failed");
            None
        }
    }
}

/// Read recent rows from the PTY input FIFO. Returns the most recent rows
/// first (descending seq), filtered by optional sid + status, capped by limit.
/// Empty Vec if the DB isn't initialized yet (just started, init failed, etc.).
async fn query_pty_input(
    sid: Option<&str>,
    status: Option<&str>,
    limit: i64,
) -> Result<Vec<serde_json::Value>, sqlx::Error> {
    let Some(pool) = PTY_INPUT_DB.get() else {
        return Ok(Vec::new());
    };
    // Build the WHERE clause dynamically. sqlx's QueryBuilder would be
    // cleaner; raw query with bind is fine for two optional filters.
    let mut sql = String::from(
        "SELECT seq, sid, payload, time_in, status, time_written FROM pty_input WHERE 1=1",
    );
    if sid.is_some() {
        sql.push_str(" AND sid = ?");
    }
    if status.is_some() {
        sql.push_str(" AND status = ?");
    }
    sql.push_str(" ORDER BY seq DESC LIMIT ?");
    let mut q = sqlx::query_as::<_, (i64, String, Vec<u8>, String, String, Option<String>)>(&sql);
    if let Some(s) = sid { q = q.bind(s); }
    if let Some(s) = status { q = q.bind(s); }
    q = q.bind(limit);
    let rows = q.fetch_all(pool).await?;
    Ok(rows
        .into_iter()
        .map(|(seq, sid, payload, time_in, status, time_written)| {
            // payload is bytes; expose as utf-8 (lossy) so JSON can carry it
            // and humans can read it. A `payloadLen` field gives the byte size
            // for binary-safety inspection.
            let payload_str = String::from_utf8_lossy(&payload).into_owned();
            serde_json::json!({
                "seq": seq,
                "sid": sid,
                "payload": payload_str,
                "payloadLen": payload.len(),
                "timeIn": time_in,
                "status": status,
                "timeWritten": time_written,
            })
        })
        .collect())
}

// ---------- Attachments ----------
//
// The manager forwards attached files (PDFs, images) to the backend via a
// /control "save_attachment" command. We decode the base64 payload and write
// it into a per-session directory:
//
//   $XDG_DATA_HOME/com.llm-chat.app/attachments/<sid>/<uuid>-<sanitized-name>
//
// On session close, the entire <sid>/ directory is removed.
//
// Only image/* and application/pdf MIME types are accepted — claude can
// process those via vision / PDF reader. Anything else is rejected so we
// don't silently store untrusted blobs of unknown type.
const ATTACHMENT_ALLOWED_MIME: &[&str] = &[
    "image/png",
    "image/jpeg",
    "image/jpg",
    "image/gif",
    "image/webp",
    "application/pdf",
];

fn attachment_dir(sid: &str) -> std::path::PathBuf {
    auth_token_file_path()
        .with_file_name("attachments")
        .join(sanitize_path_component(sid))
}

fn sanitize_path_component(s: &str) -> String {
    // Drop anything that could escape the directory or look weird in a path.
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '_' })
        .collect()
}

fn save_attachment(sid: &str, name: &str, mime: &str, b64: &str) -> Result<std::path::PathBuf, String> {
    if !ATTACHMENT_ALLOWED_MIME.contains(&mime) {
        return Err(format!("MIME type not allowed: {}", mime));
    }
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|e| format!("base64 decode: {}", e))?;
    let dir = attachment_dir(sid);
    std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir {}: {}", dir.display(), e))?;
    let safe_name = sanitize_path_component(name);
    // Prefix with a short uuid to avoid collisions when the same name is
    // attached more than once in one session.
    let id = uuid::Uuid::new_v4().simple().to_string();
    let id_short = &id[..8];
    let path = dir.join(format!("{}-{}", id_short, safe_name));
    std::fs::write(&path, &bytes).map_err(|e| format!("write {}: {}", path.display(), e))?;
    tracing::info!(
        target: "backend::attachment",
        sid,
        name = name,
        mime,
        bytes = bytes.len(),
        path = %path.display(),
        "attachment saved"
    );
    Ok(path)
}

fn cleanup_attachments(sid: &str) {
    let dir = attachment_dir(sid);
    if dir.exists() {
        if let Err(e) = std::fs::remove_dir_all(&dir) {
            tracing::warn!(target: "backend::attachment", sid, error = %e, "cleanup failed");
        } else {
            tracing::debug!(target: "backend::attachment", sid, path = %dir.display(), "cleaned up");
        }
    }
}

// We need uuid in the backend now. Add it to Cargo.toml.

/// Mark a previously-recorded write as completed (or errored).
async fn pty_input_mark(seq: i64, ok: bool) {
    let Some(pool) = PTY_INPUT_DB.get() else { return };
    let status = if ok { "written" } else { "error" };
    let _ = sqlx::query(
        "UPDATE pty_input SET status = ?, time_written = ? WHERE seq = ?",
    )
    .bind(status)
    .bind(now_iso())
    .bind(seq)
    .execute(pool)
    .await;
}

// ========== ConPTY ==========
#[cfg(windows)]
pub(crate) mod pty {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use windows::Win32::Foundation::*;
    use windows::Win32::Security::SECURITY_ATTRIBUTES;
    use windows::Win32::Storage::FileSystem::{ReadFile, WriteFile};
    use windows::Win32::System::Console::*;
    use windows::Win32::System::Pipes::*;
    use windows::Win32::System::Threading::*;
    use windows::core::PWSTR;

    #[derive(Clone, Copy)]
    pub struct SendHandle(pub HANDLE);
    unsafe impl Send for SendHandle {}
    unsafe impl Sync for SendHandle {}

    #[derive(Clone, Copy)]
    pub struct SendHPCON(pub HPCON);
    unsafe impl Send for SendHPCON {}
    unsafe impl Sync for SendHPCON {}

    pub struct PtySession {
        pub hpc: SendHPCON,
        pub stdin_write: SendHandle,
        pub stdout_read: SendHandle,
        pub stdin_read: SendHandle,
        pub stdout_write: SendHandle,
        pub process_handle: SendHandle,
        pub reader_active: Arc<AtomicBool>,
        closed: bool,
    }

    impl PtySession {
        pub fn create(command: &str, cols: i16, rows: i16) -> Result<Self, String> {
            unsafe {
                let mut stdin_read = HANDLE::default();
                let mut stdin_write = HANDLE::default();
                let mut stdout_read = HANDLE::default();
                let mut stdout_write = HANDLE::default();

                // Pipes must be non-inheritable for ConPTY
                let sa = SECURITY_ATTRIBUTES {
                    nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                    lpSecurityDescriptor: std::ptr::null_mut(),
                    bInheritHandle: FALSE,
                };

                CreatePipe(&mut stdin_read, &mut stdin_write, Some(&sa), 0)
                    .map_err(|e| format!("CreatePipe stdin: {}", e))?;
                CreatePipe(&mut stdout_read, &mut stdout_write, Some(&sa), 0)
                    .map_err(|e| format!("CreatePipe stdout: {}", e))?;

                let size = COORD { X: cols, Y: rows };
                let hpc = CreatePseudoConsole(size, stdin_read, stdout_write, 0)
                    .map_err(|e| format!("CreatePseudoConsole: {}", e))?;

                let mut attr_list_size: usize = 0;
                let _ = InitializeProcThreadAttributeList(
                    LPPROC_THREAD_ATTRIBUTE_LIST(std::ptr::null_mut()),
                    1,
                    0,
                    &mut attr_list_size,
                );

                let attr_list_buf = vec![0u8; attr_list_size];
                let attr_list =
                    LPPROC_THREAD_ATTRIBUTE_LIST(attr_list_buf.as_ptr() as *mut _);

                InitializeProcThreadAttributeList(attr_list, 1, 0, &mut attr_list_size)
                    .map_err(|e| format!("InitializeProcThreadAttributeList: {}", e))?;

                UpdateProcThreadAttribute(
                    attr_list,
                    0,
                    0x00020016, // PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE
                    Some(hpc.0 as *const std::ffi::c_void),
                    std::mem::size_of::<HPCON>(),
                    None,
                    None,
                )
                .map_err(|e| format!("UpdateProcThreadAttribute: {}", e))?;

                let mut si = STARTUPINFOEXW::default();
                si.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
                si.lpAttributeList = attr_list;
                // Force INVALID stdio so the child doesn't inherit the GUI parent's
                // broken handles — proven fix from WezTerm for ConPTY in GUI processes.
                si.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
                si.StartupInfo.hStdInput = INVALID_HANDLE_VALUE;
                si.StartupInfo.hStdOutput = INVALID_HANDLE_VALUE;
                si.StartupInfo.hStdError = INVALID_HANDLE_VALUE;

                let mut pi = PROCESS_INFORMATION::default();

                // chcp 65001 sets the child console to UTF-8
                let cmd_line = format!("cmd.exe /k \"chcp 65001 >nul & {}\"", command);
                let mut cmd_wide: Vec<u16> =
                    cmd_line.encode_utf16().chain(std::iter::once(0)).collect();

                CreateProcessW(
                    None,
                    PWSTR(cmd_wide.as_mut_ptr()),
                    None,
                    None,
                    false,
                    EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT,
                    None,
                    None,
                    &si.StartupInfo,
                    &mut pi,
                )
                .map_err(|e| format!("CreateProcessW: {}", e))?;

                let _ = CloseHandle(pi.hThread);
                DeleteProcThreadAttributeList(attr_list);

                Ok(PtySession {
                    hpc: SendHPCON(hpc),
                    stdin_write: SendHandle(stdin_write),
                    stdout_read: SendHandle(stdout_read),
                    stdin_read: SendHandle(stdin_read),
                    stdout_write: SendHandle(stdout_write),
                    process_handle: SendHandle(pi.hProcess),
                    reader_active: Arc::new(AtomicBool::new(true)),
                    closed: false,
                })
            }
        }

        pub fn write(&self, data: &[u8]) -> Result<(), String> {
            unsafe {
                let mut written: u32 = 0;
                WriteFile(self.stdin_write.0, Some(data), Some(&mut written), None)
                    .map_err(|e| format!("WriteFile: {}", e))?;
            }
            Ok(())
        }

        pub fn resize(&self, cols: i16, rows: i16) -> Result<(), String> {
            unsafe {
                let size = COORD { X: cols, Y: rows };
                ResizePseudoConsole(self.hpc.0, size)
                    .map_err(|e| format!("ResizePseudoConsole: {}", e))?;
            }
            Ok(())
        }

        pub fn close(&mut self) {
            if self.closed {
                return;
            }
            self.closed = true;
            self.reader_active.store(false, Ordering::Relaxed);
            unsafe {
                ClosePseudoConsole(self.hpc.0);
                let _ = CloseHandle(self.stdin_write.0);
                let _ = CloseHandle(self.stdin_read.0);
                let _ = CloseHandle(self.stdout_write.0);
                let _ = CloseHandle(self.process_handle.0);
            }
        }

        pub fn spawn_reader(
            &self,
            app_handle: tauri::AppHandle,
            session_id: String,
            ws_tx: tokio::sync::broadcast::Sender<Vec<u8>>,
        ) {
            let handle_val = self.stdout_read.0 .0 as usize;
            let active = self.reader_active.clone();

            std::thread::spawn(move || {
                use tauri::Emitter;
                let raw_handle = HANDLE(handle_val as *mut std::ffi::c_void);
                let mut buf = [0u8; 4096];
                while active.load(Ordering::Relaxed) {
                    let mut bytes_read: u32 = 0;
                    let ok = unsafe {
                        ReadFile(raw_handle, Some(&mut buf), Some(&mut bytes_read), None)
                    };
                    if ok.is_err() || bytes_read == 0 {
                        break;
                    }
                    let data = buf[..bytes_read as usize].to_vec();
                    // Fan out to any connected WebSocket clients (best-effort).
                    let _ = ws_tx.send(data.clone());
                    use base64::Engine;
                    let encoded = base64::engine::general_purpose::STANDARD.encode(&data);
                    let _ = app_handle.emit(
                        "pty-data",
                        serde_json::json!({"sessionId": session_id, "data": encoded}),
                    );
                }
                active.store(false, Ordering::Relaxed);
                let _ = app_handle.emit("pty-closed", &session_id);
                unsafe {
                    let _ = CloseHandle(HANDLE(handle_val as *mut std::ffi::c_void));
                }
            });
        }
    }

    impl Drop for PtySession {
        fn drop(&mut self) {
            self.close();
        }
    }
}

// ========== Unix PTY ==========
// Mirror of `mod pty` (Windows ConPTY) on Unix using portable-pty's openpty/
// forkpty wrapper. Same public API — `PtySession::{create, write, resize,
// close, spawn_reader}` — so call sites in `do_spawn_session`, `pty_write`,
// `pty_resize`, `close_session` are platform-agnostic.
#[cfg(unix)]
pub(crate) mod pty {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use portable_pty::{native_pty_system, CommandBuilder, PtySize};

    pub struct PtySession {
        master: Arc<Mutex<Box<dyn portable_pty::MasterPty + Send>>>,
        writer: Arc<Mutex<Box<dyn std::io::Write + Send>>>,
        child: Arc<Mutex<Box<dyn portable_pty::Child + Send + Sync>>>,
        pub reader_active: Arc<AtomicBool>,
        closed: bool,
    }

    impl PtySession {
        pub fn create(command: &str, cols: i16, rows: i16) -> Result<Self, String> {
            let size = PtySize {
                rows: rows.max(1) as u16,
                cols: cols.max(1) as u16,
                pixel_width: 0,
                pixel_height: 0,
            };
            let pair = native_pty_system()
                .openpty(size)
                .map_err(|e| format!("openpty: {}", e))?;

            // Run via /bin/sh -c so the caller can pass a free-form command
            // line (matches the cmd.exe /k pattern used on Windows).
            let mut cb = CommandBuilder::new("/bin/sh");
            cb.arg("-c");
            cb.arg(command);
            // CommandBuilder starts with a clean env on unix; forward the
            // vars Claude / Ink / xterm need to render its TUI.
            for var in [
                "HOME", "USER", "LOGNAME", "PATH", "SHELL",
                "LANG", "LC_ALL", "LC_CTYPE", "TERM",
                "FORCE_COLOR", "COLORFGBG", "COLORTERM",
                "CLAUDE_CODE_GIT_BASH_PATH", "DISPLAY", "WAYLAND_DISPLAY",
                "XDG_RUNTIME_DIR",
            ] {
                if let Ok(v) = std::env::var(var) {
                    cb.env(var, v);
                }
            }
            // CWD = parent's CWD (matches Windows ConPTY behavior).
            if let Ok(cwd) = std::env::current_dir() {
                cb.cwd(cwd);
            }

            let child = pair
                .slave
                .spawn_command(cb)
                .map_err(|e| format!("spawn_command: {}", e))?;
            // Drop the slave so the master sees EOF when the child exits.
            drop(pair.slave);

            let writer = pair
                .master
                .take_writer()
                .map_err(|e| format!("take_writer: {}", e))?;

            Ok(PtySession {
                master: Arc::new(Mutex::new(pair.master)),
                writer: Arc::new(Mutex::new(writer)),
                child: Arc::new(Mutex::new(child)),
                reader_active: Arc::new(AtomicBool::new(true)),
                closed: false,
            })
        }

        pub fn write(&self, data: &[u8]) -> Result<(), String> {
            use std::io::Write as _;
            let mut w = self.writer.lock().unwrap();
            w.write_all(data).map_err(|e| format!("write: {}", e))?;
            w.flush().map_err(|e| format!("flush: {}", e))?;
            Ok(())
        }

        pub fn resize(&self, cols: i16, rows: i16) -> Result<(), String> {
            let size = PtySize {
                rows: rows.max(1) as u16,
                cols: cols.max(1) as u16,
                pixel_width: 0,
                pixel_height: 0,
            };
            self.master
                .lock()
                .unwrap()
                .resize(size)
                .map_err(|e| format!("resize: {}", e))
        }

        pub fn close(&mut self) {
            if self.closed {
                return;
            }
            self.closed = true;
            self.reader_active.store(false, Ordering::Relaxed);
            let _ = self.child.lock().unwrap().kill();
        }

        pub fn spawn_reader(
            &self,
            app_handle: tauri::AppHandle,
            session_id: String,
            ws_tx: tokio::sync::broadcast::Sender<Vec<u8>>,
        ) {
            let mut reader = match self.master.lock().unwrap().try_clone_reader() {
                Ok(r) => r,
                Err(e) => {
                    tracing::error!(target: "backend::pty", error = %e, "try_clone_reader");
                    return;
                }
            };
            let active = self.reader_active.clone();
            std::thread::spawn(move || {
                use std::io::Read as _;
                use tauri::Emitter;
                let mut buf = [0u8; 4096];
                while active.load(Ordering::Relaxed) {
                    match reader.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            let data = buf[..n].to_vec();
                            let _ = ws_tx.send(data.clone());
                            use base64::Engine;
                            let encoded =
                                base64::engine::general_purpose::STANDARD.encode(&data);
                            let _ = app_handle.emit(
                                "pty-data",
                                serde_json::json!({
                                    "sessionId": session_id,
                                    "data": encoded
                                }),
                            );
                        }
                        Err(_) => break,
                    }
                }
                active.store(false, Ordering::Relaxed);
                let _ = app_handle.emit("pty-closed", &session_id);
            });
        }
    }

    impl Drop for PtySession {
        fn drop(&mut self) {
            self.close();
        }
    }
}

#[derive(Clone, serde::Serialize)]
struct QaItem {
    num: u32,
    question: String,
    answer: String,
}

// ========== App State ==========
struct AppState {
    #[cfg(any(unix, windows))]
    pty_sessions: Mutex<HashMap<String, pty::PtySession>>,
    pty_broadcasts: Mutex<HashMap<String, broadcast::Sender<Vec<u8>>>>,
    qa_broadcasts: Mutex<HashMap<String, broadcast::Sender<String>>>,
    qa_history: Mutex<HashMap<String, Vec<QaItem>>>,
    session_order: Mutex<Vec<String>>,
    active_session_id: Mutex<Option<String>>,
    terminal_ready: Arc<AtomicBool>,
}

// ========== Git Bash Discovery ==========
// Claude Code v2.1+ on Windows requires bash.exe and reads its location from
// CLAUDE_CODE_GIT_BASH_PATH. We probe standard Git for Windows install dirs
// across drives so the user doesn't have to set anything by hand.
fn find_git_bash_path() -> Option<String> {
    let drives = ["C:", "D:", "E:", "F:"];
    let suffixes = [
        "\\Program Files\\Git\\bin\\bash.exe",
        "\\Program Files (x86)\\Git\\bin\\bash.exe",
    ];
    for drive in &drives {
        for sfx in &suffixes {
            let p = format!("{}{}", drive, sfx);
            if std::path::Path::new(&p).exists() {
                return Some(p);
            }
        }
    }
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        let p = std::path::Path::new(&local)
            .join("Programs")
            .join("Git")
            .join("bin")
            .join("bash.exe");
        if p.exists() {
            return Some(p.to_string_lossy().into_owned());
        }
    }
    // Derive from git.exe on PATH: usually <root>\cmd\git.exe → <root>\bin\bash.exe
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in path_var.split(';') {
            let dir = dir.trim();
            if dir.is_empty() {
                continue;
            }
            let git_exe = std::path::Path::new(dir).join("git.exe");
            if git_exe.exists() {
                if let Some(parent) = git_exe.parent().and_then(|p| p.parent()) {
                    let bash = parent.join("bin").join("bash.exe");
                    if bash.exists() {
                        return Some(bash.to_string_lossy().into_owned());
                    }
                }
            }
        }
    }
    None
}

// ========== Window Capture ==========
//
// Captures the main webview window as a PNG so the manager (or any /control
// client) can fetch the visual state of an instance — used both as a feature
// and as a diagnostic for rendering bugs that only manifest in spawned mode.
#[cfg(windows)]
fn capture_main_window_to_png(
    app_handle: &tauri::AppHandle,
    out_path: &std::path::Path,
) -> Result<(), String> {
    use tauri::Manager as _;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Gdi::{
        BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC,
        GetDIBits, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
        DIB_RGB_COLORS, HGDIOBJ, SRCCOPY,
    };
    use windows::Win32::Storage::Xps::{PrintWindow, PRINT_WINDOW_FLAGS};
    use windows::Win32::UI::WindowsAndMessaging::GetClientRect;

    let win = app_handle
        .get_webview_window("main")
        .ok_or_else(|| "no main window".to_string())?;
    // tauri exposes HWND from a different `windows` crate version than ours;
    // both are `struct HWND(*mut c_void)`, so convert via the raw pointer.
    let raw_hwnd = win.hwnd().map_err(|e| format!("hwnd: {e}"))?;
    let hwnd: HWND = HWND(raw_hwnd.0 as *mut _);

    unsafe {
        let mut rect = std::mem::zeroed();
        GetClientRect(hwnd, &mut rect).map_err(|e| format!("GetClientRect: {e}"))?;
        let width = (rect.right - rect.left).max(1);
        let height = (rect.bottom - rect.top).max(1);

        let hdc_screen = GetDC(hwnd);
        if hdc_screen.0.is_null() {
            return Err("GetDC returned null".into());
        }
        let hdc_mem = CreateCompatibleDC(hdc_screen);
        let hbm = CreateCompatibleBitmap(hdc_screen, width, height);
        let old = SelectObject(hdc_mem, HGDIOBJ(hbm.0));

        // PW_RENDERFULLCONTENT (0x02) is required for hardware-accelerated
        // WebView2 content; without it PrintWindow returns a black image.
        const PW_RENDERFULLCONTENT: PRINT_WINDOW_FLAGS = PRINT_WINDOW_FLAGS(0x02);
        let pw_ok = PrintWindow(hwnd, hdc_mem, PW_RENDERFULLCONTENT).as_bool();
        if !pw_ok {
            // Fallback: BitBlt from the window DC. Won't include fully
            // hardware-composited frames but proves the pipeline works.
            let _ = BitBlt(hdc_mem, 0, 0, width, height, hdc_screen, 0, 0, SRCCOPY);
        }

        let mut bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                // Negative → top-down DIB, so the buffer is in scanline order.
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut buf = vec![0u8; (width as usize) * (height as usize) * 4];
        let scanlines = GetDIBits(
            hdc_mem,
            hbm,
            0,
            height as u32,
            Some(buf.as_mut_ptr() as *mut _),
            &mut bmi,
            DIB_RGB_COLORS,
        );

        SelectObject(hdc_mem, old);
        let _ = DeleteObject(HGDIOBJ(hbm.0));
        let _ = DeleteDC(hdc_mem);
        ReleaseDC(hwnd, hdc_screen);

        if scanlines == 0 {
            return Err("GetDIBits returned 0 scanlines".into());
        }

        // GDI gives BGRA; PNG wants RGBA. Swap in place. Also force alpha to
        // 0xFF since GDI leaves it zeroed for opaque pixels.
        for chunk in buf.chunks_exact_mut(4) {
            chunk.swap(0, 2);
            chunk[3] = 0xFF;
        }

        if let Some(parent) = out_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let file = std::fs::File::create(out_path)
            .map_err(|e| format!("create {}: {e}", out_path.display()))?;
        let mut encoder = png::Encoder::new(
            std::io::BufWriter::new(file),
            width as u32,
            height as u32,
        );
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|e| format!("png header: {e}"))?;
        writer
            .write_image_data(&buf)
            .map_err(|e| format!("png data: {e}"))?;
    }
    Ok(())
}

fn screenshot_path(port: u16) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join("llm-chat-qa");
    let _ = std::fs::create_dir_all(&dir);
    dir.join(format!("screenshot-{port}.png"))
}

// ========== Claude Discovery ==========
fn find_claude_path() -> Option<String> {
    if let Ok(path_var) = std::env::var("PATH") {
        let sep = if cfg!(windows) { ';' } else { ':' };
        for dir in path_var.split(sep) {
            let dir = dir.trim();
            if dir.is_empty() {
                continue;
            }
            for name in &["claude.exe", "claude.cmd", "claude"] {
                let full = std::path::Path::new(dir).join(name);
                if full.exists() {
                    return Some(full.to_string_lossy().into_owned());
                }
            }
        }
    }
    if let Ok(appdata) = std::env::var("APPDATA") {
        let npm_path = std::path::Path::new(&appdata).join("npm").join("claude.cmd");
        if npm_path.exists() {
            return Some(npm_path.to_string_lossy().into_owned());
        }
    }
    if let Ok(localappdata) = std::env::var("LOCALAPPDATA") {
        let claude_desktop = std::path::Path::new(&localappdata)
            .join("AnthropicClaude")
            .join("claude.exe");
        if claude_desktop.exists() {
            return Some(claude_desktop.to_string_lossy().into_owned());
        }
    }
    None
}

// ========== Tauri Commands ==========

#[cfg(any(unix, windows))]
fn do_spawn_session(
    session_id: String,
    cols: u16,
    rows: u16,
    state: &AppState,
    app_handle: &tauri::AppHandle,
) -> Result<String, String> {
    use tauri::Emitter;
    {
        let order = state.session_order.lock().unwrap();
        if order.len() >= MAX_SESSIONS {
            return Err(format!(
                "max session count reached ({}); close some before spawning more",
                MAX_SESSIONS
            ));
        }
    }
    let claude_path = find_claude_path().unwrap_or_else(|| {
        tracing::warn!(target: "backend::pty", "claude CLI not found on PATH");
        #[cfg(windows)]
        { "echo Claude CLI not found in PATH, APPDATA\\npm, or %LOCALAPPDATA%\\AnthropicClaude && pause".into() }
        #[cfg(unix)]
        { "echo 'Claude CLI not found in PATH'; sleep 30".into() }
    });
    // Claude needs explicit permission to use its Read tool on each file. In
    // a PTY-driven chat, the permission prompt blocks the input we typed past
    // it, so we always run claude with --dangerously-skip-permissions. The
    // user can override via $LLM_CHAT_CLAUDE_ARGS.
    let extra_args = std::env::var("LLM_CHAT_CLAUDE_ARGS")
        .unwrap_or_else(|_| "--dangerously-skip-permissions".into());
    let cmd = if extra_args.is_empty() {
        claude_path
    } else {
        format!("{} {}", claude_path, extra_args)
    };
    let c = if cols == 0 { 120i16 } else { cols as i16 };
    let r = if rows == 0 { 30i16 } else { rows as i16 };
    tracing::info!(
        target: "backend::session",
        sid = %session_id,
        cols = c,
        rows = r,
        cmd = %cmd,
        "spawning PTY session"
    );
    let _ = app_handle.emit(
        "new-pty-session",
        serde_json::json!({"sessionId": session_id}),
    );
    let session = pty::PtySession::create(&cmd, c, r)?;
    let (ws_tx, _ws_rx) = broadcast::channel::<Vec<u8>>(256);
    let (qa_tx, _qa_rx) = broadcast::channel::<String>(256);
    state
        .pty_broadcasts
        .lock()
        .unwrap()
        .insert(session_id.clone(), ws_tx.clone());
    state
        .qa_broadcasts
        .lock()
        .unwrap()
        .insert(session_id.clone(), qa_tx);
    state
        .session_order
        .lock()
        .unwrap()
        .push(session_id.clone());
    session.spawn_reader(app_handle.clone(), session_id.clone(), ws_tx);
    state
        .pty_sessions
        .lock()
        .unwrap()
        .insert(session_id.clone(), session);
    let _ = app_handle.emit(
        "claude-session",
        serde_json::json!({"sessionId": session_id}),
    );
    Ok(cmd)
}

#[tauri::command]
fn spawn_session(
    session_id: String,
    cols: u16,
    rows: u16,
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<String, String> {
    #[cfg(any(unix, windows))]
    {
        return do_spawn_session(session_id, cols, rows, &state, &app_handle);
    }
    #[allow(unreachable_code)]
    Err("Unsupported platform".into())
}

#[tauri::command]
fn close_session(
    session_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    #[cfg(any(unix, windows))]
    {
        let existed = state.pty_sessions.lock().unwrap().remove(&session_id).map(|mut s| s.close()).is_some();
        state.pty_broadcasts.lock().unwrap().remove(&session_id);
        state.qa_broadcasts.lock().unwrap().remove(&session_id);
        state.qa_history.lock().unwrap().remove(&session_id);
        state
            .session_order
            .lock()
            .unwrap()
            .retain(|id| id != &session_id);
        cleanup_attachments(&session_id);
        tracing::info!(
            target: "backend::session",
            sid = %session_id,
            existed,
            "PTY session closed"
        );
    }
    Ok(())
}

#[tauri::command]
fn list_sessions(state: tauri::State<'_, AppState>) -> Vec<String> {
    state.session_order.lock().unwrap().clone()
}

#[tauri::command]
fn set_active_session(
    session_id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    *state.active_session_id.lock().unwrap() = Some(session_id);
    Ok(())
}

// ========== Path/log safety helpers ==========
// All QA logs and control logs live under <temp>\llm-chat-qa. Reject any
// caller-provided path that escapes that directory.
fn qa_root_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join("llm-chat-qa");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn is_safe_qa_path(p: &str) -> bool {
    let root = match std::fs::canonicalize(qa_root_dir()) {
        Ok(r) => r,
        Err(_) => return false,
    };
    let path = std::path::Path::new(p);
    // For new files, canonicalize the parent (the file may not exist yet).
    let candidate = match path.parent() {
        Some(par) if !par.as_os_str().is_empty() => par,
        _ => return false,
    };
    match std::fs::canonicalize(candidate) {
        Ok(c) => c.starts_with(&root),
        Err(_) => false,
    }
}

// Enforce an upper bound on concurrent sessions so a runaway client can't
// spawn unbounded claude.exe processes.
const MAX_SESSIONS: usize = 50;

// ========== QA log commands (verbatim from onscreen-kbd) ==========

#[tauri::command]
fn get_qa_log_path(session_id: Option<String>) -> Result<String, String> {
    let dir = std::env::temp_dir().join("llm-chat-qa");
    std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create dir: {}", e))?;
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let days = secs / 86400;
    let hour = (secs % 86400) / 3600;
    let mut y = 1970i64;
    let mut remaining = days as i64;
    loop {
        let days_in_year = if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 { 366 } else { 365 };
        if remaining < days_in_year { break; }
        remaining -= days_in_year;
        y += 1;
    }
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let mdays = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut m = 0usize;
    for i in 0..12 {
        if remaining < mdays[i] { m = i + 1; break; }
        remaining -= mdays[i];
    }
    let d = remaining + 1;
    let suffix = session_id
        .as_deref()
        .map(|s| format!("_{}", s.replace(|c: char| !c.is_ascii_alphanumeric(), "")))
        .unwrap_or_default();
    let filename = format!("qa_claude_{:04}{:02}{:02}_{:02}{}.log", y, m, d, hour, suffix);
    let path = dir.join(filename);
    Ok(path.to_string_lossy().into_owned())
}

#[tauri::command]
fn append_qa_log(content: String, path: String) -> Result<(), String> {
    if !is_safe_qa_path(&path) {
        return Err("path is outside the QA log directory".into());
    }
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| format!("Failed to open {}: {}", path, e))?;
    file.write_all(content.as_bytes())
        .map_err(|e| format!("Failed to write: {}", e))?;
    Ok(())
}

#[tauri::command]
fn write_qa_log(content: String, path: String) -> Result<(), String> {
    if !is_safe_qa_path(&path) {
        return Err("path is outside the QA log directory".into());
    }
    std::fs::write(&path, content.as_bytes())
        .map_err(|e| format!("Failed to write {}: {}", path, e))?;
    Ok(())
}

#[tauri::command]
fn open_qa_log(path: String) -> Result<(), String> {
    if !is_safe_qa_path(&path) {
        return Err("path is outside the QA log directory".into());
    }
    // Use ShellExecuteW directly so the path argument is never re-parsed by
    // a shell — closes the cmd.exe quoting injection vector.
    #[cfg(windows)]
    {
        use windows::core::PCWSTR;
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::Shell::ShellExecuteW;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        let path_w: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
        let verb: Vec<u16> = "open".encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            let _ = ShellExecuteW(
                HWND::default(),
                PCWSTR(verb.as_ptr()),
                PCWSTR(path_w.as_ptr()),
                PCWSTR(std::ptr::null()),
                PCWSTR(std::ptr::null()),
                SW_SHOWNORMAL,
            );
        }
    }
    // xdg-open takes a single argument and execs the user's preferred handler;
    // no shell interpolation, so the same injection-resistance argument holds.
    #[cfg(unix)]
    {
        let _ = std::process::Command::new("xdg-open").arg(&path).spawn();
    }
    Ok(())
}

/// Strip Claude-CLI TUI footer noise from a parser-emitted answer.
///
/// Two passes:
///   1. Line-based filter for raw multi-line input.
///   2. Inline truncation at the first TUI-chrome glyph. The frontend parser
///      joins xterm visual rows with single spaces before invoking
///      broadcast_qa, which collapses chrome lines into the answer; pass 1
///      can't see them. The glyphs below (spinners, mode/model indicators)
///      never appear in normal prose, so truncating at the first occurrence
///      reliably cuts the footer without harming legitimate text.
fn clean_answer(text: &str) -> String {
    let line_filtered = text
        .lines()
        .map(|l| l.trim_end())
        .filter(|l| !l.is_empty())
        .filter(|l| {
            if let Some(c) = l.chars().next() {
                // Spinner glyphs: "✻ Cogitated for 6s", "✶ ...", "✽ ...", "✢ ..."
                if matches!(c, '✻' | '✶' | '✽' | '✢' | '✱' | '✷' | '✺') {
                    return false;
                }
                // Model/effort indicator line: "◉ xhigh · /effort"
                if matches!(c, '◉' | '◯') {
                    return false;
                }
            }
            // Claude's PATH hint and the export command it suggests
            if l.starts_with("Native installation exists") {
                return false;
            }
            if l.starts_with("echo 'export PATH=") {
                return false;
            }
            // Bottom hint bar: "? for shortcuts" / "esc to interrupt ..."
            if l.starts_with("? for shortcuts") || l.starts_with("esc to interrupt") {
                return false;
            }
            // Mode hint: "⏵⏵ bypass permissions on (shift+tab to cycle)"
            if l.starts_with("⏵⏵") || l.contains("(shift+tab to cycle)") {
                return false;
            }
            true
        })
        .collect::<Vec<&str>>()
        .join("\n");

    const CHROME_CHARS: &[char] = &[
        '⏵', '◉', '◯', '✻', '✶', '✢', '✷', '✺', '✱', '✽',
    ];
    match line_filtered.find(|c: char| CHROME_CHARS.contains(&c)) {
        Some(pos) => line_filtered[..pos].trim_end().to_string(),
        None => line_filtered,
    }
}

#[tauri::command]
fn broadcast_qa(
    num: u32,
    question: String,
    answer: String,
    session_id: Option<String>,
    is_new: Option<bool>,
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    use tauri::Emitter;
    // Normalize once at the source so every downstream consumer (in-app
    // history, Tauri event listeners, /qa/<sid> WebSocket subscribers, and
    // the manager bridge) receives the same cleaned text. The JS parser
    // already drops most chrome line-by-line, but anything that slipped
    // through and got space-joined into the answer is scrubbed here.
    let answer = clean_answer(&answer);
    let payload = serde_json::json!({
        "num": num,
        "question": question,
        "answer": answer,
        "sessionId": session_id,
        "isNew": is_new.unwrap_or(false),
    });
    let _ = app_handle.emit("qa-detected", &payload);
    if let Some(sid) = session_id.as_deref() {
        // Record per-session history so /control "history" can serve a snapshot.
        {
            let mut hist = state.qa_history.lock().unwrap();
            let entries = hist.entry(sid.to_string()).or_default();
            if is_new.unwrap_or(false) {
                entries.push(QaItem {
                    num,
                    question: question.clone(),
                    answer: answer.clone(),
                });
            } else if let Some(item) = entries.iter_mut().find(|i| i.num == num) {
                item.answer = answer.clone();
            } else {
                // First time seeing this num (parser sent isNew=false but we missed
                // the prior). Insert anyway.
                entries.push(QaItem {
                    num,
                    question: question.clone(),
                    answer: answer.clone(),
                });
            }
        }
        // Forward to any external WS subscribers on /qa/<sid>.
        if let Some(tx) = state.qa_broadcasts.lock().unwrap().get(sid).cloned() {
            let _ = tx.send(payload.to_string());
        }
    }
    Ok(())
}

#[tauri::command]
fn save_terminal_output(content: String) -> Result<String, String> {
    let dir = std::env::temp_dir().join("llm-chat-output");
    std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create dir: {}", e))?;
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let path = dir.join(format!("terminal_output_{}.txt", secs));
    std::fs::write(&path, content.as_bytes())
        .map_err(|e| format!("Failed to write: {}", e))?;
    Ok(path.to_string_lossy().into_owned())
}

#[tauri::command]
fn terminal_ready(
    _cols: u16,
    _rows: u16,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    state.terminal_ready.store(true, Ordering::Relaxed);
    Ok("ready".into())
}

#[tauri::command]
fn pty_write(
    session_id: String,
    data: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    #[cfg(any(unix, windows))]
    {
        let bytes = data.as_bytes();
        // xterm focus reports can hang ConPTY/cmd.exe during startup
        if bytes == b"\x1b[I" || bytes == b"\x1b[O" {
            return Ok(());
        }
        // Record into SQLite FIFO (best-effort) before writing to the PTY.
        // This is a sync command but the DB API is async — use the tauri async
        // runtime to drive the small INSERT inline. Latency is sub-millisecond
        // for SQLite WAL on local disk.
        let seq = tauri::async_runtime::block_on(pty_input_record(&session_id, bytes));
        let res = {
            let map = state.pty_sessions.lock().unwrap();
            match map.get(&session_id) {
                Some(session) => session.write(bytes),
                None => Err(format!("No PTY session: {}", session_id)),
            }
        };
        if let Some(seq) = seq {
            tauri::async_runtime::block_on(pty_input_mark(seq, res.is_ok()));
        }
        return res;
    }
    #[allow(unreachable_code)]
    Ok(())
}

#[tauri::command]
fn pty_resize(
    session_id: String,
    cols: u16,
    rows: u16,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    #[cfg(any(unix, windows))]
    {
        if cols == 0 || rows == 0 {
            return Ok(());
        }
        let map = state.pty_sessions.lock().unwrap();
        if let Some(session) = map.get(&session_id) {
            session.resize(cols as i16, rows as i16)?;
        }
    }
    Ok(())
}

// ========== Control log helper ==========
fn iso_now() -> String {
    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let total_secs = dur.as_secs();
    let millis = dur.subsec_millis();
    let day_secs = total_secs % 86400;
    let h = day_secs / 3600;
    let mi = (day_secs % 3600) / 60;
    let s = day_secs % 60;
    let mut y = 1970i64;
    let mut rem = (total_secs / 86400) as i64;
    loop {
        let dy = if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 { 366 } else { 365 };
        if rem < dy { break; }
        rem -= dy;
        y += 1;
    }
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let mdays = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut mo = 0usize;
    for i in 0..12 {
        if rem < mdays[i] { mo = i + 1; break; }
        rem -= mdays[i];
    }
    let d = rem + 1;
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z", y, mo, d, h, mi, s, millis)
}

const CONTROL_LOG_MAX_BYTES: u64 = 1024 * 1024;

fn today_yyyymmdd() -> String {
    let total_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let mut y = 1970i64;
    let mut rem = (total_secs / 86400) as i64;
    loop {
        let dy = if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 { 366 } else { 365 };
        if rem < dy { break; }
        rem -= dy;
        y += 1;
    }
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let mdays = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut mo = 0usize;
    for i in 0..12 {
        if rem < mdays[i] { mo = i + 1; break; }
        rem -= mdays[i];
    }
    let d = rem + 1;
    format!("{:04}{:02}{:02}", y, mo, d)
}

fn control_log_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join("llm-chat-qa");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn make_log_path(date: &str, seq: u32) -> std::path::PathBuf {
    control_log_dir().join(format!("control_{}_{:03}.log", date, seq))
}

// Tracks (current date, current sequence number) so we don't rescan the
// directory on every write.
fn ctrl_log_state() -> &'static std::sync::Mutex<(String, u32)> {
    use std::sync::{Mutex, OnceLock};
    static S: OnceLock<Mutex<(String, u32)>> = OnceLock::new();
    S.get_or_init(|| Mutex::new((String::new(), 0)))
}

fn next_seq_for_date(date: &str) -> u32 {
    let dir = control_log_dir();
    let prefix = format!("control_{}_", date);
    let mut max_seq: u32 = 0;
    if let Ok(read) = std::fs::read_dir(&dir) {
        for entry in read.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                if let Some(rest) = name.strip_prefix(&prefix) {
                    if let Some(num_str) = rest.strip_suffix(".log") {
                        if let Ok(n) = num_str.parse::<u32>() {
                            if n > max_seq { max_seq = n; }
                        }
                    }
                }
            }
        }
    }
    if max_seq == 0 { 1 } else { max_seq }
}

fn control_log_path() -> std::path::PathBuf {
    let today = today_yyyymmdd();
    let mut state = ctrl_log_state().lock().unwrap();
    if state.0 != today {
        state.0 = today.clone();
        state.1 = next_seq_for_date(&today);
    }
    let mut path = make_log_path(&today, state.1);
    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    if size >= CONTROL_LOG_MAX_BYTES {
        state.1 += 1;
        path = make_log_path(&today, state.1);
    }
    path
}

fn append_control_log(dir: &str, msg: &serde_json::Value) {
    use std::io::Write;
    let path = control_log_path();
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let line = serde_json::json!({
            "ts": iso_now(),
            "dir": dir,
            "msg": msg,
        });
        let _ = writeln!(f, "{}", line);
    }
}

// ========== WebSocket auth ==========
// Every WS endpoint requires a per-process random token. Manager passes the
// token to spawned backends via LLM_CHAT_AUTH_TOKEN; standalone backends
// fall back to %LOCALAPPDATA%\com.llm-chat.app\auth.token (per-user, not
// swept by temp cleaners) and tighten its ACL so only the current user can
// read it. The token is also printed to stderr at startup so external
// clients can capture it without needing filesystem access.
fn auth_token_file_path() -> std::path::PathBuf {
    // Per-user persistent app-data location, mirrors the manager's choice so
    // backend-{port}.sqlite ends up next to manager.sqlite in the same dir.
    //   Windows: %LOCALAPPDATA%\com.llm-chat.app\
    //   Linux/macOS: $XDG_DATA_HOME/com.llm-chat.app/  (default ~/.local/share)
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
                std::env::var_os("HOME")
                    .map(|h| std::path::PathBuf::from(h).join(".local/share"))
            });
        if let Some(b) = base {
            let dir = b.join("com.llm-chat.app");
            if std::fs::create_dir_all(&dir).is_ok() {
                return dir.join("auth.token");
            }
        }
    }
    qa_root_dir().join("auth.token")
}

fn lock_token_file_acl(path: &std::path::Path) {
    #[cfg(windows)]
    {
        let username = match std::env::var("USERNAME") {
            Ok(u) if !u.is_empty() => u,
            _ => return,
        };
        let _ = std::process::Command::new("icacls")
            .arg(path)
            .arg("/inheritance:r")
            .arg("/grant:r")
            .arg(format!("{}:F", username))
            .output();
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
}

fn load_or_generate_auth_token() -> String {
    if let Ok(t) = std::env::var("LLM_CHAT_AUTH_TOKEN") {
        if !t.is_empty() {
            return t;
        }
    }
    let token_path = auth_token_file_path();
    if let Ok(t) = std::fs::read_to_string(&token_path) {
        let trimmed = t.trim().to_string();
        if !trimmed.is_empty() {
            return trimmed;
        }
    }
    use rand::RngCore;
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    let token: String = bytes.iter().map(|b| format!("{:02x}", b)).collect();
    let _ = std::fs::write(&token_path, &token);
    lock_token_file_acl(&token_path);
    // Backend persists its own token only when standalone (no env var). The
    // manager already logged its persistence, so this is just diagnostic depth.
    tracing::debug!(
        target: "backend::auth",
        token_path = %token_path.display(),
        "persisted standalone auth token"
    );
    token
}

fn check_token_eq(provided: &str, expected: &str) -> bool {
    use subtle::ConstantTimeEq;
    provided.as_bytes().ct_eq(expected.as_bytes()).into()
}

fn extract_token_from_request(
    req: &tokio_tungstenite::tungstenite::handshake::server::Request,
) -> Option<String> {
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

// ========== WebSocket relay server ==========
// External chat clients can connect to ws://127.0.0.1:7878/s/<index> where
// <index> is the 1-based session number. The server bridges the WS frames to
// the session's PTY: WS text/binary -> PTY stdin; PTY output -> WS binary.
//
// Every connection must present the auth token, either via
// `Authorization: Bearer <token>` header or `?token=<token>` query string.
// Browser-style http(s) Origin headers are rejected outright.
#[cfg(any(unix, windows))]
fn start_ws_server(app_handle: tauri::AppHandle, port: u16) {
    use futures_util::{SinkExt, StreamExt};
    use tokio::net::TcpListener;
    use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
    use tokio_tungstenite::tungstenite::http;
    use tokio_tungstenite::tungstenite::Message;

    let auth_token = load_or_generate_auth_token();
    tracing::debug!(target: "backend::auth", token_len = auth_token.len(), "auth token loaded");

    tauri::async_runtime::spawn(async move {
        // Initialize SQLite-backed PTY input FIFO before binding the WS port,
        // so first writes after bind always land in the durable queue.
        let db_path = pty_db_path();
        match open_pty_db(&db_path).await {
            Ok(pool) => {
                if PTY_INPUT_DB.set(pool).is_err() {
                    tracing::debug!(target: "backend::db", "PTY_INPUT_DB already set");
                }
                tracing::info!(
                    target: "backend::db",
                    path = %db_path.display(),
                    "PTY input FIFO opened"
                );
            }
            Err(e) => {
                // Backend continues without DB — writes still flow but won't be
                // durably queued. Logged at ERROR so it's obvious.
                tracing::error!(
                    target: "backend::db",
                    path = %db_path.display(),
                    error = %e,
                    "open PTY input FIFO failed (running without durable queue)"
                );
            }
        }

        let addr = format!("127.0.0.1:{}", port);
        let listener = match TcpListener::bind(&addr).await {
            Ok(l) => l,
            Err(e) => {
                tracing::error!(target: "backend::ws", port, error = %e, "WS bind failed");
                return;
            }
        };
        // Manager logs the higher-level "backend ready" event after wait_for_tcp;
        // this stays at DEBUG so we don't double-report binding success.
        tracing::debug!(target: "backend::ws", port, "WS server listening");
        loop {
            let (stream, _peer) = match listener.accept().await {
                Ok(p) => p,
                Err(e) => {
                    tracing::warn!(target: "backend::ws", error = %e, "accept");
                    continue;
                }
            };
            let app_handle = app_handle.clone();
            let auth_token = auth_token.clone();
            tokio::spawn(async move {
                let path_holder = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
                let path_capture = path_holder.clone();
                let token_for_cb = auth_token.clone();
                let cb = move |req: &Request, resp: Response| -> Result<Response, ErrorResponse> {
                    *path_capture.lock().unwrap() = req.uri().path().to_string();
                    // Reject browser-style origins outright.
                    if let Some(origin) = req.headers().get("origin") {
                        let s = origin.to_str().unwrap_or("");
                        if s.starts_with("http://") || s.starts_with("https://") {
                            return Err(http::Response::builder()
                                .status(http::StatusCode::FORBIDDEN)
                                .body(Some("origin not allowed".to_string()))
                                .unwrap());
                        }
                    }
                    let provided = match extract_token_from_request(req) {
                        Some(t) => t,
                        None => {
                            return Err(http::Response::builder()
                                .status(http::StatusCode::UNAUTHORIZED)
                                .body(Some("missing auth token".to_string()))
                                .unwrap());
                        }
                    };
                    if !check_token_eq(&provided, &token_for_cb) {
                        return Err(http::Response::builder()
                            .status(http::StatusCode::UNAUTHORIZED)
                            .body(Some("invalid auth token".to_string()))
                            .unwrap());
                    }
                    Ok(resp)
                };
                let ws = match tokio_tungstenite::accept_hdr_async(stream, cb).await {
                    Ok(w) => w,
                    Err(e) => {
                        // Common: nc -zv probes, manager wait_for_tcp, port scanners.
                        // Log at DEBUG so it doesn't pollute INFO output.
                        tracing::debug!(target: "backend::ws", error = %e, "handshake");
                        return;
                    }
                };
                let req_path = path_holder.lock().unwrap().clone();
                let state = {
                    use tauri::Manager;
                    app_handle.state::<AppState>()
                };

                let (mut ws_sink, mut ws_stream) = ws.split();

                // Resolve session id from the path. Crucially: clone any data
                // out of the Mutex guards BEFORE we hit an `.await`, otherwise
                // the future captured by tokio::spawn isn't `Send`.
                let order_snapshot: Vec<String> =
                    state.session_order.lock().unwrap().clone();

                // /control endpoint — JSON command channel. Clients send one
                // command per line: {"cmd":"open"} | {"cmd":"close","sessionId":"..."} |
                // {"cmd":"list"}. Server replies with one JSON line per command.
                if req_path == "/control" {
                    let app_handle_ctrl = app_handle.clone();
                    let hello = serde_json::json!({"ok": true, "hello": "control"});
                    append_control_log("hello", &hello);
                    let _ = ws_sink.send(Message::Text(hello.to_string())).await;
                    while let Some(msg) = ws_stream.next().await {
                        let text = match msg {
                            Ok(Message::Text(t)) => t,
                            Ok(Message::Close(_)) => break,
                            Ok(_) => continue,
                            Err(_) => break,
                        };
                        let req: serde_json::Value =
                            serde_json::from_str(&text).unwrap_or(serde_json::json!({}));
                        append_control_log("in", &req);
                        let cmd = req.get("cmd").and_then(|v| v.as_str()).unwrap_or("");
                        let reply: serde_json::Value = match cmd {
                            "screenshot" => {
                                #[cfg(windows)]
                                {
                                    let out = screenshot_path(port);
                                    match capture_main_window_to_png(&app_handle_ctrl, &out) {
                                        Ok(()) => serde_json::json!({
                                            "ok": true,
                                            "path": out.to_string_lossy(),
                                            "port": port,
                                        }),
                                        Err(e) => serde_json::json!({"ok": false, "error": e}),
                                    }
                                }
                                #[cfg(not(windows))]
                                {
                                    serde_json::json!({"ok": false, "error": "screenshot is windows-only"})
                                }
                            }
                            "list" => {
                                use tauri::Manager;
                                let st = app_handle_ctrl.state::<AppState>();
                                let order = st.session_order.lock().unwrap().clone();
                                let active = st.active_session_id.lock().unwrap().clone();
                                serde_json::json!({
                                    "ok": true,
                                    "count": order.len(),
                                    "sessions": order,
                                    "active": active,
                                })
                            }
                            "info" => {
                                use tauri::Manager;
                                let st = app_handle_ctrl.state::<AppState>();
                                let order = st.session_order.lock().unwrap().clone();
                                let active = st.active_session_id.lock().unwrap().clone();
                                let active_index = active
                                    .as_ref()
                                    .and_then(|a| order.iter().position(|x| x == a))
                                    .map(|i| i + 1);
                                serde_json::json!({
                                    "ok": true,
                                    "count": order.len(),
                                    "sessions": order,
                                    "active": active,
                                    "activeIndex": active_index,
                                    "logPath": control_log_path().to_string_lossy(),
                                })
                            }
                            "current" => {
                                use tauri::Manager;
                                let st = app_handle_ctrl.state::<AppState>();
                                let active = st.active_session_id.lock().unwrap().clone();
                                let order = st.session_order.lock().unwrap().clone();
                                let active_index = active
                                    .as_ref()
                                    .and_then(|a| order.iter().position(|x| x == a))
                                    .map(|i| i + 1);
                                serde_json::json!({
                                    "ok": true,
                                    "active": active,
                                    "activeIndex": active_index,
                                    "sessions": order,
                                })
                            }
                            "clear" => {
                                let raw_sid = req
                                    .get("sessionId")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string();
                                let what = req
                                    .get("what")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("all")
                                    .to_string();
                                use tauri::Manager;
                                let st = app_handle_ctrl.state::<AppState>();
                                let order = st.session_order.lock().unwrap().clone();
                                let resolved = if let Ok(idx) = raw_sid.parse::<usize>() {
                                    if idx >= 1 && idx <= order.len() {
                                        Some(order[idx - 1].clone())
                                    } else {
                                        None
                                    }
                                } else if !raw_sid.is_empty() {
                                    Some(raw_sid.clone())
                                } else {
                                    None
                                };
                                match resolved {
                                    Some(sid) => {
                                        if what == "stream" || what == "all" {
                                            st.qa_history.lock().unwrap().remove(&sid);
                                            use tauri::Emitter;
                                            let _ = app_handle_ctrl.emit(
                                                "external-clear-stream",
                                                serde_json::json!({"sessionId": sid}),
                                            );
                                        }
                                        if what == "terminal" || what == "all" {
                                            use tauri::Emitter;
                                            let _ = app_handle_ctrl.emit(
                                                "external-clear-terminal",
                                                serde_json::json!({"sessionId": sid}),
                                            );
                                        }
                                        serde_json::json!({"ok":true,"sessionId":sid,"what":what})
                                    }
                                    None => serde_json::json!({"ok":false,"error":"bad sessionId"}),
                                }
                            }
                            "history" => {
                                use tauri::Manager;
                                let st = app_handle_ctrl.state::<AppState>();
                                let order = st.session_order.lock().unwrap().clone();
                                let hist = st.qa_history.lock().unwrap().clone();
                                let target = req.get("sessionId").and_then(|v| v.as_str());
                                if let Some(sid) = target {
                                    // Resolve numeric index → session id.
                                    let resolved = if let Ok(idx) = sid.parse::<usize>() {
                                        if idx >= 1 && idx <= order.len() {
                                            Some(order[idx - 1].clone())
                                        } else {
                                            None
                                        }
                                    } else {
                                        Some(sid.to_string())
                                    };
                                    match resolved {
                                        Some(s) => serde_json::json!({
                                            "ok": true,
                                            "sessionId": s,
                                            "history": hist.get(&s).cloned().unwrap_or_default(),
                                        }),
                                        None => serde_json::json!({"ok":false,"error":"bad sessionId"}),
                                    }
                                } else {
                                    // No session specified → return histories for every session.
                                    let mut out = serde_json::Map::new();
                                    for sid in &order {
                                        out.insert(
                                            sid.clone(),
                                            serde_json::to_value(
                                                hist.get(sid).cloned().unwrap_or_default(),
                                            )
                                            .unwrap_or(serde_json::Value::Null),
                                        );
                                    }
                                    serde_json::json!({"ok":true,"histories":out})
                                }
                            }
                            "open" => {
                                use tauri::Manager;
                                // Wait for the webview JS to register its
                                // event listeners (signalled by the
                                // terminal_ready command). Without this we'd
                                // emit "external-session-added" into the void
                                // and the GUI parser would never start for
                                // this session.
                                {
                                    let st = app_handle_ctrl.state::<AppState>();
                                    let mut waits = 0;
                                    while !st
                                        .terminal_ready
                                        .load(std::sync::atomic::Ordering::Relaxed)
                                    {
                                        tokio::time::sleep(
                                            std::time::Duration::from_millis(100),
                                        )
                                        .await;
                                        waits += 1;
                                        if waits > 100 {
                                            break;
                                        }
                                    }
                                }
                                let st_handle = app_handle_ctrl.state::<AppState>();
                                let id = format!(
                                    "s{}",
                                    std::time::SystemTime::now()
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .map(|d| d.as_micros())
                                        .unwrap_or(0)
                                );
                                let res = do_spawn_session(
                                    id.clone(),
                                    120,
                                    30,
                                    &*st_handle,
                                    &app_handle_ctrl,
                                );
                                match res {
                                    Ok(_) => {
                                        use tauri::Emitter;
                                        let _ = app_handle_ctrl.emit(
                                            "external-session-added",
                                            serde_json::json!({"sessionId": id}),
                                        );
                                        serde_json::json!({"ok":true,"sessionId":id})
                                    }
                                    Err(e) => serde_json::json!({"ok":false,"error":e}),
                                }
                            }
                            "close" => {
                                let sid = req
                                    .get("sessionId")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string();
                                use tauri::Manager;
                                let st = app_handle_ctrl.state::<AppState>();
                                if let Some(mut sess) =
                                    st.pty_sessions.lock().unwrap().remove(&sid)
                                {
                                    sess.close();
                                }
                                st.pty_broadcasts.lock().unwrap().remove(&sid);
                                st.qa_broadcasts.lock().unwrap().remove(&sid);
                                st.qa_history.lock().unwrap().remove(&sid);
                                st.session_order
                                    .lock()
                                    .unwrap()
                                    .retain(|x| x != &sid);
                                cleanup_attachments(&sid);
                                use tauri::Emitter;
                                let _ = app_handle_ctrl.emit(
                                    "external-session-closed",
                                    serde_json::json!({"sessionId": sid}),
                                );
                                serde_json::json!({"ok":true,"sessionId":sid})
                            }
                            "switch" => {
                                let sid = req
                                    .get("sessionId")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string();
                                use tauri::Manager;
                                let st = app_handle_ctrl.state::<AppState>();
                                let exists = st.session_order.lock().unwrap().contains(&sid);
                                if !exists {
                                    serde_json::json!({"ok":false,"error":format!("no session: {}", sid)})
                                } else {
                                    use tauri::Emitter;
                                    let _ = app_handle_ctrl.emit(
                                        "external-switch-session",
                                        serde_json::json!({"sessionId": sid}),
                                    );
                                    serde_json::json!({"ok":true,"sessionId":sid})
                                }
                            }
                            "log" => {
                                let path = control_log_path();
                                serde_json::json!({
                                    "ok": true,
                                    "path": path.to_string_lossy(),
                                })
                            }
                            "save_attachment" => {
                                // Manager-only call: { sid, name, mime, data:base64 }
                                // → { ok, path } (absolute file path on this host)
                                let sid = req.get("sid").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                let name = req.get("name").and_then(|v| v.as_str()).unwrap_or("attachment").to_string();
                                let mime = req.get("mime").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                let data = req.get("data").and_then(|v| v.as_str()).unwrap_or("").to_string();
                                if sid.is_empty() || mime.is_empty() || data.is_empty() {
                                    serde_json::json!({"ok":false,"error":"sid+mime+data required"})
                                } else {
                                    match save_attachment(&sid, &name, &mime, &data) {
                                        Ok(p) => serde_json::json!({"ok":true,"path":p.to_string_lossy()}),
                                        Err(e) => serde_json::json!({"ok":false,"error":e}),
                                    }
                                }
                            }
                            "fifo" => {
                                // Inspect the SQLite-backed PTY input FIFO.
                                // Optional filters: sid (string), status (pending|written|error),
                                // limit (default 100, capped at 1000).
                                let sid_filter = req.get("sid").and_then(|v| v.as_str()).map(|s| s.to_string());
                                let status_filter = req.get("status").and_then(|v| v.as_str()).map(|s| s.to_string());
                                let limit = req.get("limit").and_then(|v| v.as_i64()).unwrap_or(100).clamp(1, 1000);
                                match query_pty_input(sid_filter.as_deref(), status_filter.as_deref(), limit).await {
                                    Ok(rows) => serde_json::json!({"ok": true, "rows": rows, "count": rows.len()}),
                                    Err(e) => serde_json::json!({"ok": false, "error": format!("fifo query: {}", e)}),
                                }
                            }
                            other => serde_json::json!({"ok":false,"error":format!("unknown cmd: {}", other)}),
                        };
                        append_control_log("out", &reply);
                        if ws_sink
                            .send(Message::Text(reply.to_string()))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    return;
                }

                // /qa/<id-or-index> — read-only stream of parsed Q&A as JSON
                // lines, scoped to one session.
                if req_path.starts_with("/qa/") {
                    let trimmed = &req_path[4..];
                    let qa_session_id = if let Ok(idx) = trimmed.parse::<usize>() {
                        if idx == 0 || idx > order_snapshot.len() {
                            let _ = ws_sink
                                .send(Message::Text(format!(
                                    "session index {} out of range",
                                    idx
                                )))
                                .await;
                            return;
                        }
                        order_snapshot[idx - 1].clone()
                    } else {
                        trimmed.to_string()
                    };
                    let qa_tx_opt: Option<broadcast::Sender<String>> = state
                        .qa_broadcasts
                        .lock()
                        .unwrap()
                        .get(&qa_session_id)
                        .cloned();
                    let mut qa_rx = match qa_tx_opt {
                        Some(tx) => tx.subscribe(),
                        None => {
                            let _ = ws_sink
                                .send(Message::Text(format!("no session: {}", qa_session_id)))
                                .await;
                            return;
                        }
                    };
                    let _ = ws_sink
                        .send(Message::Text(serde_json::json!({
                            "type": "subscribed",
                            "sessionId": qa_session_id,
                        }).to_string()))
                        .await;
                    loop {
                        match qa_rx.recv().await {
                            Ok(json_line) => {
                                if ws_sink.send(Message::Text(json_line)).await.is_err() {
                                    break;
                                }
                            }
                            Err(broadcast::error::RecvError::Lagged(_)) => continue,
                            Err(broadcast::error::RecvError::Closed) => break,
                        }
                    }
                    return;
                }

                let session_id: String = if req_path == "/" || req_path.is_empty() {
                    let body = serde_json::to_string(&order_snapshot).unwrap_or_default();
                    let _ = ws_sink.send(Message::Text(body)).await;
                    return;
                } else {
                    let trimmed = req_path
                        .trim_start_matches("/s/")
                        .trim_start_matches('/');
                    if let Ok(idx) = trimmed.parse::<usize>() {
                        if idx == 0 || idx > order_snapshot.len() {
                            let msg = format!(
                                "session index {} out of range (have {})",
                                idx,
                                order_snapshot.len()
                            );
                            let _ = ws_sink.send(Message::Text(msg)).await;
                            return;
                        }
                        order_snapshot[idx - 1].clone()
                    } else {
                        trimmed.to_string()
                    }
                };

                // Subscribe to broadcast for this session.
                let bcast_opt: Option<broadcast::Sender<Vec<u8>>> = state
                    .pty_broadcasts
                    .lock()
                    .unwrap()
                    .get(&session_id)
                    .cloned();
                let mut rx = match bcast_opt {
                    Some(tx) => tx.subscribe(),
                    None => {
                        let _ = ws_sink
                            .send(Message::Text(format!("no session: {}", session_id)))
                            .await;
                        return;
                    }
                };

                let _ = ws_sink
                    .send(Message::Text(format!("connected to session {}", session_id)))
                    .await;

                let ws_sink = std::sync::Arc::new(tokio::sync::Mutex::new(ws_sink));

                // PTY -> WS
                let sink_for_forward = ws_sink.clone();
                let forward = tokio::spawn(async move {
                    loop {
                        match rx.recv().await {
                            Ok(bytes) => {
                                let mut s = sink_for_forward.lock().await;
                                if s.send(Message::Binary(bytes)).await.is_err() {
                                    break;
                                }
                            }
                            Err(broadcast::error::RecvError::Lagged(_)) => continue,
                            Err(broadcast::error::RecvError::Closed) => break,
                        }
                    }
                });

                // WS -> PTY
                let app_handle_inner = app_handle.clone();
                let session_for_write = session_id.clone();
                while let Some(msg) = ws_stream.next().await {
                    let msg = match msg {
                        Ok(m) => m,
                        Err(_) => break,
                    };
                    let bytes: Vec<u8> = match msg {
                        Message::Text(t) => t.into_bytes(),
                        Message::Binary(b) => b,
                        Message::Close(_) => break,
                        _ => continue,
                    };
                    // Record the write into the SQLite FIFO BEFORE doing the
                    // PTY write, so a crash mid-write leaves a 'pending' row
                    // we can audit/replay later. Then write to PTY and mark.
                    let seq = pty_input_record(&session_for_write, &bytes).await;
                    // Scope the std::sync::Mutex guard so it's dropped before
                    // the next iteration's `.await` (not Send across await).
                    let write_ok = {
                        use tauri::Manager;
                        let st = app_handle_inner.state::<AppState>();
                        let map = st.pty_sessions.lock().unwrap();
                        match map.get(&session_for_write) {
                            Some(sess) => sess.write(&bytes).is_ok(),
                            None => false,
                        }
                    };
                    if let Some(seq) = seq {
                        pty_input_mark(seq, write_ok).await;
                    }
                }
                forward.abort();
            });
        }
    });
}

/// Init the backend's tracing subscriber. Mirrors the manager's setup so the
/// two streams interleave cleanly in shared logs. RUST_LOG default is INFO;
/// override per-component, e.g. `RUST_LOG=backend=debug,backend::pty=trace`.
fn init_backend_tracing() {
    use tracing_subscriber::EnvFilter;
    // Idempotent: try_init avoids panicking if a subscriber is already set
    // (e.g. when the backend is loaded as a library by tests).
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"));
    let json = matches!(std::env::var("LOG_JSON").ok().as_deref(), Some("1") | Some("true"));
    let _ = if json {
        tracing_subscriber::fmt()
            .json()
            .with_env_filter(filter)
            .with_writer(std::io::stderr)
            .try_init()
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_target(true)
            .with_writer(std::io::stderr)
            .try_init()
    };
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    init_backend_tracing();
    let backend_port: Option<u16> = std::env::var("LLM_CHAT_WS_PORT")
        .ok()
        .and_then(|v| v.parse().ok());
    tracing::info!(
        target: "backend",
        port = backend_port.unwrap_or(0),
        stealth = std::env::var("LLM_CHAT_STEALTH").ok().as_deref() == Some("1"),
        managed = std::env::var("LLM_CHAT_AUTH_TOKEN").is_ok(),
        "backend starting"
    );
    // Claude Code v2.1+ on Windows refuses to start without bash.exe. If the
    // user has Git for Windows installed but bash isn't on PATH and the env
    // var isn't set, set it now so child processes inherit it.
    if std::env::var("CLAUDE_CODE_GIT_BASH_PATH").is_err() {
        if let Some(bash) = find_git_bash_path() {
            std::env::set_var("CLAUDE_CODE_GIT_BASH_PATH", &bash);
        }
    }
    // Hint full-color, dark-theme terminal so claude renders its rich TUI
    // (orange box, mascot, etc.) instead of the degraded plain mode it falls
    // back to when capabilities look weak. `npm run tauri dev` sets these via
    // npm; bare `cargo build` + manager spawn doesn't, so set them ourselves.
    if std::env::var("FORCE_COLOR").is_err() {
        std::env::set_var("FORCE_COLOR", "3");
    }
    if std::env::var("COLORFGBG").is_err() {
        std::env::set_var("COLORFGBG", "15;0");
    }
    if std::env::var("COLORTERM").is_err() {
        std::env::set_var("COLORTERM", "truecolor");
    }
    if std::env::var("TERM").is_err() {
        std::env::set_var("TERM", "xterm-256color");
    }
    // UTF-8 locale so claude/Ink uses Unicode box drawing instead of the
    // ASCII fallback (`|` pipes, blocky mascot) we get in clean-env spawns.
    if std::env::var("LANG").is_err() {
        std::env::set_var("LANG", "en_US.UTF-8");
    }
    if std::env::var("LC_ALL").is_err() {
        std::env::set_var("LC_ALL", "en_US.UTF-8");
    }
    tauri::Builder::default()
        .manage(AppState {
            #[cfg(any(unix, windows))]
            pty_sessions: Mutex::new(HashMap::new()),
            pty_broadcasts: Mutex::new(HashMap::new()),
            qa_broadcasts: Mutex::new(HashMap::new()),
            qa_history: Mutex::new(HashMap::new()),
            session_order: Mutex::new(Vec::new()),
            active_session_id: Mutex::new(None),
            terminal_ready: Arc::new(AtomicBool::new(false)),
        })
        .invoke_handler(tauri::generate_handler![
            terminal_ready,
            pty_write,
            pty_resize,
            spawn_session,
            close_session,
            list_sessions,
            set_active_session,
            get_qa_log_path,
            append_qa_log,
            write_qa_log,
            open_qa_log,
            broadcast_qa,
            save_terminal_output,
        ])
        .setup(|app| {
            #[cfg(any(unix, windows))]
            {
                let port: u16 = std::env::var("LLM_CHAT_WS_PORT")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(7878);
                start_ws_server(app.handle().clone(), port);
            }
            // Stealth mode: hide the main window at startup. The Rust process
            // and WS server keep running normally; just no visible UI / no
            // taskbar entry. Useful for headless tests.
            if std::env::var("LLM_CHAT_STEALTH").ok().as_deref() == Some("1") {
                use tauri::Manager;
                if let Some(win) = app.get_webview_window("main") {
                    let _ = win.hide();
                    let _ = win.set_skip_taskbar(true);
                }
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
