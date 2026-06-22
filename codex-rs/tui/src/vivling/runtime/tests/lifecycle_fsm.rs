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
