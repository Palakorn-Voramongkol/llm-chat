use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

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

#[derive(Clone, serde::Serialize)]
struct QaItem {
    num: u32,
    question: String,
    answer: String,
}

// ========== App State ==========
struct AppState {
    #[cfg(windows)]
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

#[cfg(windows)]
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
    let cmd = find_claude_path().unwrap_or_else(|| {
        "echo Claude CLI not found in PATH, APPDATA\\npm, or %LOCALAPPDATA%\\AnthropicClaude && pause".into()
    });
    let c = if cols == 0 { 120i16 } else { cols as i16 };
    let r = if rows == 0 { 30i16 } else { rows as i16 };
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
    #[cfg(windows)]
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
    #[cfg(windows)]
    {
        if let Some(mut sess) = state.pty_sessions.lock().unwrap().remove(&session_id) {
            sess.close();
        }
        state.pty_broadcasts.lock().unwrap().remove(&session_id);
        state.qa_broadcasts.lock().unwrap().remove(&session_id);
        state.qa_history.lock().unwrap().remove(&session_id);
        state
            .session_order
            .lock()
            .unwrap()
            .retain(|id| id != &session_id);
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
    Ok(())
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
    #[cfg(windows)]
    {
        let bytes = data.as_bytes();
        // xterm focus reports can hang ConPTY/cmd.exe during startup
        if bytes == b"\x1b[I" || bytes == b"\x1b[O" {
            return Ok(());
        }
        let map = state.pty_sessions.lock().unwrap();
        if let Some(session) = map.get(&session_id) {
            session.write(bytes)?;
            return Ok(());
        }
        return Err(format!("No PTY session: {}", session_id));
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
    #[cfg(windows)]
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
// generate one and write it to <temp>\llm-chat-qa\auth.token so local
// scripts/the user can read it (filesystem ACL is the only barrier — fine
// for loopback-only).
fn load_or_generate_auth_token() -> String {
    if let Ok(t) = std::env::var("LLM_CHAT_AUTH_TOKEN") {
        if !t.is_empty() {
            return t;
        }
    }
    let token_path = qa_root_dir().join("auth.token");
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
#[cfg(windows)]
fn start_ws_server(app_handle: tauri::AppHandle, port: u16) {
    use futures_util::{SinkExt, StreamExt};
    use tokio::net::TcpListener;
    use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
    use tokio_tungstenite::tungstenite::http;
    use tokio_tungstenite::tungstenite::Message;

    let auth_token = load_or_generate_auth_token();
    eprintln!("[ws] auth token loaded ({} chars)", auth_token.len());

    tauri::async_runtime::spawn(async move {
        let addr = format!("127.0.0.1:{}", port);
        let listener = match TcpListener::bind(&addr).await {
            Ok(l) => l,
            Err(e) => {
                eprintln!("WS bind {} failed: {}", addr, e);
                return;
            }
        };
        eprintln!("WS server listening on ws://{}", addr);
        loop {
            let (stream, _peer) = match listener.accept().await {
                Ok(p) => p,
                Err(e) => { eprintln!("WS accept: {}", e); continue; }
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
                    Err(e) => { eprintln!("WS handshake: {}", e); return; }
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
                    // Scope the std::sync::Mutex guard so it's dropped before
                    // the next iteration's `.await`, otherwise the future is
                    // not `Send`.
                    {
                        use tauri::Manager;
                        let st = app_handle_inner.state::<AppState>();
                        let map = st.pty_sessions.lock().unwrap();
                        if let Some(sess) = map.get(&session_for_write) {
                            let _ = sess.write(&bytes);
                        }
                    }
                }
                forward.abort();
            });
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Claude Code v2.1+ on Windows refuses to start without bash.exe. If the
    // user has Git for Windows installed but bash isn't on PATH and the env
    // var isn't set, set it now so child processes inherit it.
    if std::env::var("CLAUDE_CODE_GIT_BASH_PATH").is_err() {
        if let Some(bash) = find_git_bash_path() {
            std::env::set_var("CLAUDE_CODE_GIT_BASH_PATH", &bash);
        }
    }
    tauri::Builder::default()
        .manage(AppState {
            #[cfg(windows)]
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
            #[cfg(windows)]
            {
                let port: u16 = std::env::var("LLM_CHAT_WS_PORT")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(7878);
                start_ws_server(app.handle().clone(), port);
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
