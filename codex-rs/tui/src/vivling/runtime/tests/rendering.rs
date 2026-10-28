use super::common::*;
use crate::vivling::model::VivlingGeneVector;
use crate::vivling::runtime::render::compose_line_layout_row;

#[test]
fn active_footer_pose_changes_while_task_running() {
    let mut vivling = Vivling::unavailable();
    vivling.state = Some(seeded_state());
    vivling.configure_runtime(FrameRequester::test_dummy(), true);
    vivling.set_task_running(true);

    let state = vivling.visible_state().expect("hatched state");
    let sprite = vivling.current_sprite(state, Instant::now());
    assert_ne!(sprite, species_for_id(&state.species).ascii_baby);
}

#[test]
fn footer_pose_animates_while_visible_and_idle() {
    let mut vivling = Vivling::unavailable();
    vivling.state = Some(seeded_state());
    vivling.configure_runtime(FrameRequester::test_dummy(), true);

    // codex-vl: il clock della pose parte dal percorso di produzione
    // (set_task_running → mark_recent_activity); il render è read-only
    // (salvo CrtAnimationLedger, v. nota di perimetro) e non inizializza
    // più `active_started_at` al primo draw.
    vivling.set_task_running(true);
    vivling.set_task_running(false);

    let state = vivling.visible_state().expect("hatched state");
    let start = Instant::now();
    let first = vivling.current_sprite(state, start);
    let second = vivling.current_sprite(state, start + ACTIVE_FOOTER_FRAME_INTERVAL);
    assert_ne!(first, second);
}

#[test]
fn static_footer_pose_used_when_animations_disabled() {
    let mut vivling = Vivling::unavailable();
    vivling.state = Some(seeded_state());
    vivling.configure_runtime(FrameRequester::test_dummy(), false);
    vivling.set_task_running(true);

    let state = vivling.visible_state().expect("hatched state");
    let sprite = vivling.current_sprite(state, Instant::now());
    assert_eq!(sprite, species_for_id(&state.species).ascii_baby);
}

#[test]
fn render_keeps_vivling_line_shape() {
    let mut vivling = Vivling::unavailable();
    vivling.state = Some(seeded_state());
    vivling.configure_runtime(FrameRequester::test_dummy(), true);
    vivling.set_task_running(true);

    let area = Rect::new(0, 0, 80, 3);
    let mut buf = Buffer::empty(area);
    vivling.render(area, &mut buf);
    let rendered = buf
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(rendered.contains("("));
    assert!(rendered.contains(")") || rendered.contains(">"));
    assert!(!rendered.contains("L01"));
    assert!(!rendered.contains("EN"));
    assert!(!rendered.contains("watching"));
    assert!(!rendered.contains("focus"));
    assert_eq!(vivling.desired_height(80), 3);
}

#[test]
fn animation_text_remains_volatile_and_never_updates_saved_last_message() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());
    let original = vivling
        .state
        .as_ref()
        .and_then(|state| state.last_message.clone())
        .expect("last message");

    vivling.set_animation_text("is counting sparks".to_string());
    vivling
        .save_state()
        .expect("save with volatile animation text");

    let reloaded = configured_vivling(temp.path());
    let state = reloaded.state.as_ref().expect("reloaded state");
    assert_eq!(state.last_message.as_deref(), Some(original.as_str()));
    assert!(reloaded.shadow.animation_text.is_none());
}

#[test]
fn animation_text_expires_without_touching_saved_last_message() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());
    let original = vivling
        .state
        .as_ref()
        .and_then(|state| state.last_message.clone())
        .expect("last message");
    let now = Instant::now();

    vivling.set_animation_text_at("working now".to_string(), now);
    assert_eq!(
        vivling.current_animation_text_at(now).as_deref(),
        Some("working now")
    );

    assert!(
        vivling
            .current_animation_text_at(now + ANIMATION_TEXT_TTL)
            .is_none()
    );
    // codex-vl: il render è read-only (salvo CrtAnimationLedger, v.
    // nota di perimetro) — la pulizia dell'expiry vive in Vivling::tick,
    // quindi il test la guida col percorso di produzione.
    vivling.tick(now + ANIMATION_TEXT_TTL);
    assert!(vivling.shadow.animation_text.is_none());
    assert_eq!(
        vivling
            .state
            .as_ref()
            .and_then(|state| state.last_message.as_deref()),
        Some(original.as_str())
    );
}

#[test]
fn status_and_card_include_individual_gene_surface() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = hatched_vivling(temp.path());
    let mut state = vivling.state.clone().expect("state");
    state.gene_vector = VivlingGeneVector {
        affinity_mod: [1.12, 0.96, 1.03, 1.18],
        curiosity: 72,
        caution: 66,
        sociability: 40,
        patience: 82,
        brain_potential: 1.11,
    };
    vivling.state = Some(state);
    vivling.save_state().expect("save gene state");

    let status = match vivling
        .command(VivlingAction::Status, temp.path())
        .expect("status")
    {
        VivlingCommandOutcome::Message(message) => message,
        other => panic!("unexpected outcome: {other:?}"),
    };
    assert!(status.contains("genes BLD +12%"));
    assert!(status.contains("temperament patient, curious, cautious"));
    assert!(status.contains("brain potential high"));

    let card = match vivling
        .command(VivlingAction::Card, temp.path())
        .expect("card")
    {
        VivlingCommandOutcome::OpenCard(card) => card,
        other => panic!("unexpected outcome: {other:?}"),
    };
    let rendered = card.narrow_lines.join("\n");
    assert!(rendered.contains("Genes BLD +12%"));
    assert!(rendered.contains("Temperament patient, curious, cautious"));
    assert!(rendered.contains("brain potential high"));
}

#[test]
fn line_layout_uses_a_single_row() {
    let mut vivling = Vivling::unavailable();
    vivling.state = Some(seeded_state());
    vivling.ui_layout = crate::vl::crt::VivlingLayout::Line;
    vivling.configure_runtime(FrameRequester::test_dummy(), true);
    vivling.set_task_running(true);
    assert_eq!(vivling.desired_height(80), 1);
    // Line layout threshold is exactly 5 columns:
    assert_eq!(vivling.desired_height(5), 1);
    assert_eq!(vivling.desired_height(4), 0);

    // Full layout threshold remains 12 columns:
    vivling.ui_layout = crate::vl::crt::VivlingLayout::Full;
    assert_eq!(vivling.desired_height(12), 3);
    assert_eq!(vivling.desired_height(11), 0);
}

#[test]
fn line_layout_renders_within_80_and_60_columns() {
    for width in [60, 80] {
        let mut vivling = Vivling::unavailable();
        vivling.state = Some(seeded_state());
        vivling.ui_layout = crate::vl::crt::VivlingLayout::Line;
        vivling.configure_runtime(FrameRequester::test_dummy(), true);
        vivling.set_task_running(true);

        let area = Rect::new(0, 0, width, 1);
        let mut buf = Buffer::empty(area);
        vivling.render(area, &mut buf);
        let rendered: String = buf.content.iter().map(|cell| cell.symbol()).collect();
        assert!(
            rendered.chars().count() <= width as usize,
            "row exceeds {width} columns: {rendered:?}"
        );
        assert!(
            rendered.contains('●') || rendered.contains('·'),
            "the status dot must be present in {rendered:?}"
        );
    }
}

#[test]
fn line_layout_blinks_slowly_with_animations() {
    let mut vivling = Vivling::unavailable();
    vivling.state = Some(seeded_state());
    vivling.ui_layout = crate::vl::crt::VivlingLayout::Line;
    vivling.configure_runtime(FrameRequester::test_dummy(), true);
    assert_eq!(
        vivling.schedule_animation_wake(Instant::now()),
        Some(std::time::Duration::from_millis(1000))
    );
}

#[test]
fn line_layout_schedules_no_frame_without_animations() {
    let mut vivling = Vivling::unavailable();
    vivling.state = Some(seeded_state());
    vivling.ui_layout = crate::vl::crt::VivlingLayout::Line;
    vivling.configure_runtime(FrameRequester::test_dummy(), false);
    assert_eq!(vivling.schedule_animation_wake(Instant::now()), None);
}

#[test]
fn line_layout_renders_at_all_canonical_widths() {
    for width in [4, 5, 6, 7, 11, 12, 20, 50, 120] {
        let mut vivling = Vivling::unavailable();
        vivling.state = Some(seeded_state());
        vivling.ui_layout = crate::vl::crt::VivlingLayout::Line;
        vivling.configure_runtime(FrameRequester::test_dummy(), true);
        vivling.set_task_running(true);

        let area = Rect::new(0, 0, width, 1);
        let mut buf = Buffer::empty(area);
        vivling.render(area, &mut buf);
        let rendered: String = buf
            .content
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect();

        if width < 5 {
            assert_eq!(
                rendered,
                " ".repeat(width as usize),
                "width {width} must not draw"
            );
            assert_eq!(vivling.desired_height(width), 0);
        } else {
            assert_eq!(vivling.desired_height(width), 1);
            // The 5-column face always stays at the start:
            assert!(
                rendered.starts_with(".-<-.") || rendered.starts_with(".-<-'"),
                "width {width} must start with face: {rendered:?}"
            );
            // Under 8 columns the dot is omitted; at 8 columns or wider the dot is present:
            if width >= 8 {
                assert!(
                    rendered.contains('●') || rendered.contains('·'),
                    "width {width} must have dot: {rendered:?}"
                );
            }
        }
    }
}

#[test]
fn full_layout_threshold_at_11_and_12() {
    let mut vivling = Vivling::unavailable();
    vivling.state = Some(seeded_state());
    vivling.ui_layout = crate::vl::crt::VivlingLayout::Full;
    vivling.configure_runtime(FrameRequester::test_dummy(), true);
    vivling.set_task_running(true);

    assert_eq!(vivling.desired_height(11), 0);
    assert_eq!(vivling.desired_height(12), 3);

    let mut buf11 = Buffer::empty(Rect::new(0, 0, 11, 3));
    vivling.render(Rect::new(0, 0, 11, 3), &mut buf11);
    let content11: String = buf11
        .content
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect();
    assert_eq!(content11, " ".repeat(33));

    let mut buf12 = Buffer::empty(Rect::new(0, 0, 12, 3));
    vivling.render(Rect::new(0, 0, 12, 3), &mut buf12);
    let content12: String = buf12
        .content
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect();
    assert_ne!(content12, " ".repeat(36));
}

#[test]
fn line_layout_face_cells_and_color_containment() {
    // 1. Ambient policy test: inspect the real color policy applied by the product
    let mut vivling = Vivling::unavailable();
    vivling.state = Some(seeded_state());
    vivling.ui_layout = crate::vl::crt::VivlingLayout::Line;
    vivling.configure_runtime(FrameRequester::test_dummy(), true);
    vivling.set_task_running(true);

    let area = Rect::new(0, 0, 60, 1);
    let mut buf = Buffer::empty(area);
    vivling.render(area, &mut buf);

    let expected_face_style = if crate::terminal_palette::effective_stdout_color_level()
        != crate::terminal_palette::StdoutColorLevel::Unknown
    {
        crate::vl::crt::Palette::codex().face
    } else {
        ratatui::style::Style::default()
    };

    // First 5 cells must match the expected policy face style:
    for x in 0..5 {
        let cell = buf.cell((x, 0)).expect("cell in bounds");
        if let Some(expected_fg) = expected_face_style.fg {
            assert_eq!(
                cell.fg, expected_fg,
                "face cell at {x} must have expected face color"
            );
            assert!(
                cell.modifier.contains(ratatui::style::Modifier::BOLD),
                "face cell at {x} must be bold"
            );
        } else {
            assert_eq!(
                cell.fg,
                ratatui::style::Color::Reset,
                "face cell at {x} must be uncolored when color level is unknown"
            );
            assert!(
                !cell.modifier.contains(ratatui::style::Modifier::BOLD),
                "face cell at {x} must not be bold when uncolored"
            );
        }
    }

    // Cells after the face must NOT be LightYellow face color:
    let face_palette = crate::vl::crt::Palette::codex().face;
    for x in 5..60 {
        let cell = buf.cell((x, 0)).expect("cell in bounds");
        assert_ne!(
            Some(cell.fg),
            face_palette.fg,
            "non-face cell at {x} must not leak face color"
        );
    }

    // 2. Explicit colored terminal injection: test with truecolor enabled
    crate::terminal_palette::with_test_default_colors(
        crate::terminal_probe::DefaultColors {
            fg: (238, 238, 238),
            bg: (17, 17, 17),
        },
        || {
            let mut vivling = Vivling::unavailable();
            vivling.state = Some(seeded_state());
            vivling.ui_layout = crate::vl::crt::VivlingLayout::Line;
            vivling.configure_runtime(FrameRequester::test_dummy(), true);
            vivling.set_task_running(true);

            let area = Rect::new(0, 0, 60, 1);
            let mut buf = Buffer::empty(area);
            vivling.render(area, &mut buf);

            // First 5 cells must have LightYellow face color and bold:
            for x in 0..5 {
                let cell = buf.cell((x, 0)).expect("cell in bounds");
                assert_eq!(
                    Some(cell.fg),
                    face_palette.fg,
                    "colored face cell at {x} must have face color"
                );
                assert!(
                    cell.modifier.contains(ratatui::style::Modifier::BOLD),
                    "colored face cell at {x} must be bold"
                );
            }

            // Cells after the face must NOT leak face color:
            for x in 5..60 {
                let cell = buf.cell((x, 0)).expect("cell in bounds");
                assert_ne!(
                    Some(cell.fg),
                    face_palette.fg,
                    "non-face cell at {x} must not leak face color"
                );
            }
        },
    );
}

#[test]
fn line_layout_wide_and_combining_characters() {
    let face = ".-o-.";
    let dot = "●";
    // CJK characters (each 2 display columns)
    let row_cjk = compose_line_layout_row(20, face, "観察中", "好奇心", dot);
    assert!(
        row_cjk.starts_with(face),
        "must start with face: {row_cjk:?}"
    );
    assert!(row_cjk.ends_with(dot), "must end with dot: {row_cjk:?}");
    assert!(crate::width::display_width(&row_cjk) <= 20);

    // Combining diacritics (0 display width)
    let row_combining = compose_line_layout_row(20, face, "re\u{0301}sume\u{0301}", "calm", dot);
    assert!(
        row_combining.starts_with(face),
        "must start with face: {row_combining:?}"
    );
    assert!(
        row_combining.ends_with(dot),
        "must end with dot: {row_combining:?}"
    );
    assert!(crate::width::display_width(&row_combining) <= 20);
}

#[test]
fn line_layout_animates_with_injected_time() {
    let mut vivling = Vivling::unavailable();
    vivling.state = Some(seeded_state());
    vivling.ui_layout = crate::vl::crt::VivlingLayout::Line;
    vivling.configure_runtime(FrameRequester::test_dummy(), true);
    vivling.set_task_running(true);

    let t0 = Instant::now();
    vivling.shadow.active_started_at = Some(t0);

    let area = Rect::new(0, 0, 20, 1);
    let mut buf0 = Buffer::empty(area);
    vivling.render_line_layout(area, &mut buf0, vivling.visible_state().unwrap(), t0);
    let s0: String = buf0
        .content
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect();
    assert!(s0.starts_with(".-<-."), "t0 must be frame A: {s0:?}");

    let t1 = t0 + std::time::Duration::from_secs(1);
    let mut buf1 = Buffer::empty(area);
    vivling.render_line_layout(area, &mut buf1, vivling.visible_state().unwrap(), t1);
    let s1: String = buf1
        .content
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect();
    assert!(s1.starts_with(".-<-'"), "t1 must be frame B: {s1:?}");

    let t2 = t0 + std::time::Duration::from_secs(2);
    let mut buf2 = Buffer::empty(area);
    vivling.render_line_layout(area, &mut buf2, vivling.visible_state().unwrap(), t2);
    let s2: String = buf2
        .content
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect();
    assert!(s2.starts_with(".-<-."), "t2 must wrap to frame A: {s2:?}");
}

#[test]
fn vivling_layout_config_roundtrip() {
    let temp = TempDir::new().expect("tempdir");
    let home = temp.path();

    // Default is Full
    assert_eq!(
        crate::vl::crt::VivlingLayout::load_from_codex_home(home),
        crate::vl::crt::VivlingLayout::Full
    );

    // Save Line
    crate::vl::crt::VivlingLayout::save_layout_to_codex_home(
        home,
        crate::vl::crt::VivlingLayout::Line,
    )
    .expect("save line");
    assert_eq!(
        crate::vl::crt::VivlingLayout::load_from_codex_home(home),
        crate::vl::crt::VivlingLayout::Line
    );

    // Save Full
    crate::vl::crt::VivlingLayout::save_layout_to_codex_home(
        home,
        crate::vl::crt::VivlingLayout::Full,
    )
    .expect("save full");
    assert_eq!(
        crate::vl::crt::VivlingLayout::load_from_codex_home(home),
        crate::vl::crt::VivlingLayout::Full
    );
}
