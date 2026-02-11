use ratatui::prelude::*;
use std::sync::LazyLock;
use syntect::highlighting::{Theme, ThemeSet};
use syntect::parsing::SyntaxSet;

static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_newlines);
static THEME: LazyLock<Theme> = LazyLock::new(|| {
    let ts = ThemeSet::load_defaults();
    ts.themes["base16-eighties.dark"].clone()
});

const CODE_BG: Color = Color::Rgb(30, 30, 30);

/// Highlight a code block with syntax coloring.
///
/// Returns styled lines with a dark background. If the language is not
/// recognized, falls back to plain dimmed monospace text.
pub fn highlight_code(code: &str, language: &str) -> Vec<Line<'static>> {
    let syntax = SYNTAX_SET
        .find_syntax_by_token(language)
        .or_else(|| SYNTAX_SET.find_syntax_by_extension(language));

    match syntax {
        Some(syntax) => highlight_with_syntect(code, syntax),
        None => plain_code_lines(code),
    }
}

fn highlight_with_syntect(
    code: &str,
    syntax: &syntect::parsing::SyntaxReference,
) -> Vec<Line<'static>> {
    use syntect::easy::HighlightLines;

    let mut h = HighlightLines::new(syntax, &THEME);
    let mut lines = Vec::new();

    for line_text in code.lines() {
        let regions = h.highlight_line(line_text, &SYNTAX_SET).unwrap();

        let mut spans: Vec<Span<'static>> = Vec::new();
        spans.push(Span::styled("    ", Style::default().bg(CODE_BG)));

        for (style, text) in regions {
            let fg = Color::Rgb(style.foreground.r, style.foreground.g, style.foreground.b);
            spans.push(Span::styled(
                text.to_string(),
                Style::default().fg(fg).bg(CODE_BG),
            ));
        }

        lines.push(Line::from(spans));
    }

    lines
}

fn plain_code_lines(code: &str) -> Vec<Line<'static>> {
    let style = Style::default().fg(Color::Gray).bg(CODE_BG);
    code.lines()
        .map(|line_text| Line::from(Span::styled(format!("    {}", line_text), style)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_language_returns_highlighted_lines() {
        let code = "let x = 42;";
        let lines = highlight_code(code, "rust");
        assert!(!lines.is_empty());
        // Should have at least one span with non-default foreground (syntax coloring)
        let has_colored_span = lines[0].spans.iter().any(|s| {
            matches!(s.style.fg, Some(Color::Rgb(..)))
        });
        assert!(has_colored_span, "Expected syntax-highlighted spans");
    }

    #[test]
    fn unknown_language_returns_plain_lines() {
        let code = "some text here";
        let lines = highlight_code(code, "notareallanguage");
        assert_eq!(lines.len(), 1);
        let text: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("some text here"));
    }

    #[test]
    fn empty_code_returns_empty() {
        let lines = highlight_code("", "rust");
        // Empty string has no lines when split by lines()
        assert!(lines.is_empty() || lines.len() == 1);
    }

    #[test]
    fn multiline_code_returns_multiple_lines() {
        let code = "fn main() {\n    println!(\"hello\");\n}";
        let lines = highlight_code(code, "rust");
        assert_eq!(lines.len(), 3);
    }
}
