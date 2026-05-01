use super::assets::art_for;
use super::director::CrtMode;
use super::expressions::ART_WIDTH;
use super::expressions::overlay_accents;
use super::frame::compose_at;
use super::palette::Palette;
use super::scripts;
use super::speech;
use super::surface::CrtSurface;
use super::tier::CrtTier;
use crate::vivling::Stage;
use crate::vl::VivlingActivity;

pub(crate) const TICK_DIVISOR_MS: u64 = 180;
const PANEL_GAP: u16 = 2;
const LEFT_MARGIN_THRESHOLD: u16 = 18;

pub(crate) fn compose_expression(
    surface: &mut CrtSurface,
    mode: CrtMode,
    elapsed_ms: u64,
    palette: &Palette,
    last_message: Option<&str>,
    activity: Option<VivlingActivity>,
    seed: u32,
    tier: CrtTier,
    species_id: &str,
    stage: Stage,
) {
    let width = surface.width();
    if width == 0 || surface.height() < 3 {
        return;
    }
    let tick = elapsed_ms / TICK_DIVISOR_MS;
    let face = art_for(species_id, stage, mode, tier.for_width(width), tick);
    let art_left: u16 = if width >= LEFT_MARGIN_THRESHOLD { 1 } else { 0 };
    let art_right = art_left.saturating_add(ART_WIDTH).min(width);
    compose_at(surface, &face, art_left, palette);

    let panel_x = art_right.saturating_add(PANEL_GAP).min(width);
    let panel_w = width.saturating_sub(panel_x);
    let active_script = activity.is_some_and(|act| act != VivlingActivity::Idle);
    let (drew_script, drew_speech) = if active_script {
        let script_w = if width >= 40 {
            panel_w.min(14)
        } else {
            panel_w
        };
        let drew_script = scripts::draw_activity_script(
            surface, panel_x, script_w, activity, seed, elapsed_ms, palette,
        );
        let bubble_x = panel_x
            .saturating_add(script_w)
            .saturating_add(1)
            .min(width);
        let bubble_w = width.saturating_sub(bubble_x);
        let drew_speech = if width >= 40 {
            speech::draw_bubble(surface, bubble_x, bubble_w, last_message, palette).is_some()
        } else {
            false
        };
        (drew_script, drew_speech)
    } else {
        let bubble_w = speech::bubble_width(last_message, panel_w);
        let script_x = if bubble_w > 0 {
            panel_x
                .saturating_add(bubble_w)
                .saturating_add(1)
                .min(width)
        } else {
            panel_x
        };
        let script_w = width.saturating_sub(script_x);
        let drew_script = scripts::draw_activity_script(
            surface, script_x, script_w, activity, seed, elapsed_ms, palette,
        );
        let drew_speech =
            speech::draw_bubble(surface, panel_x, panel_w, last_message, palette).is_some();
        (drew_script, drew_speech)
    };
    if !drew_speech && !drew_script {
        overlay_accents(surface, mode, tick, palette, art_left, art_right);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::style::Style;

    const MODES: &[CrtMode] = &[
        CrtMode::Idle,
        CrtMode::Thinking,
        CrtMode::Working,
        CrtMode::Alert,
        CrtMode::Tired,
        CrtMode::Hungry,
    ];

    fn render(
        mode: CrtMode,
        width: u16,
        elapsed_ms: u64,
        msg: Option<&str>,
        act: Option<VivlingActivity>,
    ) -> String {
        let palette = Palette::codex();
        let mut surface = CrtSurface::new(width, 3, Style::default());
        surface.fill(0, 0, width, ' ', palette.dim);
        surface.fill(0, 1, width, ' ', palette.dim);
        surface.fill(0, 2, width, ' ', palette.dim);
        compose_expression(
            &mut surface,
            mode,
            elapsed_ms,
            &palette,
            msg,
            act,
            7,
            CrtTier::Safe,
            "syllo",
            Stage::Baby,
        );
        let mut buf = Buffer::empty(Rect::new(0, 0, width, 3));
        surface.render(Rect::new(0, 0, width, 3), &mut buf);
        buf.content.iter().map(|c| c.symbol()).collect()
    }

    #[test]
    fn each_mode_produces_distinct_visual_signature() {
        let signatures: Vec<String> = MODES
            .iter()
            .map(|m| render(*m, 40, 360, None, None))
            .collect();
        for i in 0..signatures.len() {
            for j in (i + 1)..signatures.len() {
                assert_ne!(signatures[i], signatures[j]);
            }
        }
    }

    #[test]
    fn narrow_widths_render_exact_three_rows() {
        for width in [8u16, 12, 18, 24, 40, 80] {
            for mode in MODES {
                let rendered = render(*mode, width, 540, Some("hello world"), None);
                assert_eq!(
                    rendered.len(),
                    width as usize * 3,
                    "{:?} at width {} overflowed",
                    mode,
                    width
                );
            }
        }
    }

    #[test]
    fn face_row_marker_survives_long_message() {
        let rendered = render(
            CrtMode::Working,
            80,
            360,
            Some("a very long story that goes on and on and on indefinitely"),
            None,
        );
        let row1: String = rendered.chars().skip(80).take(80).collect();
        assert!(row1.contains('('));
    }

    #[test]
    fn message_renders_to_the_right_of_face_when_room_allows() {
        let rendered = render(CrtMode::Idle, 60, 0, Some("greets the world"), None);
        assert!(rendered.contains("< greets the world"));
    }

    #[test]
    fn lifecycle_text_is_ignored_when_no_message() {
        let rendered = render(CrtMode::Hungry, 60, 0, None, None);
        assert!(!rendered.contains("*munch*"));
    }

    #[test]
    fn speech_is_omitted_on_narrow_widths() {
        for width in [8u16, 12, 18] {
            let rendered = render(CrtMode::Idle, width, 0, Some("greets"), None);
            assert!(!rendered.contains("greets"));
            assert!(!rendered.contains("*tick*"));
            assert_eq!(rendered.len(), width as usize * 3);
        }
    }

    #[test]
    fn rendering_is_deterministic_for_same_inputs() {
        for mode in MODES {
            let a = render(*mode, 40, 360, Some("hi"), None);
            let b = render(*mode, 40, 360, Some("hi"), None);
            assert_eq!(a, b);
        }
    }

    #[test]
    fn speech_and_activity_can_share_the_strip() {
        let rendered = render(
            CrtMode::Thinking,
            80,
            1000,
            Some("watching completed turns closely"),
            Some(VivlingActivity::Playing),
        );
        assert!(rendered.contains("< watching"));
        assert!(rendered.contains("o"));
        assert!(!rendered.contains("completed turns"));
    }

    #[test]
    fn activity_gets_priority_without_overlapping_bubble() {
        let rendered = render(
            CrtMode::Working,
            40,
            1000,
            Some("watching completed turns closely"),
            Some(VivlingActivity::Working),
        );
        assert!(rendered.contains("[>]"));
        assert!(!rendered.contains("completed turns"));
        assert_eq!(rendered.len(), 120);
    }

    #[test]
    fn rendering_has_no_dashboard_words_or_numeric_labels() {
        const FORBIDDEN: &[&str] = &[
            "MOOD", "LOOP0", "LOOP1", "EN0", "EN1", "EN9", "HU0", "HU9", "BLD", "RVW",
        ];
        for mode in MODES {
            for width in [18u16, 40, 80] {
                let rendered = render(*mode, width, 360, None, None);
                for needle in FORBIDDEN {
                    assert!(
                        !rendered.contains(needle),
                        "{:?} at width {} contained forbidden token {:?}",
                        mode,
                        width,
                        needle
                    );
                }
            }
        }
    }
}
