use super::common::*;

#[test]
fn shadow_phase_agrees_with_task_running_flag() {
    let temp = TempDir::new().expect("tempdir");
    let vivling = configured_vivling(temp.path());
    assert!(!vivling.task_running.get());
    assert!(!vivling.lifecycle.borrow().is_task_running());

    vivling.set_task_running(true);
    assert!(vivling.task_running.get());
    assert!(
        vivling.lifecycle.borrow().is_task_running(),
        "shadow concorda con true"
    );

    vivling.set_task_running(false);
    assert!(!vivling.task_running.get());
    assert!(
        !vivling.lifecycle.borrow().is_task_running(),
        "shadow concorda con false"
    );
}

#[test]
fn expression_gate_is_singular_and_orthogonal_to_phase() {
    use crate::vivling::runtime::ExpressionKind;
    let temp = TempDir::new().expect("tempdir");
    let vivling = configured_vivling(temp.path());
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
