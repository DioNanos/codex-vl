use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::style::Style;

#[derive(Clone, Copy)]
struct CrtCell {
    ch: char,
    style: Style,
}

pub(crate) struct CrtSurface {
    width: u16,
    height: u16,
    cells: Vec<CrtCell>,
    base_style: Style,
}

impl CrtSurface {
    pub(crate) fn new(width: u16, height: u16, base_style: Style) -> Self {
        let len = width as usize * height as usize;
        Self {
            width,
            height,
            cells: vec![
                CrtCell {
                    ch: ' ',
                    style: base_style,
                };
                len
            ],
            base_style,
        }
    }

    pub(crate) fn width(&self) -> u16 {
        self.width
    }

    pub(crate) fn height(&self) -> u16 {
        self.height
    }

    pub(crate) fn put_clipped(&mut self, x: u16, y: u16, max_width: u16, text: &str, style: Style) {
        if y >= self.height || x >= self.width || max_width == 0 {
            return;
        }
        let available = max_width.min(self.width - x) as usize;
        for (offset, ch) in text.chars().take(available).enumerate() {
            self.set(x + offset as u16, y, ch, style);
        }
    }

    pub(crate) fn ch_at(&self, x: u16, y: u16) -> char {
        self.cell(x, y).ch
    }

    pub(crate) fn set_cell(&mut self, x: u16, y: u16, ch: char, style: Style) {
        self.set(x, y, ch, style);
    }

    pub(crate) fn add_row_modifier(&mut self, y: u16, modifier: Modifier) {
        if y >= self.height || self.width == 0 {
            return;
        }
        let row_start = y as usize * self.width as usize;
        let row_end = row_start + self.width as usize;
        for cell in &mut self.cells[row_start..row_end] {
            cell.style = cell.style.add_modifier(modifier);
        }
    }

    pub(crate) fn fill(&mut self, x: u16, y: u16, width: u16, ch: char, style: Style) {
        if y >= self.height || x >= self.width {
            return;
        }
        for col in x..x.saturating_add(width).min(self.width) {
            self.set(col, y, ch, style);
        }
    }

    pub(crate) fn render(&self, area: Rect, buf: &mut Buffer) {
        let height = area.height.min(self.height);
        let width = area.width.min(self.width);
        for y in 0..height {
            for x in 0..width {
                let cell = self.cell(x, y);
                let symbol = cell.ch.to_string();
                buf[(area.x + x, area.y + y)]
                    .set_symbol(&symbol)
                    .set_style(cell.style);
            }
        }
    }

    fn set(&mut self, x: u16, y: u16, ch: char, style: Style) {
        if x >= self.width || y >= self.height {
            return;
        }
        let idx = y as usize * self.width as usize + x as usize;
        if let Some(cell) = self.cells.get_mut(idx) {
            cell.ch = ch;
            cell.style = style;
        }
    }

    fn cell(&self, x: u16, y: u16) -> CrtCell {
        if x >= self.width || y >= self.height {
            return CrtCell {
                ch: ' ',
                style: self.base_style,
            };
        }
        self.cells[y as usize * self.width as usize + x as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;

    #[test]
    fn put_clipped_never_extends_surface() {
        let mut surface = CrtSurface::new(5, 1, Style::default());
        surface.put_clipped(0, 0, 3, "abcdef", Style::default().fg(Color::Yellow));

        let mut buf = Buffer::empty(Rect::new(0, 0, 5, 1));
        surface.render(Rect::new(0, 0, 5, 1), &mut buf);
        let row: String = (0..5).map(|x| buf[(x, 0)].symbol()).collect();
        assert_eq!(row, "abc  ");
    }

    #[test]
    fn offscreen_writes_are_ignored() {
        let mut surface = CrtSurface::new(4, 2, Style::default());
        surface.put_clipped(8, 0, 1, "x", Style::default());
        surface.put_clipped(0, 8, 1, "x", Style::default());

        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 2));
        surface.render(Rect::new(0, 0, 4, 2), &mut buf);
        let text: String = buf.content.iter().map(|cell| cell.symbol()).collect();
        assert_eq!(text, "        ");
    }
}
