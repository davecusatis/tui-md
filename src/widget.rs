use ratatui::prelude::*;
use ratatui::widgets::{Paragraph, Wrap};

/// A ratatui widget that renders markdown text.
///
/// Convenience wrapper around [`crate::render()`] that implements [`Widget`]
/// so you can render markdown directly into a frame area.
pub struct MarkdownWidget {
    lines: Vec<Line<'static>>,
}

impl MarkdownWidget {
    pub fn new(input: &str) -> Self {
        Self {
            lines: crate::render(input),
        }
    }
}

impl Widget for MarkdownWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let paragraph = Paragraph::new(self.lines).wrap(Wrap { trim: false });
        paragraph.render(area, buf);
    }
}
