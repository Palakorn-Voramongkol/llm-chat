//! Interactive REPL: a colored, multi-turn chat over a persistent session.
//! Port of `repl.py`.

use std::io::{IsTerminal, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::cli::identity_from_token;
use crate::errors::Error;
use crate::protocol::ChatClient;
use crate::render::{render_markdown, RenderMode};

const HELP: &str = "commands:\n\
  /help            show this help\n\
  /history         print this session's Q&A so far\n\
  /session         show the backend session id\n\
  /status          show your identity + client/connection status\n\
  /render MODE     switch markdown display: auto | plain | raw\n\
  /reset           drop the session and start a fresh one (clears claude context)\n\
  /multi           enter a multi-line message (end with '.')\n\
  /quit, /exit     leave\n\
anything else is sent to claude on the same (context-preserving) session.";

/// Static context for the REPL's `/status` block. The dynamic bits (identity,
/// connection state, message count) are read live when `/status` runs.
pub struct ReplCtx {
    pub kind: &'static str,    // "rust"
    pub version: &'static str, // crate version
    pub auth_label: String,    // "human (browser login)" | "machine (kabytech key)"
    pub issuer: String,
    pub project: String,
    pub manager_url: String,
}

const STATUS_RULE: &str = "─────────────────────────────────────────────";

/// PURE: render the `/status` block (`roles` already sorted/de-duped). The
/// Python client emits the identical layout — keep them in sync.
#[allow(clippy::too_many_arguments)]
fn format_status(
    ctx: &ReplCtx,
    who: &str,
    sub: &str,
    roles: &[String],
    connected: bool,
    session_id: Option<&str>,
    msgs: usize,
    render: &str,
    timeout_s: u64,
) -> String {
    let roles_str = if roles.is_empty() { "—".to_string() } else { roles.join(", ") };
    let conn = if connected { "connected" } else { "disconnected" };
    let sid = session_id.unwrap_or("—");
    format!(
        "─ status ───────────────────────────────────\n\
         \x20client    llm-chat · {kind} · v{version}\n\
         \x20auth      {auth}\n\
         \x20user      {who}\n\
         \x20  sub     {sub}\n\
         \x20  roles   {roles}\n\
         \x20manager   {manager} · {conn}\n\
         \x20session   {sid} · {msgs} msgs this session\n\
         \x20issuer    {issuer}\n\
         \x20project   {project}\n\
         \x20display   render={render} · timeout={timeout}s\n\
         {rule}",
        kind = ctx.kind, version = ctx.version, auth = ctx.auth_label,
        who = who, sub = sub, roles = roles_str, manager = ctx.manager_url,
        conn = conn, sid = sid, msgs = msgs, issuer = ctx.issuer,
        project = ctx.project, render = render, timeout = timeout_s, rule = STATUS_RULE,
    )
}

/// Minimal ANSI styling, disabled when stdout isn't a TTY or NO_COLOR is set.
struct Ansi {
    enabled: bool,
}
impl Ansi {
    fn new() -> Self {
        Ansi {
            enabled: std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none(),
        }
    }
    fn wrap(&self, code: &str, s: &str) -> String {
        if self.enabled {
            format!("\x1b[{code}m{s}\x1b[0m")
        } else {
            s.to_string()
        }
    }
    fn you(&self, s: &str) -> String {
        self.wrap("1;36", s) // bold cyan
    }
    fn claude(&self, s: &str) -> String {
        self.wrap("1;33", s) // bold yellow
    }
    fn dim(&self, s: &str) -> String {
        self.wrap("2", s)
    }
    fn err(&self, s: &str) -> String {
        self.wrap("1;31", s)
    }
}

fn parse_mode(s: &str) -> Option<RenderMode> {
    match s {
        "auto" => Some(RenderMode::Auto),
        "plain" => Some(RenderMode::Plain),
        "raw" => Some(RenderMode::Raw),
        _ => None,
    }
}

fn mode_name(m: RenderMode) -> &'static str {
    match m {
        RenderMode::Auto => "auto",
        RenderMode::Plain => "plain",
        RenderMode::Raw => "raw",
    }
}

/// Read one line, printing `prompt` first. None on EOF/error (Ctrl-D).
async fn read_line(prompt: String) -> Option<String> {
    print!("{prompt}");
    let _ = std::io::stdout().flush();
    tokio::task::spawn_blocking(|| {
        let mut s = String::new();
        match std::io::stdin().read_line(&mut s) {
            Ok(0) => None, // EOF
            Ok(_) => Some(s.trim_end_matches(['\n', '\r']).to_string()),
            Err(_) => None,
        }
    })
    .await
    .ok()
    .flatten()
}

async fn read_multiline(c: &Ansi) -> Option<String> {
    println!("{}", c.dim("(multi-line: end with a single '.' on its own line)"));
    let mut lines: Vec<String> = Vec::new();
    loop {
        let line = read_line(c.dim("… ")).await?;
        if line.trim() == "." {
            break;
        }
        lines.push(line);
    }
    Some(lines.join("\n"))
}

fn spinner(stop: Arc<AtomicBool>, label: String) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        if !std::io::stdout().is_terminal() {
            while !stop.load(Ordering::SeqCst) {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            return;
        }
        let frames = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
        let t0 = Instant::now();
        let mut i = 0usize;
        while !stop.load(Ordering::SeqCst) {
            print!(
                "\r{label} {} thinking… ({:.0}s)   ",
                frames[i % frames.len()],
                t0.elapsed().as_secs_f64()
            );
            let _ = std::io::stdout().flush();
            i += 1;
            tokio::time::sleep(Duration::from_millis(120)).await;
        }
        print!("\r{}\r", " ".repeat(48));
        let _ = std::io::stdout().flush();
    })
}

fn print_answer(c: &Ansi, text: &str, mode: RenderMode, latency_s: Option<f64>) {
    // Label, then render the markdown body as its own block.
    println!("{}", c.claude("Claude:"));
    render_markdown(text, mode);
    if let Some(s) = latency_s {
        println!("{}", c.dim(&format!("({s:.1}s)")));
    }
    println!();
}

/// Run the interactive loop until the user quits. Returns an exit code.
pub async fn run_repl(client: &mut ChatClient, ctx: &ReplCtx, timeout: Duration, mut render_mode: RenderMode) -> i32 {
    let c = Ansi::new();
    if let Err(e) = client.connect().await {
        eprintln!("{}", c.err(&format!("cannot connect: {e}")));
        return 2;
    }
    println!(
        "{}",
        c.dim(&format!(
            "connected — session {}",
            client.session_id.as_deref().unwrap_or("?")
        ))
    );
    println!(
        "{}",
        c.dim("type a message, /help for commands. first reply includes warm-up.\n")
    );
    let mut history: Vec<(String, String)> = Vec::new();

    loop {
        let user = match read_line(c.you("You: ")).await {
            Some(u) => u,
            None => break,
        };
        let mut user = user.trim().to_string();
        if user.is_empty() {
            continue;
        }

        if user == "/quit" || user == "/exit" {
            break;
        }
        if user == "/help" {
            println!("{}\n", c.dim(HELP));
            continue;
        }
        if user == "/session" {
            println!(
                "{}\n",
                c.dim(&format!("session {}", client.session_id.as_deref().unwrap_or("?")))
            );
            continue;
        }
        if user == "/status" {
            // Re-mint/refresh the token and decode its identity live.
            let (who, sub, roles, note) = match client.current_token().await {
                Ok(tok) => {
                    let (w, s, r) = identity_from_token(&tok);
                    (w, s, r, None)
                }
                Err(e) => (
                    "(could not read token)".to_string(),
                    "—".to_string(),
                    Vec::new(),
                    Some(format!("{e}")),
                ),
            };
            let block = format_status(
                ctx,
                &who,
                &sub,
                &roles,
                client.connected(),
                client.session_id.as_deref(),
                history.len(),
                mode_name(render_mode),
                timeout.as_secs(),
            );
            println!("{}", c.dim(&block));
            if let Some(n) = note {
                println!("{}", c.err(&format!("  token error: {n}")));
            }
            println!();
            continue;
        }
        if user == "/history" {
            if history.is_empty() {
                println!("{}\n", c.dim("(no messages yet)"));
            }
            for (i, (q, a)) in history.iter().enumerate() {
                println!("{} {q}", c.you(&format!("You[{}]:", i + 1)));
                println!("{}", c.claude(&format!("Claude[{}]:", i + 1)));
                render_markdown(a, render_mode);
                println!();
            }
            continue;
        }
        if user.starts_with("/render") {
            let parts: Vec<&str> = user.split_whitespace().collect();
            match parts.get(1).and_then(|p| parse_mode(p)) {
                Some(m) if parts.len() == 2 => {
                    render_mode = m;
                    println!("{}\n", c.dim(&format!("render mode: {}", mode_name(render_mode))));
                }
                _ => println!(
                    "{}\n",
                    c.dim(&format!("usage: /render auto|plain|raw (current: {})", mode_name(render_mode)))
                ),
            }
            continue;
        }
        if user == "/reset" {
            client.close().await;
            if let Err(e) = client.connect().await {
                eprintln!("{}", c.err(&format!("reconnect failed: {e}")));
                return 2;
            }
            history.clear();
            println!(
                "{}\n",
                c.dim(&format!(
                    "fresh session — {}",
                    client.session_id.as_deref().unwrap_or("?")
                ))
            );
            continue;
        }
        if user == "/multi" {
            match read_multiline(&c).await {
                Some(m) if !m.trim().is_empty() => user = m,
                _ => continue,
            }
        }

        let stop = Arc::new(AtomicBool::new(false));
        let spin = spinner(stop.clone(), c.claude("Claude:"));
        let res = client.ask(&user, timeout).await;
        stop.store(true, Ordering::SeqCst);
        let _ = spin.await;

        match res {
            Ok(answer) => {
                history.push((user.clone(), answer.text.clone()));
                print_answer(&c, &answer.text, render_mode, answer.latency_s());
            }
            Err(Error::AnswerTimeout(_)) => {
                println!(
                    "{}\n",
                    c.err(&format!("Claude: [no answer within {}s]", timeout.as_secs_f64()))
                );
            }
            Err(Error::Protocol(e)) => {
                println!("{}\n", c.err(&format!("Claude: [error] {e}")));
            }
            Err(Error::ManagerUnavailable(e)) => {
                eprintln!("{}", c.err(&format!("[connection lost] {e}")));
                return 2;
            }
            Err(e) => {
                println!("{}\n", c.err(&format!("Claude: [error] {e}")));
            }
        }
    }

    println!("{}", c.dim("bye"));
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> ReplCtx {
        ReplCtx {
            kind: "rust",
            version: "1.0.0",
            auth_label: "machine (kabytech key)".to_string(),
            issuer: "http://iss:8080".to_string(),
            project: "P123".to_string(),
            manager_url: "ws://m:7777/chat".to_string(),
        }
    }

    #[test]
    fn format_status_includes_all_fields() {
        let roles = vec!["chat.admin".to_string(), "chat.user".to_string()];
        let s = format_status(&ctx(), "admin@example.com", "U9", &roles, true, Some("s1"), 2, "auto", 120);
        assert!(s.contains("llm-chat · rust · v1.0.0"));
        assert!(s.contains("machine (kabytech key)"));
        assert!(s.contains("user      admin@example.com"));
        assert!(s.contains("sub     U9"));
        assert!(s.contains("roles   chat.admin, chat.user"));
        assert!(s.contains("ws://m:7777/chat · connected"));
        assert!(s.contains("session   s1 · 2 msgs this session"));
        assert!(s.contains("issuer    http://iss:8080"));
        assert!(s.contains("project   P123"));
        assert!(s.contains("render=auto · timeout=120s"));
    }

    #[test]
    fn format_status_handles_empty_roles_and_no_session() {
        let s = format_status(&ctx(), "who", "sub", &[], false, None, 0, "raw", 60);
        assert!(s.contains("roles   —"));
        assert!(s.contains("session   — · 0 msgs"));
        assert!(s.contains("ws://m:7777/chat · disconnected"));
        assert!(s.contains("render=raw · timeout=60s"));
    }
}
