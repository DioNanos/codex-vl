use super::palette::Palette;
use super::palette::Slot;
use super::surface::CrtSurface;

pub(crate) struct Frame {
    pub(crate) rows: [&'static str; 3],
    pub(crate) row_slots: [Slot; 3],
}

impl Frame {
    pub(crate) const fn new(rows: [&'static str; 3], row_slots: [Slot; 3]) -> Self {
        Self { rows, row_slots }
    }
}

pub(crate) fn compose_at(surface: &mut CrtSurface, frame: &Frame, x0: u16, palette: &Palette) {
    let width = surface.width();
    if width == 0 || surface.height() < 3 {
        return;
    }
    for (row_idx, text) in frame.rows.iter().enumerate() {
        let slot = frame.row_slots[row_idx];
        let style = match palette.style_for(slot) {
            Some(s) => s,
            None => continue,
        };
        let mut buf = [0u8; 4];
        for (offset, ch) in text.chars().enumerate() {
            if ch == ' ' {
                continue;
            }
            let x = x0.saturating_add(offset as u16);
            if x >= width {
                break;
            }
            let s = ch.encode_utf8(&mut buf);
            surface.put_clipped(x, row_idx as u16, 1, s, style);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::style::Style;

    #[test]
    fn compose_centers_frame_and_skips_transparent_spaces() {
        let palette = Palette::codex();
        let mut surface = CrtSurface::new(10, 3, Style::default());
        surface.fill(0, 0, 10, '.', palette.dim);
        surface.fill(0, 1, 10, '.', palette.dim);
        surface.fill(0, 2, 10, '.', palette.dim);

        let frame = Frame::new(
            ["abc", "xyz", "uvw"],
            [Slot::Signal, Slot::Face, Slot::Signal],
        );
        let len = frame.rows[0].chars().count() as u16;
        let bounded = len.min(surface.width());
        let x0 = surface.width().saturating_sub(bounded) / 2;
        compose_at(&mut surface, &frame, x0, &palette);

        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 3));
        surface.render(Rect::new(0, 0, 10, 3), &mut buf);
        let row1: String = (0..10).map(|x| buf[(x, 1)].symbol()).collect();
        // 3-wide centered in 10 → x0=3
        assert_eq!(row1, "...xyz....");
    }

    #[test]
    fn compose_does_not_overflow_narrow_surface() {
        let palette = Palette::codex();
        let mut surface = CrtSurface::new(6, 3, Style::default());
        let frame = Frame::new(
            ["aaaaaaaaaaa", "bbbbbbbbbbb", "ccccccccccc"],
            [Slot::Signal, Slot::Face, Slot::Signal],
        );
        let len = frame.rows[0].chars().count() as u16;
        let bounded = len.min(surface.width());
        let x0 = surface.width().saturating_sub(bounded) / 2;
        compose_at(&mut surface, &frame, x0, &palette);

        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 3));
        surface.render(Rect::new(0, 0, 6, 3), &mut buf);
        let rendered: String = buf.content.iter().map(|cell| cell.symbol()).collect();
        assert_eq!(rendered.len(), 18);
    }

    #[test]
    fn transparent_row_slot_leaves_background_intact() {
        let palette = Palette::codex();
        let mut surface = CrtSurface::new(8, 3, Style::default());
        surface.fill(0, 0, 8, '~', palette.dim);
        let frame = Frame::new(
            ["xxxx", "    ", "    "],
            [Slot::Transparent, Slot::Face, Slot::Signal],
        );
        let len = frame.rows[0].chars().count() as u16;
        let bounded = len.min(surface.width());
        let x0 = surface.width().saturating_sub(bounded) / 2;
        compose_at(&mut surface, &frame, x0, &palette);

        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 3));
        surface.render(Rect::new(0, 0, 8, 3), &mut buf);
        let row0: String = (0..8).map(|x| buf[(x, 0)].symbol()).collect();
        assert_eq!(row0, "~~~~~~~~");
    }
}
