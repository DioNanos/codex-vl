use ratatui::style::Modifier;

use super::palette::Palette;
use super::surface::CrtSurface;

const FACE_ROW: u16 = 1;
const PHOSPHOR_PER_ROW: u32 = 3;
const PHOSPHOR_TICK_DIVISOR: u64 = 360;

pub(crate) fn apply_all(surface: &mut CrtSurface, palette: &Palette, seed: u32, elapsed_ms: u64) {
    apply_scanline(surface);
    apply_phosphor(surface, palette, seed, elapsed_ms);
}

pub(crate) fn apply_scanline(surface: &mut CrtSurface) {
    if surface.height() < 3 {
        return;
    }
    surface.add_row_modifier(0, Modifier::DIM);
    surface.add_row_modifier(2, Modifier::DIM);
}

pub(crate) fn apply_phosphor(
    surface: &mut CrtSurface,
    palette: &Palette,
    seed: u32,
    elapsed_ms: u64,
) {
    let width = surface.width();
    if width < 4 || surface.height() < 3 {
        return;
    }
    let tick = elapsed_ms / PHOSPHOR_TICK_DIVISOR;
    let dim = palette.dim;
    for &row in &[0u16, 2u16] {
        if row == FACE_ROW {
            continue;
        }
        for k in 0..PHOSPHOR_PER_ROW {
            let h = mix(seed as u64, tick, row as u64, k as u64);
            let x = (h % width as u64) as u16;
            if surface.ch_at(x, row) == ' ' {
                surface.set_cell(x, row, '.', dim);
            }
        }
    }
}

fn mix(a: u64, b: u64, c: u64, d: u64) -> u64 {
    let mut x = a.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(b);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94D0_49BB_1331_11EB).wrapping_add(c);
    x ^= x >> 31;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9).wrapping_add(d);
    x ^ (x >> 33)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::style::Style;

    fn collect(surface: &CrtSurface, w: u16) -> String {
        let mut buf = Buffer::empty(Rect::new(0, 0, w, 3));
        surface.render(Rect::new(0, 0, w, 3), &mut buf);
        buf.content.iter().map(|c| c.symbol()).collect()
    }

    fn seed_face_row(surface: &mut CrtSurface, palette: &Palette) {
        // simulate a centred face on row 1 with a known marker
        surface.fill(0, 1, surface.width(), ' ', palette.face);
        surface.set_cell(5, 1, '(', palette.face);
        surface.set_cell(6, 1, 'o', palette.face);
        surface.set_cell(7, 1, ')', palette.face);
    }

    #[test]
    fn effects_preserve_dimensions_across_widths() {
        let palette = Palette::codex();
        for width in [10u16, 14, 18, 24, 40, 80] {
            let mut surface = CrtSurface::new(width, 3, Style::default());
            surface.fill(0, 0, width, ' ', palette.dim);
            surface.fill(0, 2, width, ' ', palette.dim);
            seed_face_row(&mut surface, &palette);
            apply_all(&mut surface, &palette, 7, 240);
            let rendered = collect(&surface, width);
            assert_eq!(rendered.len(), width as usize * 3);
        }
    }

    #[test]
    fn phosphor_is_deterministic_for_same_seed_and_tick() {
        let palette = Palette::codex();
        let mut a = CrtSurface::new(40, 3, Style::default());
        let mut b = CrtSurface::new(40, 3, Style::default());
        for s in [&mut a, &mut b] {
            s.fill(0, 0, 40, ' ', palette.dim);
            s.fill(0, 2, 40, ' ', palette.dim);
            seed_face_row(s, &palette);
            apply_all(s, &palette, 11, 720);
        }
        assert_eq!(collect(&a, 40), collect(&b, 40));
    }

    #[test]
    fn effects_do_not_erase_face_row() {
        let palette = Palette::codex();
        let mut surface = CrtSurface::new(40, 3, Style::default());
        surface.fill(0, 0, 40, ' ', palette.dim);
        surface.fill(0, 2, 40, ' ', palette.dim);
        seed_face_row(&mut surface, &palette);
        let before: String = (0..40).map(|x| surface.ch_at(x, 1)).collect();
        apply_all(&mut surface, &palette, 99, 1800);
        let after: String = (0..40).map(|x| surface.ch_at(x, 1)).collect();
        assert_eq!(before, after);
        assert!(after.contains("(o)"));
    }

    #[test]
    fn phosphor_only_replaces_space_cells() {
        let palette = Palette::codex();
        let mut surface = CrtSurface::new(20, 3, Style::default());
        // Fill rows 0 and 2 with non-space chars so phosphor must not overwrite.
        surface.fill(0, 0, 20, '#', palette.signal);
        surface.fill(0, 2, 20, '#', palette.signal);
        seed_face_row(&mut surface, &palette);
        apply_all(&mut surface, &palette, 5, 360);
        for y in [0u16, 2] {
            for x in 0..20u16 {
                assert_eq!(surface.ch_at(x, y), '#');
            }
        }
    }

    #[test]
    fn effects_safe_on_narrow_surface() {
        let palette = Palette::codex();
        let mut surface = CrtSurface::new(3, 3, Style::default());
        surface.fill(0, 0, 3, ' ', palette.dim);
        surface.fill(0, 1, 3, ' ', palette.face);
        surface.fill(0, 2, 3, ' ', palette.dim);
        apply_all(&mut surface, &palette, 1, 0);
        let rendered = collect(&surface, 3);
        assert_eq!(rendered.len(), 9);
    }
}
