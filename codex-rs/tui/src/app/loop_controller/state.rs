//! codex-vl loop_controller: free helpers (timing + error wrapping).
//!
//! `loop_state_runtime` remains a `pub(super) async fn` on `impl App`
//! in `mod.rs` because several App methods share that call pattern. It
//! clones the process-owned state handle instead of reopening and
//! migrating SQLite from a consumer path. Only the byte-pure timing and
//! error helpers remain here.

use chrono::TimeZone;

pub(super) fn loop_now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

pub(super) fn loop_state_error(err: anyhow::Error) -> color_eyre::Report {
    color_eyre::eyre::eyre!("{err}")
}

/// Schedule descriptor inputs for [`next_run_at_ms`]. All values come
/// from persisted storage (0930 job + 0933 descriptor); nothing here touches
/// I/O or the clock — `now_ms` is always passed in by the caller.
pub(super) struct SchedulePlan<'a> {
    pub schedule_kind: &'a str,
    pub interval_seconds: i64,
    pub schedule_at: Option<&'a str>,
    pub tz: Option<&'a str>,
    pub one_shot_at_ms: Option<i64>,
}

/// Grace window for an expired, never-claimed one-shot: a rapid restart within 5 minutes still fires the
/// tick; past the grace the occurrence is terminal `expired` (disarm, no
/// late execution, tick summary).
pub(super) const ONE_SHOT_GRACE_MS: i64 = 5 * 60 * 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RetryTickRuntimeState {
    pub next_run_ms: Option<i64>,
    pub pending_tick: bool,
}

/// One transition for every failed or busy tick. One-shots are consumed;
/// recurring schedules retain the claimed occurrence as the pending key.
pub(super) fn retry_tick_runtime_state(
    schedule_kind: &str,
    occurrence_ms: Option<i64>,
) -> RetryTickRuntimeState {
    if schedule_kind == "one_shot" {
        RetryTickRuntimeState {
            next_run_ms: None,
            pending_tick: false,
        }
    } else if occurrence_ms.is_some() {
        RetryTickRuntimeState {
            next_run_ms: occurrence_ms,
            pending_tick: true,
        }
    } else {
        RetryTickRuntimeState {
            next_run_ms: None,
            pending_tick: false,
        }
    }
}

pub(super) fn next_run_after_tick_ms(plan: &SchedulePlan<'_>, now_ms: i64) -> Option<i64> {
    if plan.schedule_kind == "one_shot" {
        None
    } else {
        next_run_at_ms(plan, now_ms)
    }
}

pub(super) fn one_shot_expired(plan: &SchedulePlan<'_>, now_ms: i64) -> bool {
    plan.schedule_kind == "one_shot"
        && plan
            .one_shot_at_ms
            .is_some_and(|at| now_ms > at.saturating_add(ONE_SHOT_GRACE_MS))
}

/// Pure scheduler (the single place computing the next run instant).
/// `interval` keeps today's behaviour (`now + interval`); `at` resolves the
/// next wall-clock HH:MM in the persisted IANA tz (DST-aware: fold picks the
/// first valid instant, gap skips to the next day — both pinned by tests);
/// `one_shot` fires at its epoch-ms instant, stays claimable within the
/// grace window after its instant, and expires (None) past the grace.
/// Returns epoch-ms UTC, or None when the schedule disarms (expired
/// one-shot, invalid schedule input).
pub(super) fn next_run_at_ms(plan: &SchedulePlan<'_>, now_ms: i64) -> Option<i64> {
    match plan.schedule_kind {
        "interval" => Some(now_ms.saturating_add(plan.interval_seconds.saturating_mul(1000))),
        "at" => next_daily_at_ms(plan.schedule_at?, plan.tz?, now_ms),
        "one_shot" => (!one_shot_expired(plan, now_ms))
            .then_some(plan.one_shot_at_ms)
            .flatten(),
        _ => None,
    }
}

/// Next strictly-future wall-clock HH:MM occurrence in `tz_name` (IANA).
fn next_daily_at_ms(hhmm: &str, tz_name: &str, now_ms: i64) -> Option<i64> {
    let (hour, minute) = parse_hhmm(hhmm)?;
    let tz: chrono_tz::Tz = tz_name.parse().ok()?;
    let now_utc = chrono::Utc.timestamp_millis_opt(now_ms).single()?;
    let now_local = now_utc.with_timezone(&tz);
    // At most one DST gap per calendar day can swallow the requested time,
    // so a bounded 0..=2-day search always finds the next occurrence.
    for day_offset in 0..=2i64 {
        let date = now_local.date_naive() + chrono::Duration::days(day_offset);
        let Some(naive) = date.and_hms_opt(hour, minute, 0) else {
            continue;
        };
        match tz.from_local_datetime(&naive) {
            chrono::MappedLocalTime::Single(instant) => {
                if instant.timestamp_millis() > now_ms {
                    return Some(instant.timestamp_millis());
                }
            }
            // DST fold: try both instants in chronological order. If `now`
            // is exactly the first occurrence, the second occurrence is the
            // next valid instant on the same wall-clock day.
            chrono::MappedLocalTime::Ambiguous(first, second) => {
                for instant in [first, second] {
                    if instant.timestamp_millis() > now_ms {
                        return Some(instant.timestamp_millis());
                    }
                }
            }
            // DST gap: the wall-clock time does not exist on this day; the
            // next occurrence is the same HH:MM on the following day.
            chrono::MappedLocalTime::None => continue,
        }
    }
    None
}

fn parse_hhmm(hhmm: &str) -> Option<(u32, u32)> {
    let (hour, minute) = hhmm.trim().split_once(':')?;
    let hour: u32 = hour.trim().parse().ok()?;
    let minute: u32 = minute.trim().parse().ok()?;
    (hour < 24 && minute < 60).then_some((hour, minute))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan<'a>(
        schedule_kind: &'a str,
        interval_seconds: i64,
        schedule_at: Option<&'a str>,
        tz: Option<&'a str>,
        one_shot_at_ms: Option<i64>,
    ) -> SchedulePlan<'a> {
        SchedulePlan {
            schedule_kind,
            interval_seconds,
            schedule_at,
            tz,
            one_shot_at_ms,
        }
    }

    #[test]
    fn interval_keeps_today_now_plus_interval() {
        let plan = plan("interval", 90, None, None, None);
        assert_eq!(next_run_at_ms(&plan, 1_000), Some(91_000));
    }

    #[test]
    fn at_resolves_next_wall_clock_in_tz() {
        // 2026-09-03 08:00Z = 10:00 CEST in Rome; the 09:00 occurrence is in
        // the past, so the next one is tomorrow 09:00 CEST = 07:00Z.
        let plan = plan("at", 60, Some("09:00"), Some("Europe/Rome"), None);
        assert_eq!(
            next_run_at_ms(&plan, 1_788_422_400_000),
            Some(1_788_505_200_000)
        );
    }

    #[test]
    fn at_fold_picks_the_first_valid_instant() {
        // 2026-10-25 00:30Z is 02:30 CEST, right before the fall-back
        // transition: 02:30 is in the fold (00:30Z CEST and 01:30Z CET). The
        // first fold instant equals `now` (not strictly future), so the next
        // occurrence is the second fold instant, 02:30 CET = 01:30Z.
        let plan = plan("at", 60, Some("02:30"), Some("Europe/Rome"), None);
        assert_eq!(
            next_run_at_ms(&plan, 1_792_888_200_000),
            Some(1_792_891_800_000)
        );
    }

    #[test]
    fn at_gap_skips_to_the_next_day() {
        // 2026-03-29 01:30Z is 02:30 CET, inside the spring-forward gap
        // (02:00-02:59 does not exist that day). The next 02:30 is on
        // 2026-03-30 CET = 00:30Z.
        let plan = plan("at", 60, Some("02:30"), Some("Europe/Rome"), None);
        assert_eq!(
            next_run_at_ms(&plan, 1_774_747_800_000),
            Some(1_774_830_600_000)
        );
    }

    #[test]
    fn at_invalid_tz_or_time_disarms() {
        assert_eq!(
            next_run_at_ms(
                &plan("at", 60, Some("09:00"), Some("Not/AZone"), None),
                1_000
            ),
            None
        );
        assert_eq!(
            next_run_at_ms(
                &plan("at", 60, Some("25:00"), Some("Europe/Rome"), None),
                1_000
            ),
            None
        );
    }

    #[test]
    fn one_shot_fires_once_then_expires() {
        let plan = plan("one_shot", 60, None, None, Some(5_000));
        assert_eq!(next_run_at_ms(&plan, 1_000), Some(5_000));
        // Inside the grace window the occurrence stays claimable (rapid
        // restart: the timer fires with delay 0 and the tick executes).
        assert_eq!(next_run_at_ms(&plan, 5_000), Some(5_000));
        assert_eq!(
            next_run_at_ms(&plan, 5_000 + ONE_SHOT_GRACE_MS),
            Some(5_000)
        );
        // Past the grace: terminal expired, never rescheduled.
        assert_eq!(next_run_at_ms(&plan, 5_000 + ONE_SHOT_GRACE_MS + 1), None);
    }

    #[test]
    fn retry_transition_is_shared_across_runner_paths() {
        let occurrence_ms = Some(5_000);
        for runner in ["main", "child_agent", "vivling"] {
            assert_eq!(
                retry_tick_runtime_state("interval", occurrence_ms),
                RetryTickRuntimeState {
                    next_run_ms: occurrence_ms,
                    pending_tick: true,
                },
                "recurring failure/busy transition diverged for {runner}"
            );
            assert_eq!(
                retry_tick_runtime_state("one_shot", occurrence_ms),
                RetryTickRuntimeState {
                    next_run_ms: None,
                    pending_tick: false,
                },
                "one-shot failure/busy transition diverged for {runner}"
            );
        }
        assert_eq!(
            retry_tick_runtime_state("interval", None),
            RetryTickRuntimeState {
                next_run_ms: None,
                pending_tick: false,
            }
        );
    }

    #[test]
    fn successful_one_shot_is_terminal_and_at_uses_wall_clock_scheduler() {
        let one_shot = plan("one_shot", 60, None, None, Some(5_000));
        assert_eq!(next_run_after_tick_ms(&one_shot, 5_000), None);

        let at = plan("at", 60, Some("09:00"), Some("Europe/Rome"), None);
        assert_eq!(
            next_run_after_tick_ms(&at, 1_788_422_400_000),
            Some(1_788_505_200_000)
        );
    }

    #[test]
    fn expired_one_shot_is_terminal_before_claim_or_schedule() {
        let plan = plan("one_shot", 60, None, None, Some(5_000));
        assert!(!one_shot_expired(&plan, 5_000 + ONE_SHOT_GRACE_MS));
        assert!(one_shot_expired(&plan, 5_000 + ONE_SHOT_GRACE_MS + 1));
        assert_eq!(next_run_at_ms(&plan, 5_000 + ONE_SHOT_GRACE_MS + 1), None);
    }
}
