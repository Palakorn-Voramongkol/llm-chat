//! One-shot, confined LLM sandbox migration (Sub-project 2). Drives `claude`
//! in stream-json mode (source of truth — never scrapes a TTY) with cwd locked
//! to a single user's `{userId}/{app}/` folder. Pure helpers are unit-tested;
//! the spawn is exercised by the live e2e.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

/// PURE: render the desired template as an LLM-readable manifest (target
/// layout). Files show their content; directories are marked.
pub fn render_manifest(entries: &[crate::user_env::SeedEntry]) -> String {
    let mut out = String::new();
    for e in entries {
        if e.dir {
            out.push_str(&format!("- DIR  {}\n", e.path));
        } else {
            out.push_str(&format!("- FILE {}\n  ----- desired content -----\n", e.path));
            for line in e.content.lines() {
                out.push_str(&format!("  {line}\n"));
            }
            out.push_str("  ----- end -----\n");
        }
    }
    if out.is_empty() {
        out.push_str("(empty template)\n");
    }
    out
}

/// PURE: build the migration prompt. The instructions are operator-authored;
/// the manifest is the desired (already variable-substituted) layout.
pub fn migration_prompt(instructions: &str, manifest: &str) -> String {
    format!(
        "You are migrating the current working directory (a user's app sandbox) \
to a new template version. Apply ONLY the migration described below. Do not \
touch hidden files under .llm-chat/. Be idempotent: if the change is already \
applied, make no edits.\n\n\
=== MIGRATION INSTRUCTIONS ===\n{instructions}\n\n\
=== DESIRED TEMPLATE (target layout) ===\n{manifest}\n\
=== END ===\n\nPerform the migration now."
    )
}

/// PURE: classify a single claude stream-json stdout line. Some(true) on a
/// successful `result` event, Some(false) on a failed one, None otherwise.
/// Mirrors the JsonSession reader's success rule.
pub fn migration_result_ok(line: &str) -> Option<bool> {
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    if v.get("type").and_then(|x| x.as_str()) != Some("result") {
        return None;
    }
    let ok = v.get("subtype").and_then(|x| x.as_str()) == Some("success")
        || v.get("is_error").and_then(|x| x.as_bool()) == Some(false);
    Some(ok)
}

/// Run a one-shot claude migration in `cwd` (BLOCKING). Returns Ok(()) only on a
/// successful `result` event. Kills the child after `timeout`. Fail closed:
/// any spawn/read/timeout/failed-result → Err.
pub fn run_box_migration(
    claude_path: &str,
    cwd: &Path,
    prompt: &str,
    timeout: Duration,
) -> Result<(), String> {
    // Pre-trust the cwd so claude's TUI trust dialog never blocks the run.
    let cwd_str = cwd.to_string_lossy().to_string();
    let _ = crate::ensure_claude_trusts(&cwd_str);

    let args = [
        "-p",
        "--input-format", "stream-json",
        "--output-format", "stream-json",
        "--verbose",
        "--dangerously-skip-permissions",
    ];
    let lower = claude_path.to_ascii_lowercase();
    let mut cmd = if cfg!(windows) && (lower.ends_with(".cmd") || lower.ends_with(".bat")) {
        let mut c = Command::new("cmd.exe");
        c.arg("/c").arg(claude_path).args(args);
        c
    } else {
        let mut c = Command::new(claude_path);
        c.args(args);
        c
    };
    cmd.current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    let mut child = cmd.spawn().map_err(|e| format!("spawn claude (migrate): {e}"))?;

    // One stream-json user message, then close stdin so claude finishes the turn.
    {
        let mut stdin = child.stdin.take().ok_or("no child stdin")?;
        let msg = serde_json::json!({
            "type": "user",
            "message": {"role": "user", "content": [{"type": "text", "text": prompt}]}
        });
        let mut line = msg.to_string();
        line.push('\n');
        stdin.write_all(line.as_bytes()).map_err(|e| format!("write stdin: {e}"))?;
        stdin.flush().map_err(|e| format!("flush stdin: {e}"))?;
        // stdin dropped here → EOF.
    }

    // Watchdog: kill the child if it overruns the timeout.
    let killer = child.id();
    let kill_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    {
        let kf = kill_flag.clone();
        std::thread::spawn(move || {
            std::thread::sleep(timeout);
            if !kf.load(std::sync::atomic::Ordering::SeqCst) {
                // Best-effort kill by pid (platform tools); the wait() below then returns.
                #[cfg(windows)]
                let _ = Command::new("taskkill").args(["/PID", &killer.to_string(), "/T", "/F"]).output();
                #[cfg(unix)]
                let _ = Command::new("kill").args(["-9", &killer.to_string()]).output();
            }
        });
    }

    let stdout = child.stdout.take().ok_or("no child stdout")?;
    let reader = BufReader::new(stdout);
    let mut result: Option<bool> = None;
    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        if line.trim().is_empty() {
            continue;
        }
        if let Some(ok) = migration_result_ok(&line) {
            result = Some(ok);
        }
    }
    kill_flag.store(true, std::sync::atomic::Ordering::SeqCst);
    let _ = child.wait();
    match result {
        Some(true) => Ok(()),
        Some(false) => Err("claude reported a failed result".into()),
        None => Err("claude produced no result event (killed or crashed)".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::user_env::SeedEntry;

    #[test]
    fn manifest_lists_files_and_dirs() {
        let entries = vec![
            SeedEntry { path: "README.md".into(), dir: false, content: "# Hi\nLine 2".into() },
            SeedEntry { path: "notes".into(), dir: true, content: String::new() },
        ];
        let m = render_manifest(&entries);
        assert!(m.contains("FILE README.md"));
        assert!(m.contains("# Hi"));
        assert!(m.contains("DIR  notes"));
    }

    #[test]
    fn manifest_empty_template() {
        assert!(render_manifest(&[]).contains("empty template"));
    }

    #[test]
    fn prompt_embeds_instructions_and_manifest() {
        let p = migration_prompt("rename x to y", "- FILE y\n");
        assert!(p.contains("rename x to y"));
        assert!(p.contains("- FILE y"));
        assert!(p.contains(".llm-chat/")); // protects the stamp dir
    }

    #[test]
    fn result_ok_classifies_lines() {
        assert_eq!(migration_result_ok(r#"{"type":"result","subtype":"success"}"#), Some(true));
        assert_eq!(migration_result_ok(r#"{"type":"result","is_error":false}"#), Some(true));
        assert_eq!(migration_result_ok(r#"{"type":"result","subtype":"error_max_turns","is_error":true}"#), Some(false));
        assert_eq!(migration_result_ok(r#"{"type":"assistant"}"#), None);
        assert_eq!(migration_result_ok("not json"), None);
    }
}
