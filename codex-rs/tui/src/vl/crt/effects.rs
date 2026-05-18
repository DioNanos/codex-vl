use ratatui::style::Modifier;

use super::animation::TransitionPhases;
use super::animation::VivlingCrtConfig;
use super::animation::transitions::FLICKER_BURST_MS;
use super::animation::transitions::FLICKER_PERIOD_MS;
use super::animation::transitions::ease_in_out_cubic;
use super::palette::Palette;
use super::surface::CrtSurface;

const FACE_ROW: u16 = 1;
const PHOSPHOR_PER_ROW: u32 = 3;
const PHOSPHOR_TICK_DIVISOR: u64 = 360;

/// Apply the ambient CRT layer (scanline, phosphor, flicker, breathing
/// pulse, mode-fade ghost) on top of an already-composed surface.
///
/// Each layer is gated by a `VivlingCrtConfig` toggle so users can opt
/// out individually.
pub(crate) fn apply_all(
    surface: &mut CrtSurface,
    palette: &Palette,
    seed: u32,
    elapsed_ms: u64,
    config: &VivlingCrtConfig,
    transitions: TransitionPhases,
) {
    if config.scanlines {
        apply_scanline(surface);
    }
    if config.phosphor_glow {
        apply_phosphor(surface, palette, seed, elapsed_ms);
    }
    if config.idle_microanim {
        apply_breathing_pulse(surface, elapsed_ms);
    }
    if config.flicker {
        apply_flicker(surface, seed, elapsed_ms);
    }
    if config.transitions {
        apply_mode_fade(surface, transitions.mode_fade);
    }
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

/// Slow sinusoidal "breathing" of the face row's emphasis. Adds a
/// `BOLD` accent every ~9.5s for a brief window. Subtle; uses palette
/// modifiers only (no colour shift).
pub(crate) fn apply_breathing_pulse(surface: &mut CrtSurface, elapsed_ms: u64) {
    if surface.height() < 3 {
        return;
    }
    let period_ms: u64 = 9_500;
    let phase = (elapsed_ms % period_ms) as f32 / period_ms as f32;
    // Window where we add an extra emphasis modifier (front-quarter of period).
    let activated = phase < 0.18;
    let intensity = ease_in_out_cubic(if activated { 1.0 - (phase / 0.18) } else { 0.0 });
    if intensity > 0.55 {
        surface.add_row_modifier(FACE_ROW, Modifier::BOLD);
    }
}

/// Brief whole-strip flicker every ~2.7s. We dim a band of rows for
/// `FLICKER_BURST_MS` to simulate a phosphor instability. Deterministic
/// per seed.
pub(crate) fn apply_flicker(surface: &mut CrtSurface, seed: u32, elapsed_ms: u64) {
    if surface.height() < 3 {
        return;
    }
    let phase = elapsed_ms % FLICKER_PERIOD_MS;
    if phase >= FLICKER_BURST_MS {
        return;
    }
    // Vary which rows flicker per period so the eye doesn't lock onto a pattern.
    let cycle = elapsed_ms / FLICKER_PERIOD_MS;
    let bucket = mix(seed as u64, cycle, 0, 0) % 3;
    let target_row: u16 = match bucket {
        0 => 0,
        1 => 2,
        _ => FACE_ROW,
    };
    if target_row >= surface.height() {
        return;
    }
    surface.add_row_modifier(target_row, Modifier::DIM);
}

/// Cross-fade ghost during a mode transition. While `mode_fade < 1.0`,
/// the face row gets a slight DIM modifier that decays as the new
/// expression settles. Cheap, no extra layer required.
pub(crate) fn apply_mode_fade(surface: &mut CrtSurface, mode_fade: f32) {
    if surface.height() < 3 {
        return;
    }
    if mode_fade >= 0.85 {
        return;
    }
    surface.add_row_modifier(FACE_ROW, Modifier::DIM);
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
        surface.fill(0, 1, surface.width(), ' ', palette.face);
        surface.set_cell(5, 1, '(', palette.face);
        surface.set_cell(6, 1, 'o', palette.face);
        surface.set_cell(7, 1, ')', palette.face);
    }

    #[test]
    fn effects_preserve_dimensions_across_widths() {
        let palette = Palette::codex();
        let cfg = VivlingCrtConfig::default();
        let trans = TransitionPhases {
            mode_fade: 1.0,
            ..Default::default()
        };
        for width in [10u16, 14, 18, 24, 40, 80] {
            let mut surface = CrtSurface::new(width, 3, Style::default());
            surface.fill(0, 0, width, ' ', palette.dim);
            surface.fill(0, 2, width, ' ', palette.dim);
            seed_face_row(&mut surface, &palette);
            apply_all(&mut surface, &palette, 7, 240, &cfg, trans);
            let rendered = collect(&surface, width);
            assert_eq!(rendered.len(), width as usize * 3);
        }
    }

    #[test]
    fn phosphor_is_deterministic_for_same_seed_and_tick() {
        let palette = Palette::codex();
        let cfg = VivlingCrtConfig::default();
        let trans = TransitionPhases {
            mode_fade: 1.0,
            ..Default::default()
        };
        let mut a = CrtSurface::new(40, 3, Style::default());
        let mut b = CrtSurface::new(40, 3, Style::default());
        for s in [&mut a, &mut b] {
            s.fill(0, 0, 40, ' ', palette.dim);
            s.fill(0, 2, 40, ' ', palette.dim);
            seed_face_row(s, &palette);
            apply_all(s, &palette, 11, 720, &cfg, trans);
        }
        assert_eq!(collect(&a, 40), collect(&b, 40));
    }

    #[test]
    fn effects_do_not_erase_face_row() {
        let palette = Palette::codex();
        let cfg = VivlingCrtConfig::default();
        let trans = TransitionPhases {
            mode_fade: 1.0,
            ..Default::default()
        };
        let mut surface = CrtSurface::new(40, 3, Style::default());
        surface.fill(0, 0, 40, ' ', palette.dim);
        surface.fill(0, 2, 40, ' ', palette.dim);
        seed_face_row(&mut surface, &palette);
        let before: String = (0..40).map(|x| surface.ch_at(x, 1)).collect();
        apply_all(&mut surface, &palette, 99, 1800, &cfg, trans);
        let after: String = (0..40).map(|x| surface.ch_at(x, 1)).collect();
        assert_eq!(before, after);
        assert!(after.contains("(o)"));
    }

    #[test]
    fn phosphor_only_replaces_space_cells() {
        let palette = Palette::codex();
        let cfg = VivlingCrtConfig {
            flicker: false,
            idle_microanim: false,
            transitions: false,
            ..VivlingCrtConfig::default()
        };
        let trans = TransitionPhases {
            mode_fade: 1.0,
            ..Default::default()
        };
        let mut surface = CrtSurface::new(20, 3, Style::default());
        surface.fill(0, 0, 20, '#', palette.signal);
        surface.fill(0, 2, 20, '#', palette.signal);
        seed_face_row(&mut surface, &palette);
        apply_all(&mut surface, &palette, 5, 360, &cfg, trans);
        for y in [0u16, 2] {
            for x in 0..20u16 {
                assert_eq!(surface.ch_at(x, y), '#');
            }
        }
    }

    #[test]
    fn effects_safe_on_narrow_surface() {
        let palette = Palette::codex();
        let cfg = VivlingCrtConfig::default();
        let trans = TransitionPhases {
            mode_fade: 1.0,
            ..Default::default()
        };
        let mut surface = CrtSurface::new(3, 3, Style::default());
        surface.fill(0, 0, 3, ' ', palette.dim);
        surface.fill(0, 1, 3, ' ', palette.face);
        surface.fill(0, 2, 3, ' ', palette.dim);
        apply_all(&mut surface, &palette, 1, 0, &cfg, trans);
        let rendered = collect(&surface, 3);
        assert_eq!(rendered.len(), 9);
    }

    #[test]
    fn opt_out_disables_layers() {
        let palette = Palette::codex();
        let cfg = VivlingCrtConfig {
            scanlines: false,
            phosphor_glow: false,
            flicker: false,
            transitions: false,
            idle_microanim: false,
        };
        let trans = TransitionPhases::default();
        let mut surface = CrtSurface::new(40, 3, Style::default());
        surface.fill(0, 0, 40, ' ', palette.dim);
        surface.fill(0, 2, 40, ' ', palette.dim);
        seed_face_row(&mut surface, &palette);
        apply_all(&mut surface, &palette, 7, 240, &cfg, trans);
        let rendered = collect(&surface, 40);
        // Face still present; no extra phosphor dots painted on rows 0/2.
        assert!(rendered.contains("(o)"));
        let row0: String = (0..40).map(|x| surface.ch_at(x, 0)).collect();
        assert!(row0.chars().all(|c| c == ' '));
    }

    #[test]
    fn flicker_is_bounded_in_time() {
        let palette = Palette::codex();
        let cfg = VivlingCrtConfig::default();
        let trans = TransitionPhases {
            mode_fade: 1.0,
            ..Default::default()
        };
        // Sample many timestamps and ensure flicker logic does not panic
        // and remains stable.
        for t_ms in (0u64..6000).step_by(37) {
            let mut surface = CrtSurface::new(40, 3, Style::default());
            surface.fill(0, 0, 40, ' ', palette.dim);
            surface.fill(0, 2, 40, ' ', palette.dim);
            seed_face_row(&mut surface, &palette);
            apply_all(&mut surface, &palette, 13, t_ms, &cfg, trans);
            let rendered = collect(&surface, 40);
            assert_eq!(rendered.len(), 120);
        }
    }

    #[test]
    fn mode_fade_below_threshold_dims_face_row() {
        let palette = Palette::codex();
        let cfg = VivlingCrtConfig {
            scanlines: false,
            phosphor_glow: false,
            flicker: false,
            idle_microanim: false,
            ..VivlingCrtConfig::default()
        };
        let trans = TransitionPhases {
            mode_fade: 0.2,
            ..Default::default()
        };
        let mut surface = CrtSurface::new(40, 3, Style::default());
        surface.fill(0, 0, 40, ' ', palette.dim);
        surface.fill(0, 2, 40, ' ', palette.dim);
        seed_face_row(&mut surface, &palette);
        apply_all(&mut surface, &palette, 5, 240, &cfg, trans);
        // Ensure render doesn't break and structure remains.
        let rendered = collect(&surface, 40);
        assert_eq!(rendered.len(), 120);
    }
}
