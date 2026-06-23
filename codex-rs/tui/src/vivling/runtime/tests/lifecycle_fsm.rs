use super::common::*;

#[test]
fn task_running_is_tracked_by_phase() {
    let temp = TempDir::new().expect("tempdir");
    let vivling = configured_vivling(temp.path());
    assert!(!vivling.is_task_running());
    vivling.set_task_running(true);
    assert!(vivling.is_task_running());
    vivling.set_task_running(false);
    assert!(!vivling.is_task_running());
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
