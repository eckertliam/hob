//! Syntax highlighting for code blocks in assistant output.
//!
//! Uses syntect to highlight fenced code blocks before rendering
//! in the TUI.

use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use syntect::easy::HighlightLines;
use syntect::highlighting::{ThemeSet, Style as SynStyle};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

use std::sync::LazyLock;

static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_newlines);
static THEME_SET: LazyLock<ThemeSet> = LazyLock::new(ThemeSet::load_defaults);

/// Convert a syntect color to a ratatui color.
fn syn_to_ratatui(c: syntect::highlighting::Color) -> Color {
    Color::Rgb(c.r, c.g, c.b)
}

/// Highlight a code block and return ratatui Lines.
pub fn highlight_code(code: &str, lang: &str) -> Vec<Line<'static>> {
    let syntax = SYNTAX_SET
        .find_syntax_by_token(lang)
        .or_else(|| SYNTAX_SET.find_syntax_by_extension(lang))
        .unwrap_or_else(|| SYNTAX_SET.find_syntax_plain_text());

    let theme = &THEME_SET.themes["base16-ocean.dark"];
    let mut h = HighlightLines::new(syntax, theme);

    LinesWithEndings::from(code)
        .map(|line| {
            let spans: Vec<Span<'static>> = match h.highlight_line(line, &SYNTAX_SET) {
                Ok(ranges) => ranges
                    .iter()
                    .map(|(style, text)| {
                        Span::styled(
                            text.to_string(),
                            Style::default().fg(syn_to_ratatui(style.foreground)),
                        )
                    })
                    .collect(),
                Err(_) => vec![Span::raw(line.to_string())],
            };
            Line::from(spans)
        })
        .collect()
}

/// Process assistant text: find fenced code blocks and highlight them.
/// Returns a mix of plain text lines and highlighted code lines.
pub fn render_with_highlights(text: &str) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let mut in_code_block = false;
    let mut code_lang = String::new();
    let mut code_buf = String::new();

    for line in text.lines() {
        if line.starts_with("```") && !in_code_block {
            in_code_block = true;
            code_lang = line.trim_start_matches('`').trim().to_string();
            code_buf.clear();
        } else if line.starts_with("```") && in_code_block {
            // End of code block — highlight accumulated code
            let highlighted = highlight_code(&code_buf, &code_lang);
            // Add a dim border line before
            lines.push(Line::from(Span::styled(
                format!("  ╭─ {}", if code_lang.is_empty() { "code" } else { &code_lang }),
                Style::default().fg(Color::DarkGray),
            )));
            for hl in highlighted {
                let mut prefixed: Vec<Span<'static>> = vec![Span::styled(
                    "  │ ".to_string(),
                    Style::default().fg(Color::DarkGray),
                )];
                prefixed.extend(hl.spans);
                lines.push(Line::from(prefixed));
            }
            lines.push(Line::from(Span::styled(
                "  ╰─".to_string(),
                Style::default().fg(Color::DarkGray),
            )));
            in_code_block = false;
            code_buf.clear();
        } else if in_code_block {
            code_buf.push_str(line);
            code_buf.push('\n');
        } else {
            lines.push(Line::from(line.to_string()));
        }
    }

    // If we ended mid-code-block, dump remaining as plain text
    if in_code_block {
        for l in code_buf.lines() {
            lines.push(Line::from(l.to_string()));
        }
    }

    if lines.is_empty() {
        lines.push(Line::from(text.to_string()));
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_highlight_code_returns_lines() {
        let lines = highlight_code("fn main() {}\n", "rs");
        assert!(!lines.is_empty());
    }

    #[test]
    fn test_highlight_unknown_lang() {
        let lines = highlight_code("hello world\n", "zzz_nonexistent");
        assert!(!lines.is_empty());
    }

    #[test]
    fn test_render_with_highlights_plain() {
        let lines = render_with_highlights("hello world");
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn test_render_with_highlights_code_block() {
        let text = "some text\n```rust\nfn main() {}\n```\nmore text";
        let lines = render_with_highlights(text);
        // Should have: "some text", code border + highlighted line + border, "more text"
        assert!(lines.len() >= 5);
    }

    #[test]
    fn test_render_with_highlights_no_lang() {
        let text = "```\nplain code\n```";
        let lines = render_with_highlights(text);
        assert!(lines.len() >= 3);
    }
}
