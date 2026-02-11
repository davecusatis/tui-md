use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use ratatui::prelude::*;

use crate::syntax;

const CODE_BG: Color = Color::Rgb(30, 30, 30);

/// Core markdown renderer. Walks pulldown-cmark events and builds `Vec<Line>`.
pub struct Renderer {
    lines: Vec<Line<'static>>,
    current_spans: Vec<Span<'static>>,
    style_stack: Vec<Style>,

    // Block state
    in_code_block: bool,
    code_block_lang: String,
    code_block_buf: String,
    list_stack: Vec<ListKind>,
    blockquote_depth: usize,

    // Table state
    in_table: bool,
    in_table_head: bool,
    table_row_cells: Vec<Vec<Span<'static>>>,
    current_cell_spans: Vec<Span<'static>>,
    table_col_count: usize,
    table_header_rows: Vec<Vec<Vec<Span<'static>>>>,
    table_body_rows: Vec<Vec<Vec<Span<'static>>>>,

    // Inline state
    link_url: Option<String>,
}

#[derive(Clone)]
enum ListKind {
    Unordered(usize), // nesting depth (0-based)
    Ordered(u64),      // current item number
}

const BULLETS: &[&str] = &["• ", "◦ ", "▪ "];

impl Renderer {
    pub fn new() -> Self {
        Self {
            lines: Vec::new(),
            current_spans: Vec::new(),
            style_stack: vec![Style::default()],
            in_code_block: false,
            code_block_lang: String::new(),
            code_block_buf: String::new(),
            list_stack: Vec::new(),
            blockquote_depth: 0,
            in_table: false,
            in_table_head: false,
            table_row_cells: Vec::new(),
            current_cell_spans: Vec::new(),
            table_col_count: 0,
            table_header_rows: Vec::new(),
            table_body_rows: Vec::new(),
            link_url: None,
        }
    }

    /// Render markdown input into styled ratatui lines.
    pub fn render(mut self, input: &str) -> Vec<Line<'static>> {
        if input.trim().is_empty() {
            return vec![Line::from("")];
        }

        let mut options = Options::empty();
        options.insert(Options::ENABLE_STRIKETHROUGH);
        options.insert(Options::ENABLE_TASKLISTS);
        options.insert(Options::ENABLE_TABLES);
        options.insert(Options::ENABLE_SMART_PUNCTUATION);

        let events: Vec<Event<'_>> = Parser::new_ext(input, options).collect();
        for event in events {
            self.process_event(event);
        }

        self.flush_spans();

        // Remove trailing blank lines
        while self.lines.last().is_some_and(|l| line_is_blank(l)) {
            self.lines.pop();
        }

        if self.lines.is_empty() {
            self.lines.push(Line::from(""));
        }

        self.lines
    }

    fn process_event(&mut self, event: Event<'_>) {
        match event {
            Event::Start(tag) => self.start_tag(tag),
            Event::End(tag_end) => self.end_tag(tag_end),
            Event::Text(text) => self.handle_text(&text),
            Event::Code(code) => self.handle_inline_code(&code),
            Event::SoftBreak | Event::HardBreak => self.handle_break(),
            Event::TaskListMarker(checked) => self.handle_task_marker(checked),
            Event::Rule => self.handle_rule(),
            _ => {}
        }
    }

    // ── Tag start handlers ──

    fn start_tag(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Heading { level, .. } => self.start_heading(level),
            Tag::Paragraph => self.start_paragraph(),
            Tag::Strong => self.push_modifier(Modifier::BOLD),
            Tag::Emphasis => self.push_modifier(Modifier::ITALIC),
            Tag::Strikethrough => self.push_modifier(Modifier::CROSSED_OUT),
            Tag::CodeBlock(kind) => self.start_code_block(kind),
            Tag::BlockQuote(_) => self.start_blockquote(),
            Tag::List(first_item) => self.start_list(first_item),
            Tag::Item => self.start_item(),
            Tag::Link { dest_url, .. } => self.start_link(dest_url.to_string()),
            Tag::Image { dest_url, .. } => self.start_image(dest_url.to_string()),
            Tag::Table(_) => self.start_table(),
            Tag::TableHead => self.in_table_head = true,
            Tag::TableRow => {
                self.table_row_cells.clear();
            }
            Tag::TableCell => {
                self.current_cell_spans.clear();
            }
            _ => {}
        }
    }

    fn start_paragraph(&mut self) {
        // Add blockquote prefix at the start of paragraphs inside blockquotes
        if self.blockquote_depth > 0 {
            let mut spans = Vec::new();
            self.add_blockquote_prefix(&mut spans);
            self.current_spans = spans;
        }
    }

    fn start_heading(&mut self, level: pulldown_cmark::HeadingLevel) {
        let style = match level {
            pulldown_cmark::HeadingLevel::H1 => {
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
            }
            pulldown_cmark::HeadingLevel::H2 => {
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD)
            }
            _ => Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        };
        self.style_stack.push(style);
    }

    fn start_code_block(&mut self, kind: CodeBlockKind<'_>) {
        self.in_code_block = true;
        self.code_block_buf.clear();
        self.code_block_lang = match kind {
            CodeBlockKind::Fenced(lang) => lang.to_string(),
            CodeBlockKind::Indented => String::new(),
        };
    }

    fn start_blockquote(&mut self) {
        self.blockquote_depth += 1;
    }

    fn start_list(&mut self, first_item: Option<u64>) {
        let depth = self.list_stack.len();
        match first_item {
            Some(start) => self.list_stack.push(ListKind::Ordered(start)),
            None => self.list_stack.push(ListKind::Unordered(depth)),
        }
    }

    fn start_item(&mut self) {
        // Flush any pending spans from a previous item (important for nested lists)
        self.flush_spans();

        let indent_level = self.list_stack.len().saturating_sub(1);
        let indent = "  ".repeat(indent_level);

        let marker_span = match self.list_stack.last() {
            Some(ListKind::Unordered(depth)) => {
                let bullet = BULLETS[*depth % BULLETS.len()];
                Span::styled(
                    format!("{}{}", indent, bullet),
                    Style::default().fg(Color::Cyan),
                )
            }
            Some(ListKind::Ordered(n)) => {
                let span = Span::styled(
                    format!("{}{}. ", indent, n),
                    Style::default().fg(Color::Cyan),
                );
                // Increment counter for next item
                if let Some(ListKind::Ordered(num)) = self.list_stack.last_mut() {
                    *num += 1;
                }
                span
            }
            None => Span::raw("  "),
        };

        let mut spans = Vec::new();
        self.add_blockquote_prefix(&mut spans);
        spans.push(marker_span);
        self.current_spans = spans;
    }

    fn start_link(&mut self, url: String) {
        self.link_url = Some(url);
        let base = *self.style_stack.last().unwrap_or(&Style::default());
        self.style_stack
            .push(base.fg(Color::Blue).add_modifier(Modifier::UNDERLINED));
    }

    fn start_image(&mut self, _url: String) {
        // We'll handle the alt text in handle_text when we see it
        self.current_spans.push(Span::styled(
            "[img: ",
            Style::default().fg(Color::DarkGray),
        ));
    }

    fn start_table(&mut self) {
        self.in_table = true;
        self.table_col_count = 0;
        self.table_header_rows.clear();
        self.table_body_rows.clear();
    }

    // ── Tag end handlers ──

    fn end_tag(&mut self, tag_end: TagEnd) {
        match tag_end {
            TagEnd::Heading(_) => self.end_heading(),
            TagEnd::Paragraph => self.end_paragraph(),
            TagEnd::Strong | TagEnd::Emphasis | TagEnd::Strikethrough => {
                self.style_stack.pop();
            }
            TagEnd::CodeBlock => self.end_code_block(),
            TagEnd::BlockQuote(_) => self.end_blockquote(),
            TagEnd::List(_) => self.end_list(),
            TagEnd::Item => self.end_item(),
            TagEnd::Link => self.end_link(),
            TagEnd::Image => self.end_image(),
            TagEnd::Table => self.end_table(),
            TagEnd::TableHead => {
                self.table_header_rows
                    .push(self.table_row_cells.drain(..).collect());
                self.in_table_head = false;
            }
            TagEnd::TableRow => {
                self.table_body_rows
                    .push(self.table_row_cells.drain(..).collect());
            }
            TagEnd::TableCell => {
                self.table_row_cells
                    .push(self.current_cell_spans.drain(..).collect());
            }
            _ => {}
        }
    }

    fn end_heading(&mut self) {
        self.style_stack.pop();
        self.flush_spans();
        self.push_blank_line();
    }

    fn end_paragraph(&mut self) {
        self.flush_spans();
        self.push_blank_line();
    }

    fn end_code_block(&mut self) {
        self.in_code_block = false;
        let code = std::mem::take(&mut self.code_block_buf);
        let lang = std::mem::take(&mut self.code_block_lang);

        // Trim trailing newline from code
        let code = code.trim_end_matches('\n');

        let highlighted = if lang.is_empty() {
            syntax::highlight_code(code, "")
        } else {
            syntax::highlight_code(code, &lang)
        };

        // Add language label line if specified
        if !lang.is_empty() {
            self.lines.push(Line::from(Span::styled(
                format!("    {}", lang),
                Style::default().fg(Color::DarkGray).bg(CODE_BG),
            )));
        }

        for line in highlighted {
            self.lines.push(line);
        }

        self.push_blank_line();
    }

    fn end_blockquote(&mut self) {
        self.blockquote_depth = self.blockquote_depth.saturating_sub(1);
        if self.blockquote_depth == 0 {
            self.push_blank_line();
        }
    }

    fn end_list(&mut self) {
        self.list_stack.pop();
        if self.list_stack.is_empty() {
            self.push_blank_line();
        }
    }

    fn end_item(&mut self) {
        self.flush_spans();
    }

    fn end_link(&mut self) {
        self.style_stack.pop();
        if let Some(url) = self.link_url.take() {
            self.current_spans.push(Span::styled(
                format!(" ({})", url),
                Style::default().fg(Color::DarkGray),
            ));
        }
    }

    fn end_image(&mut self) {
        self.current_spans.push(Span::styled(
            "]",
            Style::default().fg(Color::DarkGray),
        ));
    }

    fn end_table(&mut self) {
        self.in_table = false;

        // Take ownership to avoid borrow conflicts
        let header_rows = std::mem::take(&mut self.table_header_rows);
        let body_rows = std::mem::take(&mut self.table_body_rows);

        // Compute column widths from all rows
        let all_rows: Vec<&Vec<Vec<Span<'static>>>> =
            header_rows.iter().chain(body_rows.iter()).collect();

        if all_rows.is_empty() {
            return;
        }

        let col_count = all_rows.iter().map(|r| r.len()).max().unwrap_or(0);
        if col_count == 0 {
            return;
        }

        // Calculate max content width per column (minimum 3)
        let mut col_widths = vec![3usize; col_count];
        for row in &all_rows {
            for (i, cell_spans) in row.iter().enumerate() {
                let content_len: usize = cell_spans.iter().map(|s| s.content.len()).sum();
                col_widths[i] = col_widths[i].max(content_len + 2);
            }
        }

        let border_style = Style::default().fg(Color::DarkGray);

        // Top border
        self.lines.push(Line::from(Span::styled(
            build_table_border(&col_widths, '┌', '┬', '┐'),
            border_style,
        )));

        // Header rows
        for row in &header_rows {
            self.lines
                .push(build_table_row_line(row, &col_widths, true));
        }

        // Middle border
        self.lines.push(Line::from(Span::styled(
            build_table_border(&col_widths, '├', '┼', '┤'),
            border_style,
        )));

        // Body rows
        for row in &body_rows {
            self.lines
                .push(build_table_row_line(row, &col_widths, false));
        }

        // Bottom border
        self.lines.push(Line::from(Span::styled(
            build_table_border(&col_widths, '└', '┴', '┘'),
            border_style,
        )));

        self.table_col_count = 0;
        self.push_blank_line();
    }

    // ── Content handlers ──

    fn handle_text(&mut self, text: &str) {
        if self.in_code_block {
            self.code_block_buf.push_str(text);
            return;
        }

        if self.in_table {
            let style = if self.in_table_head {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                *self.style_stack.last().unwrap_or(&Style::default())
            };
            self.current_cell_spans
                .push(Span::styled(text.to_string(), style));
            return;
        }

        let style = *self.style_stack.last().unwrap_or(&Style::default());

        // For blockquotes, we need to handle newlines within the text
        if self.blockquote_depth > 0 {
            let bq_style = if style == Style::default() {
                Style::default().add_modifier(Modifier::ITALIC)
            } else {
                style
            };
            self.current_spans
                .push(Span::styled(text.to_string(), bq_style));
        } else {
            self.current_spans
                .push(Span::styled(text.to_string(), style));
        }
    }

    fn handle_inline_code(&mut self, code: &str) {
        if self.in_table {
            self.current_cell_spans.push(Span::styled(
                format!("`{}`", code),
                Style::default().fg(Color::Magenta),
            ));
            return;
        }
        self.current_spans.push(Span::styled(
            format!("`{}`", code),
            Style::default().fg(Color::Magenta),
        ));
    }

    fn handle_break(&mut self) {
        self.flush_spans();
    }

    fn handle_task_marker(&mut self, checked: bool) {
        let (marker, color) = if checked {
            ("  ☑ ", Color::Green)
        } else {
            ("  ☐ ", Color::Yellow)
        };

        // Replace the last list marker spans (the bullet) with the task marker
        // The start_item already added spans, so we clear and re-add with blockquote prefix
        self.current_spans.clear();
        let mut spans = Vec::new();
        self.add_blockquote_prefix(&mut spans);
        let indent_level = self.list_stack.len().saturating_sub(1);
        let indent = "  ".repeat(indent_level);
        spans.push(Span::styled(
            format!("{}{}", indent, marker),
            Style::default().fg(color),
        ));
        self.current_spans = spans;
    }

    fn handle_rule(&mut self) {
        self.flush_spans();
        self.lines.push(Line::from(Span::styled(
            "─".repeat(40),
            Style::default().fg(Color::DarkGray),
        )));
        self.push_blank_line();
    }

    // ── Helpers ──

    fn current_style(&self) -> Style {
        *self.style_stack.last().unwrap_or(&Style::default())
    }

    fn push_modifier(&mut self, modifier: Modifier) {
        let base = self.current_style();
        self.style_stack.push(base.add_modifier(modifier));
    }

    fn flush_spans(&mut self) {
        if !self.current_spans.is_empty() {
            let spans: Vec<Span<'static>> = self.current_spans.drain(..).collect();
            self.lines.push(Line::from(spans));
        }
    }

    fn push_blank_line(&mut self) {
        // Avoid double blank lines
        if !self.lines.last().is_some_and(|l| line_is_blank(l)) {
            self.lines.push(Line::from(""));
        }
    }

    fn add_blockquote_prefix(&self, spans: &mut Vec<Span<'static>>) {
        for _ in 0..self.blockquote_depth {
            spans.push(Span::styled(
                "▌ ",
                Style::default().fg(Color::DarkGray),
            ));
        }
    }

}

fn build_table_row_line(
    row: &[Vec<Span<'static>>],
    col_widths: &[usize],
    is_header: bool,
) -> Line<'static> {
    let border_style = Style::default().fg(Color::DarkGray);
    let mut spans: Vec<Span<'static>> = Vec::new();
    spans.push(Span::styled("│", border_style));

    for (i, width) in col_widths.iter().enumerate() {
        let cell_spans = row.get(i).cloned().unwrap_or_default();
        let content_len: usize = cell_spans.iter().map(|s| s.content.len()).sum();
        let padding = width.saturating_sub(content_len);
        let left_pad = padding / 2;
        let right_pad = padding - left_pad;

        if is_header {
            let header_style = Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD);
            let text: String = cell_spans.iter().map(|s| s.content.as_ref()).collect();
            spans.push(Span::styled(
                format!(
                    "{}{}{}",
                    " ".repeat(left_pad),
                    text,
                    " ".repeat(right_pad)
                ),
                header_style,
            ));
        } else {
            spans.push(Span::raw(" ".repeat(left_pad)));
            spans.extend(cell_spans);
            spans.push(Span::raw(" ".repeat(right_pad)));
        }

        spans.push(Span::styled("│", border_style));
    }

    Line::from(spans)
}

fn build_table_border(col_widths: &[usize], left: char, mid: char, right: char) -> String {
    let mut s = String::new();
    s.push(left);
    for (i, w) in col_widths.iter().enumerate() {
        for _ in 0..*w {
            s.push('─');
        }
        if i < col_widths.len() - 1 {
            s.push(mid);
        }
    }
    s.push(right);
    s
}

fn line_is_blank(line: &Line<'_>) -> bool {
    if line.spans.is_empty() {
        return true;
    }
    line.spans
        .iter()
        .all(|s| s.content.as_ref().trim().is_empty())
}

/// Render markdown text into styled ratatui lines.
pub fn render(input: &str) -> Vec<Line<'static>> {
    Renderer::new().render(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: extract all text from a line
    fn line_text(line: &Line<'_>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    /// Helper: check if a line contains the given text
    fn assert_line_contains(line: &Line<'_>, text: &str) {
        let full = line_text(line);
        assert!(
            full.contains(text),
            "Expected line to contain {:?}, got {:?}",
            text,
            full
        );
    }

    /// Helper: check if any span in a line has the given style property
    fn line_has_modifier(line: &Line<'_>, modifier: Modifier) -> bool {
        line.spans
            .iter()
            .any(|s| s.style.add_modifier.contains(modifier))
    }

    fn line_has_fg(line: &Line<'_>, color: Color) -> bool {
        line.spans.iter().any(|s| s.style.fg == Some(color))
    }

    // ── Block element tests ──

    #[test]
    fn heading_h1() {
        let lines = render("# Hello");
        assert_line_contains(&lines[0], "Hello");
        assert!(line_has_fg(&lines[0], Color::Cyan));
        assert!(line_has_modifier(&lines[0], Modifier::BOLD));
    }

    #[test]
    fn heading_h2() {
        let lines = render("## World");
        assert_line_contains(&lines[0], "World");
        assert!(line_has_fg(&lines[0], Color::Green));
    }

    #[test]
    fn heading_h3() {
        let lines = render("### Sub");
        assert_line_contains(&lines[0], "Sub");
        assert!(line_has_fg(&lines[0], Color::Yellow));
    }

    #[test]
    fn paragraph_spacing() {
        let lines = render("First paragraph.\n\nSecond paragraph.");
        // Should have: "First paragraph.", blank, "Second paragraph."
        let texts: Vec<String> = lines.iter().map(line_text).collect();
        assert!(texts.contains(&"First paragraph.".to_string()));
        assert!(texts.contains(&"Second paragraph.".to_string()));
        // Check there's a blank line between
        let first_idx = texts.iter().position(|t| t == "First paragraph.").unwrap();
        let blank_idx = first_idx + 1;
        assert!(texts[blank_idx].trim().is_empty());
    }

    #[test]
    fn code_block_plain() {
        let lines = render("```\nhello world\n```");
        let all_text: String = lines.iter().map(line_text).collect::<Vec<_>>().join("\n");
        assert!(all_text.contains("hello world"));
    }

    #[test]
    fn code_block_with_language() {
        let lines = render("```rust\nlet x = 1;\n```");
        let all_text: String = lines.iter().map(line_text).collect::<Vec<_>>().join("\n");
        assert!(all_text.contains("let x = 1;"));
        // Should have language label
        assert!(all_text.contains("rust"));
    }

    #[test]
    fn blockquote_basic() {
        let lines = render("> quoted text");
        let all_text: String = lines.iter().map(line_text).collect::<Vec<_>>().join("\n");
        assert!(all_text.contains("▌"));
        assert!(all_text.contains("quoted text"));
    }

    #[test]
    fn unordered_list() {
        let lines = render("- first\n- second\n- third");
        let texts: Vec<String> = lines.iter().map(line_text).collect();
        assert!(texts.iter().any(|t| t.contains("•") && t.contains("first")));
        assert!(texts.iter().any(|t| t.contains("•") && t.contains("second")));
        assert!(texts.iter().any(|t| t.contains("•") && t.contains("third")));
    }

    #[test]
    fn ordered_list() {
        let lines = render("1. alpha\n2. beta\n3. gamma");
        let texts: Vec<String> = lines.iter().map(line_text).collect();
        assert!(texts.iter().any(|t| t.contains("1.") && t.contains("alpha")));
        assert!(texts.iter().any(|t| t.contains("2.") && t.contains("beta")));
        assert!(texts.iter().any(|t| t.contains("3.") && t.contains("gamma")));
    }

    #[test]
    fn task_list() {
        let lines = render("- [x] done\n- [ ] todo");
        let texts: Vec<String> = lines.iter().map(line_text).collect();
        assert!(texts.iter().any(|t| t.contains("☑") && t.contains("done")));
        assert!(texts.iter().any(|t| t.contains("☐") && t.contains("todo")));
    }

    #[test]
    fn horizontal_rule() {
        let lines = render("above\n\n---\n\nbelow");
        let texts: Vec<String> = lines.iter().map(line_text).collect();
        assert!(texts.iter().any(|t| t.contains("─")));
    }

    #[test]
    fn table_basic() {
        let lines = render("| Name | Age |\n|------|-----|\n| Alice | 30 |");
        let all_text: String = lines.iter().map(line_text).collect::<Vec<_>>().join("\n");
        assert!(all_text.contains("Name"));
        assert!(all_text.contains("Alice"));
        assert!(all_text.contains("┌"));
        assert!(all_text.contains("┘"));
    }

    // ── Inline element tests ──

    #[test]
    fn bold_text() {
        let lines = render("some **bold** text");
        assert!(line_has_modifier(&lines[0], Modifier::BOLD));
    }

    #[test]
    fn italic_text() {
        let lines = render("some *italic* text");
        assert!(line_has_modifier(&lines[0], Modifier::ITALIC));
    }

    #[test]
    fn strikethrough_text() {
        let lines = render("some ~~struck~~ text");
        assert!(line_has_modifier(&lines[0], Modifier::CROSSED_OUT));
    }

    #[test]
    fn inline_code() {
        let lines = render("use `code` here");
        let text = line_text(&lines[0]);
        assert!(text.contains("`code`"));
        assert!(lines[0]
            .spans
            .iter()
            .any(|s| s.style.fg == Some(Color::Magenta)));
    }

    #[test]
    fn link() {
        let lines = render("[click](https://example.com)");
        let text = line_text(&lines[0]);
        assert!(text.contains("click"));
        assert!(text.contains("https://example.com"));
        assert!(line_has_fg(&lines[0], Color::Blue));
    }

    // ── Nesting tests ──

    #[test]
    fn bold_inside_heading() {
        let lines = render("# Hello **world**");
        assert_line_contains(&lines[0], "Hello");
        assert_line_contains(&lines[0], "world");
        assert!(line_has_modifier(&lines[0], Modifier::BOLD));
    }

    #[test]
    fn nested_lists() {
        let input = "- outer\n  - inner\n    - deepest";
        let lines = render(input);
        let texts: Vec<String> = lines.iter().map(line_text).collect();
        assert!(texts.iter().any(|t| t.contains("outer")));
        assert!(texts.iter().any(|t| t.contains("inner")));
        assert!(texts.iter().any(|t| t.contains("deepest")));
    }

    // ── Spacing tests ──

    #[test]
    fn no_double_blank_lines() {
        let lines = render("# Title\n\nParagraph\n\n---\n\nMore text");
        let mut consecutive_blanks = 0;
        for line in &lines {
            if line_is_blank(line) {
                consecutive_blanks += 1;
                assert!(
                    consecutive_blanks <= 1,
                    "Found consecutive blank lines"
                );
            } else {
                consecutive_blanks = 0;
            }
        }
    }

    // ── Edge case tests ──

    #[test]
    fn empty_input() {
        let lines = render("");
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn whitespace_only() {
        let lines = render("   \n  \n   ");
        assert_eq!(lines.len(), 1);
    }
}
