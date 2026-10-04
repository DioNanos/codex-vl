use pretty_assertions::assert_eq;

use super::LifecycleFault;
use super::check_explicit_update;

#[tokio::test]
async fn failed_trigger_still_stops_backends_and_joins_control() {
    let reports = check_explicit_update(LifecycleFault::Trigger).await;
    assert_eq!(reports.len(), 1);
    let report = &reports[0];
    report.assert_cleanup();
    assert!(report.backend_stop_attempted && report.updater_stop_attempted);
    assert!(report.control_joined, "control join missing: {report:?}");
    assert!(!report.control_panicked);
    assert_eq!(
        report.outcomes,
        vec![
            Err("injected trigger verification failure".into()),
            Err("injected trigger verification failure".into()),
            Err("injected trigger verification failure".into()),
            Ok(()),
        ]
    );
}

#[tokio::test]
async fn control_panic_is_joined_after_backend_cleanup() {
    let reports = check_explicit_update(LifecycleFault::ControlPanic).await;
    assert_eq!(reports.len(), 1);
    let report = &reports[0];
    report.assert_cleanup();
    assert!(report.backend_stop_attempted && report.updater_stop_attempted);
    assert!(
        report.control_joined && report.control_panicked,
        "panic join missing: {report:?}"
    );
    assert_eq!(report.outcomes.len(), 5);
    assert!(report.outcomes[..4].iter().all(Result::is_ok), "{report:?}");
    let error = report.outcomes[4]
        .as_ref()
        .expect_err("control panic must be collected");
    assert!(error.starts_with("control server task:"), "{error}");
    assert!(error.contains("injected control panic"), "{error}");
}
