// llm-chat-launcher — tiny wrapper that spawns one llm-chat.exe instance.
//
// Configuration (env vars, all optional):
//   LLM_CHAT_EXE       — full path to llm-chat.exe (default: derived from
//                         this binary's location, expects sibling layout
//                         <project>/launcher/target/.../llm-chat-launcher.exe
//                         <project>/src-tauri/target/debug/llm-chat.exe)
//   LLM_CHAT_WS_PORT   — port the spawned llm-chat WS server should bind to
//                         (default: 7878, decided by llm-chat itself)
//   LLM_CHAT_AUTH_TOKEN — auth token to inject into the spawned instance
//                         (default: not set; llm-chat then generates one and
//                          writes it to %TEMP%\llm-chat-qa\auth.token)

use std::path::PathBuf;
use std::process::Command;

fn default_exe_path() -> PathBuf {
    if let Ok(p) = std::env::var("LLM_CHAT_EXE") {
        return PathBuf::from(p);
    }
    let here = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("."));
    let dir = here.parent().unwrap_or(std::path::Path::new("."));
    // Default layout: <repo>/launcher/target/{debug,release}/llm-chat-launcher.exe
    //                 <repo>/src-tauri/target/debug/llm-chat.exe
    let candidate = dir
        .join("..")
        .join("..")
        .join("..")
        .join("src-tauri")
        .join("target")
        .join("debug")
        .join("llm-chat.exe");
    candidate.canonicalize().unwrap_or(candidate)
}

fn main() -> std::io::Result<()> {
    let exe = default_exe_path();
    eprintln!("[launcher] launching: {}", exe.display());

    let mut cmd = Command::new(&exe);
    if let Ok(port) = std::env::var("LLM_CHAT_WS_PORT") {
        cmd.env("LLM_CHAT_WS_PORT", port);
    }
    if let Ok(tok) = std::env::var("LLM_CHAT_AUTH_TOKEN") {
        cmd.env("LLM_CHAT_AUTH_TOKEN", tok);
    }

    let mut child = cmd.spawn().map_err(|e| {
        eprintln!("[launcher] spawn failed: {}", e);
        e
    })?;
    eprintln!("[launcher] spawned pid {}", child.id());

    let status = child.wait()?;
    eprintln!("[launcher] llm-chat exited with status {}", status);
    Ok(())
}
