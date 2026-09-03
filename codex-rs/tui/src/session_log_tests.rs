use super::*;
use crate::app::loop_controller::summary::LoopManager;
use crate::app::loop_controller::summary::LoopScheduleKind;
use crate::app::loop_controller::summary::LoopTickOutcome;
use crate::app::loop_controller::summary::LoopTickSummary;
use crate::app::loop_controller::summary::NextRun;
use crate::vl::VlEvent;
use codex_protocol::ThreadId;
use pretty_assertions::assert_eq;
use std::sync::Arc;

#[test]
fn inbound_fallback_records_only_variant_names() {
    let directory = tempfile::tempdir().expect("temporary log directory");
    let path = directory.path().join("session.jsonl");
    let logger = SessionLogger::new();
    logger.open(path.clone()).expect("open session log");
    for event in [
        AppEvent::CopySelection {
            text: Arc::from("private plan without parentheses"),
            label: "private label".to_string(),
        },
        AppEvent::CopySelection {
            text: Arc::from("private code(with parentheses)"),
            label: "private label".to_string(),
        },
        AppEvent::ConsolidateProposedPlan("private tuple payload".to_string()),
        AppEvent::SettingsSelectionClosed,
    ] {
        log_inbound_app_event_with(&logger, &event);
    }
    let records = std::fs::read_to_string(path)
        .expect("read session log")
        .lines()
        .map(|line| {
            let mut record: serde_json::Value = serde_json::from_str(line).expect("valid log JSON");
            record.as_object_mut().expect("log object").remove("ts");
            record
        })
        .collect::<Vec<_>>();
    assert_eq!(
        records,
        [
            "CopySelection",
            "CopySelection",
            "ConsolidateProposedPlan",
            "SettingsSelectionClosed"
        ]
        .map(|variant| json!({"dir": "to_tui", "kind": "app_event", "variant": variant}))
    );
}

// The session log keeps the typed loop tick summary payload, not just the
// event variant name.
#[test]
fn loop_tick_summary_records_the_typed_payload() {
    let directory = tempfile::tempdir().expect("temporary log directory");
    let path = directory.path().join("session.jsonl");
    let logger = SessionLogger::new();
    logger.open(path.clone()).expect("open session log");

    let thread_id = ThreadId::new();
    let summary = LoopTickSummary {
        thread_id,
        job_id: "job-log".to_string(),
        label: "nightly".to_string(),
        schedule_kind: LoopScheduleKind::OneShot,
        outcome: LoopTickOutcome::OneShotExpired,
        duration_ms: 0,
        next_run: NextRun::Terminal("past the grace window".to_string()),
        runner: codex_state::LoopRunnerKind::Main,
        runner_reason: "runner=main".to_string(),
        manager: LoopManager::Main,
        manager_reason: "expired".to_string(),
        suspend_reason: None,
        occurrence_ms: None,
        finished_at_ms: 1_700_000_000_000,
    };
    log_inbound_app_event_with(&logger, &AppEvent::Vl(VlEvent::LoopTickSummary { summary }));

    let written = std::fs::read_to_string(&path).expect("session log readable");
    assert!(
        written.contains("\"kind\":\"loop_tick_summary\""),
        "the log must carry the summary payload kind: {written}"
    );
    // Every field of the typed payload, not just a sample: the doc comment
    // on this match arm claims the full payload is kept, so the test must
    // actually pin all of it.
    assert!(
        written.contains(&format!("\"thread_id\":\"{thread_id}\"")),
        "thread_id recorded: {written}"
    );
    assert!(
        written.contains("\"job_id\":\"job-log\""),
        "job_id recorded: {written}"
    );
    assert!(
        written.contains("\"label\":\"nightly\""),
        "label recorded: {written}"
    );
    assert!(
        written.contains("\"schedule_kind\":\"OneShot\""),
        "schedule_kind recorded: {written}"
    );
    assert!(
        written.contains("\"outcome\":\"OneShotExpired\""),
        "outcome recorded: {written}"
    );
    assert!(
        written.contains("\"duration_ms\":0"),
        "duration_ms recorded: {written}"
    );
    assert!(
        written.contains("\"next_run\":\"terminal:past the grace window\""),
        "next_run recorded: {written}"
    );
    assert!(
        written.contains("\"runner\":\"main\""),
        "runner recorded: {written}"
    );
    assert!(
        written.contains("\"runner_reason\":\"runner=main\""),
        "runner_reason recorded: {written}"
    );
    assert!(
        written.contains("\"manager\":\"Main\""),
        "manager recorded: {written}"
    );
    assert!(
        written.contains("\"manager_reason\":\"expired\""),
        "manager_reason recorded: {written}"
    );
    assert!(
        written.contains("\"suspend_reason\":null"),
        "suspend_reason recorded: {written}"
    );
    assert!(
        written.contains("\"occurrence_ms\":null"),
        "occurrence_ms recorded: {written}"
    );
    assert!(
        written.contains("\"finished_at_ms\":1700000000000"),
        "finished_at_ms recorded: {written}"
    );
}
