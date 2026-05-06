use super::common::*;

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
    let vivling = hatched_vivling(temp.path());
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
    assert!(reloaded.animation_text.borrow().is_none());
}

#[test]
fn animation_text_expires_without_touching_saved_last_message() {
    let temp = TempDir::new().expect("tempdir");
    let vivling = hatched_vivling(temp.path());
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
    assert!(vivling.animation_text.borrow().is_none());
    assert_eq!(
        vivling
            .state
            .as_ref()
            .and_then(|state| state.last_message.as_deref()),
        Some(original.as_str())
    );
}
