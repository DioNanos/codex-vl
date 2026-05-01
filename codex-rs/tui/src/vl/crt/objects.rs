use super::palette::Palette;
use super::surface::CrtSurface;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CrtObject {
    Ball,
    Cube,
    CrtOrb,
    Snack,
    Blanket,
    Nest,
    Pillow,
    Terminal,
    Logbook,
    TestChip,
    MemoryShard,
    ScanLens,
    SignalKey,
    LogLantern,
}

pub(crate) fn draw_object(
    surface: &mut CrtSurface,
    x: u16,
    width: u16,
    object: CrtObject,
    elapsed_ms: u64,
    palette: &Palette,
) -> bool {
    if width < 4 || surface.height() < 3 {
        return false;
    }
    match object {
        CrtObject::Ball => draw_ball(surface, x, width, elapsed_ms, palette),
        CrtObject::Cube => super::object_catalog::draw_cube(surface, x, width, elapsed_ms, palette),
        CrtObject::CrtOrb => {
            super::object_catalog::draw_crt_orb(surface, x, width, elapsed_ms, palette)
        }
        CrtObject::Snack => draw_snack(surface, x, width, elapsed_ms, palette),
        CrtObject::Blanket => draw_blanket(surface, x, width, elapsed_ms, palette),
        CrtObject::Nest => super::object_catalog::draw_nest(surface, x, width, elapsed_ms, palette),
        CrtObject::Pillow => {
            super::object_catalog::draw_pillow(surface, x, width, elapsed_ms, palette)
        }
        CrtObject::Terminal => draw_terminal(surface, x, width, elapsed_ms, palette),
        CrtObject::Logbook => {
            super::object_catalog::draw_logbook(surface, x, width, elapsed_ms, palette)
        }
        CrtObject::TestChip => {
            super::object_catalog::draw_test_chip(surface, x, width, elapsed_ms, palette)
        }
        CrtObject::MemoryShard => {
            super::object_catalog::draw_memory_shard(surface, x, width, elapsed_ms, palette)
        }
        CrtObject::ScanLens => {
            super::object_catalog::draw_scan_lens(surface, x, width, elapsed_ms, palette)
        }
        CrtObject::SignalKey => {
            super::object_catalog::draw_signal_key(surface, x, width, elapsed_ms, palette)
        }
        CrtObject::LogLantern => {
            super::object_catalog::draw_log_lantern(surface, x, width, elapsed_ms, palette)
        }
    }
}

fn draw_ball(
    surface: &mut CrtSurface,
    x: u16,
    width: u16,
    elapsed_ms: u64,
    palette: &Palette,
) -> bool {
    let span = width.saturating_sub(2).max(1);
    let pos = ((elapsed_ms / 420) as u16 % span).min(width.saturating_sub(1));
    surface.put_clipped(x + pos, 1, width.saturating_sub(pos), "o", palette.signal);
    if pos > 0 {
        surface.put_clipped(
            x + pos - 1,
            2,
            width.saturating_sub(pos - 1),
            ".",
            palette.dim,
        );
    }
    true
}

fn draw_snack(
    surface: &mut CrtSurface,
    x: u16,
    width: u16,
    elapsed_ms: u64,
    palette: &Palette,
) -> bool {
    let bite = (elapsed_ms / 900) % 3;
    let snack = match bite {
        0 => "<*>",
        1 => "<* ",
        _ => " * ",
    };
    surface.put_clipped(x, 1, width, snack, palette.signal);
    if width > 5 {
        surface.put_clipped(x + 4, 2, width.saturating_sub(4), "..", palette.dim);
    }
    true
}

fn draw_blanket(
    surface: &mut CrtSurface,
    x: u16,
    width: u16,
    elapsed_ms: u64,
    palette: &Palette,
) -> bool {
    let phase = ((elapsed_ms / 1200) % 3) as u16;
    let z_x = x.saturating_add(phase).min(x + width.saturating_sub(1));
    surface.put_clipped(z_x, 0, width.saturating_sub(phase), "z", palette.dim);
    if width >= 8 {
        surface.put_clipped(x, 2, width, "~~~~", palette.signal);
    } else {
        surface.put_clipped(x, 2, width, "~~", palette.signal);
    }
    true
}

fn draw_terminal(
    surface: &mut CrtSurface,
    x: u16,
    width: u16,
    elapsed_ms: u64,
    palette: &Palette,
) -> bool {
    if width < 6 {
        return false;
    }
    let cursor = if (elapsed_ms / 600) % 2 == 0 {
        "_"
    } else {
        " "
    };
    surface.put_clipped(x, 1, width, "[>]", palette.signal);
    surface.put_clipped(x + 3, 1, width.saturating_sub(3), cursor, palette.face);
    if width > 8 {
        surface.put_clipped(x + 5, 0, width.saturating_sub(5), "::", palette.dim);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::style::Style;

    fn render(object: CrtObject) -> String {
        let palette = Palette::codex();
        let mut surface = CrtSurface::new(16, 3, Style::default());
        draw_object(&mut surface, 0, 16, object, 1000, &palette);
        let mut buf = Buffer::empty(Rect::new(0, 0, 16, 3));
        surface.render(Rect::new(0, 0, 16, 3), &mut buf);
        buf.content.iter().map(|c| c.symbol()).collect()
    }

    #[test]
    fn objects_are_visual_not_words() {
        for object in [
            CrtObject::Ball,
            CrtObject::Cube,
            CrtObject::CrtOrb,
            CrtObject::Snack,
            CrtObject::Blanket,
            CrtObject::Nest,
            CrtObject::Pillow,
            CrtObject::Terminal,
            CrtObject::Logbook,
            CrtObject::TestChip,
            CrtObject::MemoryShard,
            CrtObject::ScanLens,
            CrtObject::SignalKey,
            CrtObject::LogLantern,
        ] {
            let rendered = render(object);
            assert!(!rendered.contains("ball"));
            assert!(!rendered.contains("snack"));
            assert!(!rendered.contains("sleep"));
            assert!(!rendered.contains("work"));
        }
    }

    #[test]
    fn objects_keep_three_row_shape() {
        for object in [
            CrtObject::Ball,
            CrtObject::Cube,
            CrtObject::CrtOrb,
            CrtObject::Snack,
            CrtObject::Blanket,
            CrtObject::Nest,
            CrtObject::Pillow,
            CrtObject::Terminal,
            CrtObject::Logbook,
            CrtObject::TestChip,
            CrtObject::MemoryShard,
            CrtObject::ScanLens,
            CrtObject::SignalKey,
            CrtObject::LogLantern,
        ] {
            let rendered = render(object);
            assert_eq!(rendered.len(), 16 * 3);
            assert!(rendered.is_ascii());
        }
    }
}
