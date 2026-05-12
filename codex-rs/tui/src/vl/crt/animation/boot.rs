//! Boot animation renderer for the Vivling Rich-tier strip.
//!
//! Painted into a temporarily-expanded `CrtSurface` (rows = `BOOT_STRIP_HEIGHT`)
//! during the first frames of the app. The renderer is a pure function of
//! `(BootPhase, sprite, palette)`; lifetime/skip is owned by `CrtAnimationLedger`.

use ratatui::style::Modifier;
use ratatui::style::Style;

use super::ledger::BootPhase;
use crate::vl::crt::palette::Palette;
use crate::vl::crt::sprites::boot::BOOT_SPRITE_HEIGHT;
use crate::vl::crt::sprites::boot::BOOT_SPRITE_WIDTH;
use crate::vl::crt::sprites::boot::BootEyeState;
use crate::vl::crt::sprites::boot::BootSprite;
use crate::vl::crt::sprites::boot::boot_sprite_for_species;
use crate::vl::crt::surface::CrtSurface;

/// Total rows the boot animation occupies (8 sprite rows + 1 greeting row).
pub(crate) const BOOT_STRIP_HEIGHT: u16 = BOOT_SPRITE_HEIGHT + 1;

/// Render the boot animation onto a freshly-cleared surface. The surface
/// must be at least `BOOT_STRIP_HEIGHT` rows tall and `BOOT_SPRITE_WIDTH`
/// wide; narrower surfaces silently no-op (caller falls back to the
/// regular 3-row scene).
pub(crate) fn render_boot_strip(
    surface: &mut CrtSurface,
    palette: &Palette,
    species_id: &str,
    phase: BootPhase,
) {
    if surface.height() < BOOT_STRIP_HEIGHT || surface.width() < BOOT_SPRITE_WIDTH {
        return;
    }

    let sprite = boot_sprite_for_species(species_id);
    let visible_rows = match phase {
        BootPhase::ScanLineWipe { progress } => {
            ((BOOT_SPRITE_HEIGHT as f32) * progress).floor() as u16
        }
        _ => BOOT_SPRITE_HEIGHT,
    };

    let eye_state = match phase {
        BootPhase::ScanLineWipe { .. } | BootPhase::EyesClosed { .. } => BootEyeState::Closed,
        BootPhase::Blink { progress } if progress < 0.5 => BootEyeState::Closed,
        BootPhase::Blink { .. } | BootPhase::Greeting { .. } => BootEyeState::Open,
    };

    paint_sprite(surface, sprite, palette, visible_rows, eye_state);
    paint_scanline_cursor(surface, palette, &phase, visible_rows);
    paint_greeting(surface, palette, sprite, &phase);
}

fn paint_sprite(
    surface: &mut CrtSurface,
    sprite: &BootSprite,
    palette: &Palette,
    visible_rows: u16,
    eyes: BootEyeState,
) {
    let surface_w = surface.width();
    let sprite_w = BOOT_SPRITE_WIDTH.min(surface_w);
    let x0 = surface_w.saturating_sub(sprite_w) / 2;
    let rows = sprite.rows(eyes);

    for y in 0..BOOT_SPRITE_HEIGHT {
        let visible = y < visible_rows;
        let row = rows[y as usize];
        let style = if visible { palette.face } else { palette.dim };
        let mut buf = [0u8; 4];
        for (offset, ch) in row.chars().enumerate() {
            if !visible || ch == ' ' {
                continue;
            }
            let x = x0.saturating_add(offset as u16);
            if x >= surface_w {
                break;
            }
            let s = ch.encode_utf8(&mut buf);
            surface.put_clipped(x, y, 1, s, style);
        }
    }
}

fn paint_scanline_cursor(
    surface: &mut CrtSurface,
    palette: &Palette,
    phase: &BootPhase,
    visible_rows: u16,
) {
    let BootPhase::ScanLineWipe { .. } = phase else {
        return;
    };
    if visible_rows >= BOOT_SPRITE_HEIGHT {
        return;
    }
    let cursor_y = visible_rows;
    let dim = palette.dim;
    let style = Style {
        add_modifier: dim.add_modifier | Modifier::REVERSED,
        ..dim
    };
    let surface_w = surface.width();
    for x in 0..surface_w {
        surface.set_cell(x, cursor_y, '_', style);
    }
}

fn paint_greeting(
    surface: &mut CrtSurface,
    palette: &Palette,
    sprite: &BootSprite,
    phase: &BootPhase,
) {
    let BootPhase::Greeting { chars_revealed } = phase else {
        return;
    };
    let row = BOOT_SPRITE_HEIGHT;
    if surface.height() <= row {
        return;
    }
    let greeting = sprite.greeting;
    let total = greeting.chars().count();
    let revealed = (*chars_revealed).min(total);
    let surface_w = surface.width();
    let x0 = surface_w.saturating_sub(total as u16) / 2;
    let mut buf = [0u8; 4];
    for (offset, ch) in greeting.chars().take(revealed).enumerate() {
        let x = x0.saturating_add(offset as u16);
        if x >= surface_w {
            break;
        }
        let s = ch.encode_utf8(&mut buf);
        surface.put_clipped(x, row, 1, s, palette.signal);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;

    fn collect(surface: &CrtSurface, w: u16, h: u16) -> String {
        let mut buf = Buffer::empty(Rect::new(0, 0, w, h));
        surface.render(Rect::new(0, 0, w, h), &mut buf);
        buf.content.iter().map(|c| c.symbol()).collect()
    }

    #[test]
    fn boot_renders_eyes_closed_during_eyes_closed_phase() {
        let palette = Palette::codex();
        let mut surface =
            CrtSurface::new(BOOT_SPRITE_WIDTH + 4, BOOT_STRIP_HEIGHT, Style::default());
        render_boot_strip(
            &mut surface,
            &palette,
            "syllo",
            BootPhase::EyesClosed { progress: 0.5 },
        );
        let text = collect(&surface, BOOT_SPRITE_WIDTH + 4, BOOT_STRIP_HEIGHT);
        assert!(!text.contains('o'), "eyes-closed must not render 'o' eyes");
    }

    #[test]
    fn boot_renders_eyes_open_after_blink_completes() {
        let palette = Palette::codex();
        let mut surface =
            CrtSurface::new(BOOT_SPRITE_WIDTH + 4, BOOT_STRIP_HEIGHT, Style::default());
        render_boot_strip(
            &mut surface,
            &palette,
            "syllo",
            BootPhase::Blink { progress: 1.0 },
        );
        let text = collect(&surface, BOOT_SPRITE_WIDTH + 4, BOOT_STRIP_HEIGHT);
        assert!(text.contains('o'));
    }

    #[test]
    fn scanline_phase_reveals_progressively() {
        let palette = Palette::codex();
        let w = BOOT_SPRITE_WIDTH + 4;
        let mut early = CrtSurface::new(w, BOOT_STRIP_HEIGHT, Style::default());
        let mut late = CrtSurface::new(w, BOOT_STRIP_HEIGHT, Style::default());
        render_boot_strip(
            &mut early,
            &palette,
            "syllo",
            BootPhase::ScanLineWipe { progress: 0.1 },
        );
        render_boot_strip(
            &mut late,
            &palette,
            "syllo",
            BootPhase::ScanLineWipe { progress: 0.9 },
        );
        let early_text = collect(&early, w, BOOT_STRIP_HEIGHT);
        let late_text = collect(&late, w, BOOT_STRIP_HEIGHT);
        let early_glyphs = early_text.chars().filter(|c| !c.is_whitespace()).count();
        let late_glyphs = late_text.chars().filter(|c| !c.is_whitespace()).count();
        assert!(
            late_glyphs > early_glyphs,
            "later progress should reveal more glyphs (early={early_glyphs}, late={late_glyphs})"
        );
    }

    #[test]
    fn greeting_typewriter_reveals_chars() {
        let palette = Palette::codex();
        let w = 40;
        let mut surface = CrtSurface::new(w, BOOT_STRIP_HEIGHT, Style::default());
        render_boot_strip(
            &mut surface,
            &palette,
            "syllo",
            BootPhase::Greeting { chars_revealed: 4 },
        );
        let mut buf = Buffer::empty(Rect::new(0, 0, w, BOOT_STRIP_HEIGHT));
        surface.render(Rect::new(0, 0, w, BOOT_STRIP_HEIGHT), &mut buf);
        let last_row: String = (0..w).map(|x| buf[(x, BOOT_SPRITE_HEIGHT)].symbol()).collect();
        let glyphs: String = last_row.chars().filter(|c| !c.is_whitespace()).collect();
        // First 4 chars of "vivling.syllo online"
        assert_eq!(glyphs, "vivl");
    }

    #[test]
    fn narrow_surface_silently_skips_render() {
        let palette = Palette::codex();
        let mut surface = CrtSurface::new(8, BOOT_STRIP_HEIGHT, Style::default());
        render_boot_strip(
            &mut surface,
            &palette,
            "syllo",
            BootPhase::Greeting { chars_revealed: 100 },
        );
        let text = collect(&surface, 8, BOOT_STRIP_HEIGHT);
        assert!(text.chars().all(|c| c == ' '));
    }
}
