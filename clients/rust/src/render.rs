//! Render claude's markdown answers for terminal display. Port of `render.py`.
//!
//! DISPLAY ONLY — claude's exact markdown (the source of truth) is unchanged;
//! this only controls how it is printed, like a browser rendering markdown to
//! HTML but to ANSI/plain text. We hand the whole markdown to a real renderer
//! (termimad); we never strip characters or guess structure.
//!
//! Modes:
//!   auto  - styled when the terminal supports it; falls back to plain text when
//!           piped, NO_COLOR is set, or TERM=dumb. ANSI is text, not a GUI, so
//!           this works on a headless Linux CLI / over SSH.
//!   plain - markdown obeyed but ZERO ANSI (termimad no_style skin).
//!   raw   - the literal markdown exactly as received.

use std::io::{IsTerminal, Write};

use termimad::MadSkin;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderMode {
    Auto,
    Plain,
    Raw,
}

/// Map the --plain / --raw flags to a mode (raw wins).
pub fn resolve_mode(plain: bool, raw: bool) -> RenderMode {
    if raw {
        RenderMode::Raw
    } else if plain {
        RenderMode::Plain
    } else {
        RenderMode::Auto
    }
}

/// True when styled ANSI output is appropriate (replicates rich's auto-detect).
fn color_capable() -> bool {
    std::io::stdout().is_terminal()
        && std::env::var_os("NO_COLOR").is_none()
        && std::env::var("TERM").map(|t| t != "dumb").unwrap_or(true)
}

fn write_raw(text: &str) {
    let mut out = std::io::stdout();
    let _ = out.write_all(text.as_bytes());
    if !text.ends_with('\n') {
        let _ = out.write_all(b"\n");
    }
    let _ = out.flush();
}

/// Print `text` (claude's markdown) to stdout per `mode`.
pub fn render_markdown(text: &str, mode: RenderMode) {
    match mode {
        RenderMode::Raw => write_raw(text),
        RenderMode::Plain => MadSkin::no_style().print_text(text),
        RenderMode::Auto => {
            if color_capable() {
                MadSkin::default().print_text(text);
            } else {
                MadSkin::no_style().print_text(text);
            }
        }
    }
}

/// Render to a string (used by tests). Auto is treated as plain (non-tty).
#[cfg(test)]
pub fn render_to_string(text: &str, mode: RenderMode) -> String {
    match mode {
        RenderMode::Raw => {
            if text.ends_with('\n') {
                text.to_string()
            } else {
                format!("{text}\n")
            }
        }
        RenderMode::Plain | RenderMode::Auto => {
            format!("{}", MadSkin::no_style().text(text, Some(80)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "## Heading\n\n- **bold** item\n- second\n\nplain line\n";

    #[test]
    fn resolve_mode_maps_flags() {
        assert_eq!(resolve_mode(false, false), RenderMode::Auto);
        assert_eq!(resolve_mode(true, false), RenderMode::Plain);
        assert_eq!(resolve_mode(false, true), RenderMode::Raw);
        assert_eq!(resolve_mode(true, true), RenderMode::Raw); // raw wins
    }

    #[test]
    fn raw_is_verbatim() {
        assert_eq!(render_to_string(SAMPLE, RenderMode::Raw), SAMPLE);
    }

    #[test]
    fn raw_adds_trailing_newline_when_missing() {
        assert_eq!(render_to_string("no newline", RenderMode::Raw), "no newline\n");
    }

    #[test]
    fn plain_has_no_ansi_and_obeys_markdown() {
        let out = render_to_string(SAMPLE, RenderMode::Plain);
        assert!(!out.contains('\u{1b}'), "plain output must have no ANSI escapes");
        assert!(!out.contains("##"), "heading marker should be rendered away");
        assert!(!out.contains("**"), "bold markers should be rendered away");
        assert!(out.contains("Heading") && out.contains("bold"));
    }

    #[test]
    fn auto_to_non_tty_is_plain() {
        let out = render_to_string(SAMPLE, RenderMode::Auto);
        assert!(!out.contains('\u{1b}'));
        assert!(!out.contains("##"));
    }
}
