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

use syntect::highlighting::{Theme, ThemeSet};
use syntect::parsing::SyntaxSet;
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
                // Styled: termimad for prose, syntect for fenced code blocks
                // (so code is syntax-highlighted like the Python client's rich).
                print_styled(text);
            } else {
                MadSkin::no_style().print_text(text);
            }
        }
    }
}

/// Render `md` to a color terminal: prose via termimad, fenced code blocks
/// syntax-highlighted via syntect. Code blocks are located with pulldown-cmark
/// (the parser's byte offsets), not by hand-scanning fences.
fn print_styled(md: &str) {
    use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

    let skin = MadSkin::default();
    let ss = SyntaxSet::load_defaults_newlines();
    let ts = ThemeSet::load_defaults();
    let theme = match ts
        .themes
        .get("base16-ocean.dark")
        .or_else(|| ts.themes.values().next())
    {
        Some(t) => t,
        None => {
            skin.print_text(md);
            return;
        }
    };

    let mut last = 0usize;
    let mut in_code = false;
    let mut lang = String::new();
    let mut code = String::new();

    for (ev, range) in Parser::new_ext(md, Options::all()).into_offset_iter() {
        match ev {
            Event::Start(Tag::CodeBlock(kind)) => {
                // Render the markdown between the previous block and this one.
                if range.start > last {
                    let gap = &md[last..range.start];
                    if !gap.trim().is_empty() {
                        skin.print_text(gap);
                    }
                }
                in_code = true;
                code.clear();
                lang = match kind {
                    CodeBlockKind::Fenced(l) => l.to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
            }
            Event::Text(t) if in_code => code.push_str(&t),
            Event::End(TagEnd::CodeBlock) => {
                print_code(&ss, theme, &lang, &code);
                in_code = false;
                last = range.end;
            }
            _ => {}
        }
    }
    if last < md.len() {
        let tail = &md[last..];
        if !tail.trim().is_empty() {
            skin.print_text(tail);
        }
    }
}

fn print_code(ss: &SyntaxSet, theme: &Theme, lang: &str, code: &str) {
    use syntect::easy::HighlightLines;
    use syntect::util::{as_24_bit_terminal_escaped, LinesWithEndings};

    // Match the fence's info string to a syntax by token (e.g. "rust", "py")
    // or extension; fall back to plain text for unknown/absent languages.
    let syntax = ss
        .find_syntax_by_token(lang)
        .or_else(|| ss.find_syntax_by_extension(lang))
        .unwrap_or_else(|| ss.find_syntax_plain_text());
    let mut h = HighlightLines::new(syntax, theme);
    let mut out = std::io::stdout();
    for line in LinesWithEndings::from(code) {
        let ranges = h.highlight_line(line, ss).unwrap_or_default();
        let _ = write!(out, "{}", as_24_bit_terminal_escaped(&ranges[..], false));
    }
    let _ = write!(out, "\x1b[0m"); // reset so following prose isn't tinted
    let _ = writeln!(out);
    let _ = out.flush();
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

    #[test]
    fn code_highlighting_produces_multiple_token_colors() {
        // Proves the syntect path (used by styled `auto`) syntax-highlights —
        // the capability rich provides in the Python client.
        use syntect::easy::HighlightLines;
        use syntect::util::as_24_bit_terminal_escaped;
        let ss = SyntaxSet::load_defaults_newlines();
        let ts = ThemeSet::load_defaults();
        let theme = &ts.themes["base16-ocean.dark"];
        let syntax = ss.find_syntax_by_token("rust").expect("bundled rust syntax");
        let mut h = HighlightLines::new(syntax, theme);
        let ranges = h.highlight_line("fn main() { let x = 1; }\n", &ss).unwrap();
        let out = as_24_bit_terminal_escaped(&ranges[..], false);
        assert!(out.contains('\u{1b}'), "highlighted code must contain ANSI color");
        // ≥2 distinct truecolor codes → tokens colored differently (keyword vs ident).
        assert!(
            out.matches("\u{1b}[38;2;").count() >= 2,
            "expected multiple token colors, got: {out:?}"
        );
    }
}
