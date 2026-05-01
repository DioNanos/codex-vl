use super::director::CrtDirector;
use super::effects;
use super::palette::Palette;
use super::scenes::render_scene;
use super::surface::CrtSurface;
use super::tier::CrtTier;
use crate::vivling::Stage;
use crate::vl::VivlingActivity;

pub(crate) struct CrtScene<'a> {
    pub(crate) species_id: &'a str,
    pub(crate) stage: Stage,
    pub(crate) name: &'a str,
    pub(crate) level: u64,
    pub(crate) role: &'a str,
    pub(crate) mood: &'a str,
    pub(crate) energy: i64,
    pub(crate) hunger: i64,
    pub(crate) loop_count: u64,
    pub(crate) sprite: &'a str,
    pub(crate) seed: u32,
    pub(crate) elapsed_ms: u64,
    pub(crate) last_message: Option<&'a str>,
    pub(crate) activity: Option<VivlingActivity>,
    pub(crate) tier: CrtTier,
}

pub(crate) fn render_crt_scene(surface: &mut CrtSurface, scene: &CrtScene<'_>) {
    let palette = Palette::codex();

    let width = surface.width();
    if width == 0 || surface.height() < 3 {
        return;
    }

    surface.fill(0, 0, width, ' ', palette.dim);
    surface.fill(0, 1, width, ' ', palette.dim);
    surface.fill(0, 2, width, ' ', palette.dim);

    let mode = CrtDirector::select(scene, width);
    render_scene(surface, scene, mode, &palette);
    effects::apply_all(surface, &palette, scene.seed, scene.elapsed_ms);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::style::Style;

    #[test]
    fn scene_uses_three_fixed_rows_without_dashboard_numbers() {
        let mut surface = CrtSurface::new(40, 3, Style::default());
        let scene = CrtScene {
            species_id: "syllo",
            stage: Stage::Baby,
            name: "Nilo",
            level: 5,
            role: "builder",
            mood: "curious",
            energy: 73,
            hunger: 74,
            loop_count: 5,
            sprite: "('.')=  .",
            seed: 7,
            elapsed_ms: 240,
            last_message: None,
            activity: None,
            tier: CrtTier::Safe,
        };
        render_crt_scene(&mut surface, &scene);

        let mut buf = Buffer::empty(Rect::new(0, 0, 40, 3));
        surface.render(Rect::new(0, 0, 40, 3), &mut buf);
        let rendered: String = buf.content.iter().map(|cell| cell.symbol()).collect();
        assert!(rendered.contains("(  o  o  )") || rendered.contains("(  -  o  )"));
        assert!(!rendered.contains("L05"));
        assert!(!rendered.contains("EN73"));
        assert!(!rendered.contains("HU74"));
        assert!(!rendered.contains("WK64"));
        assert!(!rendered.contains("watching"));
        assert_eq!(rendered.len(), 120);
    }

    #[test]
    fn scene_keeps_fixed_shape_across_terminal_widths() {
        for width in [8, 12, 18, 24, 40, 80] {
            let mut surface = CrtSurface::new(width, 3, Style::default());
            let scene = CrtScene {
                species_id: "syllo",
                stage: Stage::Baby,
                name: "Nilo",
                level: 5,
                role: "builder",
                mood: "curious",
                energy: 73,
                hunger: 74,
                loop_count: 5,
                sprite: "('.')=  .",
                seed: 7,
                elapsed_ms: 240,
                last_message: None,
                activity: None,
                tier: CrtTier::Safe,
            };
            render_crt_scene(&mut surface, &scene);

            let mut buf = Buffer::empty(Rect::new(0, 0, width, 3));
            surface.render(Rect::new(0, 0, width, 3), &mut buf);
            let rendered: String = buf.content.iter().map(|cell| cell.symbol()).collect();
            assert_eq!(rendered.len(), width as usize * 3);
            assert!(!rendered.contains("watching"));
            assert!(!rendered.contains("focus"));
        }
    }

    #[test]
    fn scene_animates_face_without_scrolling_tape() {
        let first = render_row_at(24, 1, 0);
        let second = render_row_at(24, 1, 360);
        assert_eq!(first.chars().count(), 24);
        assert_eq!(second.chars().count(), 24);
        assert!(first.contains("("));
        assert!(second.contains("("));
        assert!(!first.contains("LOOP05"));
        assert!(!second.contains("LOOP05"));
    }

    fn render_row_at(width: u16, row: u16, elapsed_ms: u64) -> String {
        let mut surface = CrtSurface::new(width, 3, Style::default());
        let scene = CrtScene {
            species_id: "syllo",
            stage: Stage::Baby,
            name: "Nilo",
            level: 5,
            role: "builder",
            mood: "curious",
            energy: 73,
            hunger: 74,
            loop_count: 5,
            sprite: "('.')=  .",
            seed: 7,
            elapsed_ms,
            last_message: None,
            activity: None,
            tier: CrtTier::Safe,
        };
        render_crt_scene(&mut surface, &scene);

        let mut buf = Buffer::empty(Rect::new(0, 0, width, 3));
        surface.render(Rect::new(0, 0, width, 3), &mut buf);
        (0..width).map(|x| buf[(x, row)].symbol()).collect()
    }
}
