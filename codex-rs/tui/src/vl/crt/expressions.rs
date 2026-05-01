use ratatui::style::Style;

use super::director::CrtMode;
use super::palette::Palette;
use super::surface::CrtSurface;

pub(crate) const ART_WIDTH: u16 = 14;
pub(crate) const ACCENT_MIN_WIDTH: u16 = 18;

pub(crate) fn overlay_accents(
    surface: &mut CrtSurface,
    mode: CrtMode,
    tick: u64,
    palette: &Palette,
    art_left: u16,
    art_right: u16,
) {
    let width = surface.width();
    if width < ACCENT_MIN_WIDTH || surface.height() < 3 {
        return;
    }
    match mode {
        CrtMode::Idle => idle(surface, tick, palette, art_right, width),
        CrtMode::Thinking => thinking(surface, tick, palette, art_left),
        CrtMode::Working => working(surface, tick, palette, art_right, width),
        CrtMode::Alert => alert(surface, tick, palette, art_left, art_right, width),
        CrtMode::Tired => tired(surface, tick, palette, art_right, width),
        CrtMode::Hungry => hungry(surface, tick, palette, art_left, art_right, width),
    }
}

fn put_if_space(surface: &mut CrtSurface, x: u16, y: u16, ch: char, style: Style) {
    if x >= surface.width() || y >= surface.height() {
        return;
    }
    if surface.ch_at(x, y) == ' ' {
        surface.set_cell(x, y, ch, style);
    }
}

fn idle(surface: &mut CrtSurface, tick: u64, palette: &Palette, art_right: u16, width: u16) {
    // very subtle "breath" tick to the right of the face
    if tick % 8 == 0 && art_right + 1 < width {
        put_if_space(surface, art_right + 1, 0, '`', palette.dim);
    }
}

fn thinking(surface: &mut CrtSurface, tick: u64, palette: &Palette, art_left: u16) {
    if art_left < 3 {
        return;
    }
    let phase = (tick % 4) as usize;
    let chars = ['.', 'o', 'O', '?'];
    let col = art_left.saturating_sub(3);
    put_if_space(surface, col, 0, chars[phase], palette.signal);
    if phase >= 2 && art_left >= 2 {
        put_if_space(surface, art_left.saturating_sub(2), 1, '.', palette.dim);
    }
}

fn working(surface: &mut CrtSurface, tick: u64, palette: &Palette, art_right: u16, width: u16) {
    if art_right + 1 >= width {
        return;
    }
    let phase = tick % 4;
    let tool = match phase {
        0 => '/',
        1 => '|',
        2 => '\\',
        _ => '_',
    };
    put_if_space(surface, art_right + 1, 1, tool, palette.signal);
    if phase % 2 == 0 && art_right + 2 < width {
        put_if_space(surface, art_right + 2, 0, '*', palette.signal);
    }
}

fn alert(
    surface: &mut CrtSurface,
    tick: u64,
    palette: &Palette,
    art_left: u16,
    art_right: u16,
    width: u16,
) {
    let blink = tick % 2 == 0;
    let ch = if blink { '!' } else { ':' };
    if art_right + 1 < width {
        put_if_space(surface, art_right + 1, 0, ch, palette.alert);
    }
    if art_left >= 2 {
        put_if_space(surface, art_left.saturating_sub(2), 0, ch, palette.alert);
    }
}

fn tired(surface: &mut CrtSurface, tick: u64, palette: &Palette, art_right: u16, width: u16) {
    if art_right + 1 >= width {
        return;
    }
    let drift = (tick % 4) as u16;
    let x = (art_right + 1)
        .saturating_add(drift)
        .min(width.saturating_sub(1));
    put_if_space(surface, x, 0, 'z', palette.dim);
}

fn hungry(
    surface: &mut CrtSurface,
    tick: u64,
    palette: &Palette,
    art_left: u16,
    art_right: u16,
    width: u16,
) {
    let offset = (tick % 2) as u16;
    let mut x = offset;
    while x < art_left {
        put_if_space(surface, x, 2, '~', palette.dim);
        x = x.saturating_add(3);
    }
    let mut x = art_right.saturating_add(offset);
    while x < width {
        put_if_space(surface, x, 2, '~', palette.dim);
        x = x.saturating_add(3);
    }
}
