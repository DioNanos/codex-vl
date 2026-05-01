use super::objects::{CrtObject, draw_object};
use super::palette::Palette;
use super::surface::CrtSurface;
use crate::vl::VivlingActivity;

const MIN_SCRIPT_WIDTH: u16 = 6;

pub(crate) fn draw_activity_script(
    surface: &mut CrtSurface,
    x: u16,
    width: u16,
    activity: Option<VivlingActivity>,
    seed: u32,
    elapsed_ms: u64,
    palette: &Palette,
) -> bool {
    if width < MIN_SCRIPT_WIDTH || surface.height() < 3 {
        return false;
    }
    match activity {
        Some(VivlingActivity::Eating) => {
            draw_object(surface, x, width, CrtObject::Snack, elapsed_ms, palette)
        }
        Some(VivlingActivity::Playing) => draw_object(
            surface,
            x,
            width,
            play_object(seed, elapsed_ms),
            elapsed_ms,
            palette,
        ),
        Some(VivlingActivity::Sleeping) => draw_object(
            surface,
            x,
            width,
            rest_object(seed, elapsed_ms),
            elapsed_ms,
            palette,
        ),
        Some(VivlingActivity::Working) => draw_object(
            surface,
            x,
            width,
            work_object(seed, elapsed_ms),
            elapsed_ms,
            palette,
        ),
        _ => false,
    }
}

fn play_object(seed: u32, elapsed_ms: u64) -> CrtObject {
    match ((seed as u64 + elapsed_ms / 4_000) % 3) as u8 {
        0 => CrtObject::Ball,
        1 => CrtObject::Cube,
        _ => CrtObject::CrtOrb,
    }
}

fn rest_object(seed: u32, elapsed_ms: u64) -> CrtObject {
    match ((seed as u64 + elapsed_ms / 5_000) % 3) as u8 {
        0 => CrtObject::Blanket,
        1 => CrtObject::Nest,
        _ => CrtObject::Pillow,
    }
}

fn work_object(seed: u32, elapsed_ms: u64) -> CrtObject {
    match ((seed as u64 + elapsed_ms / 4_500) % 7) as u8 {
        0 => CrtObject::Terminal,
        1 => CrtObject::Logbook,
        2 => CrtObject::TestChip,
        3 => CrtObject::MemoryShard,
        4 => CrtObject::ScanLens,
        5 => CrtObject::SignalKey,
        _ => CrtObject::LogLantern,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::style::Style;

    fn render(activity: VivlingActivity, width: u16, elapsed_ms: u64) -> String {
        let palette = Palette::codex();
        let mut surface = CrtSurface::new(width, 3, Style::default());
        draw_activity_script(
            &mut surface,
            0,
            width,
            Some(activity),
            7,
            elapsed_ms,
            &palette,
        );
        let mut buf = Buffer::empty(Rect::new(0, 0, width, 3));
        surface.render(Rect::new(0, 0, width, 3), &mut buf);
        buf.content.iter().map(|c| c.symbol()).collect()
    }

    #[test]
    fn playing_uses_visual_playfield_not_words() {
        let rendered = render(VivlingActivity::Playing, 24, 1000);
        assert!(rendered.contains("o") || rendered.contains("[") || rendered.contains("@"));
        assert!(!rendered.contains("hop"));
        assert!(!rendered.contains("bounce"));
        assert!(!rendered.contains("wiggle"));
    }

    #[test]
    fn eating_uses_crumbs_not_text() {
        let rendered = render(VivlingActivity::Eating, 24, 1000);
        assert!(rendered.contains("*"));
        assert!(!rendered.contains("munch"));
        assert!(!rendered.contains("chomp"));
        assert!(!rendered.contains("nom"));
    }

    #[test]
    fn working_uses_terminal_object_not_words() {
        let rendered = render(VivlingActivity::Working, 24, 1000);
        assert!(rendered.contains("[") || rendered.contains("/___\\"));
        assert!(!rendered.contains("work"));
        assert!(!rendered.contains("loop"));
    }

    #[test]
    fn scripts_vary_objects_by_seed_without_words() {
        let play_a = render(VivlingActivity::Playing, 24, 0);
        let play_b = {
            let palette = Palette::codex();
            let mut surface = CrtSurface::new(24, 3, Style::default());
            draw_activity_script(
                &mut surface,
                0,
                24,
                Some(VivlingActivity::Playing),
                8,
                0,
                &palette,
            );
            let mut buf = Buffer::empty(Rect::new(0, 0, 24, 3));
            surface.render(Rect::new(0, 0, 24, 3), &mut buf);
            buf.content.iter().map(|c| c.symbol()).collect::<String>()
        };
        assert_ne!(play_a, play_b);
        assert!(!play_a.contains("play"));
        assert!(!play_b.contains("play"));
    }
}
