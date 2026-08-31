use super::common::*;

#[test]
fn task_running_is_tracked_by_phase() {
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = configured_vivling(temp.path());
    assert!(!vivling.is_task_running());
    vivling.set_task_running(true);
    assert!(vivling.is_task_running());
    vivling.set_task_running(false);
    assert!(!vivling.is_task_running());
}

#[test]
fn set_task_running_true_twice_does_not_reset_the_pose_clock() {
    use std::time::Duration;
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = configured_vivling(temp.path());

    vivling.set_task_running(true);
    let started = vivling
        .shadow
        .active_started_at
        .expect("clock seeded at the real task transition");

    // true->true (task gia' in corso, nessuna transizione FSM): il clock
    // della pose NON si riazzera.
    std::thread::sleep(Duration::from_millis(10));
    vivling.set_task_running(true);
    assert_eq!(
        vivling.shadow.active_started_at,
        Some(started),
        "a redundant set_task_running(true) reset the pose clock"
    );

    // Controprova: end_task + nuova transizione reale -> il clock riparte.
    vivling.set_task_running(false);
    std::thread::sleep(Duration::from_millis(5));
    vivling.set_task_running(true);
    let restarted = vivling
        .shadow
        .active_started_at
        .expect("clock re-seeded at the new task transition");
    assert!(
        restarted > started,
        "a real task transition must restart the pose clock"
    );
}

#[test]
fn expression_gate_is_singular_and_orthogonal_to_phase() {
    use crate::vivling::runtime::ExpressionKind;
    let temp = TempDir::new().expect("tempdir");
    let mut vivling = configured_vivling(temp.path());
    // ortogonale: un task in corso non blocca l'apertura di un dispatch
    vivling.set_task_running(true);
    assert!(vivling.try_begin_expression(ExpressionKind::Crt));
    assert!(vivling.expression_in_flight());
    // singolo: secondo dispatch mentre uno è in volo → skip
    assert!(!vivling.try_begin_expression(ExpressionKind::Assist));
    // task ancora running: il gate non ha toccato la fase
    assert!(vivling.is_task_running());
    // clear (fail-safe) riapre
    vivling.finish_expression();
    assert!(!vivling.expression_in_flight());
    assert!(vivling.try_begin_expression(ExpressionKind::Bootstrap));
}
