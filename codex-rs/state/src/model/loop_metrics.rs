use super::LoopDelegationStrategy;
use serde::Deserialize;
use serde::Serialize;

pub const LOOP_METRICS_WINDOW: usize = 20;
pub const LOOP_MANAGE_SUGGEST_TICKS: i64 = 5;
pub const LOOP_MANAGE_TICKS: i64 = 15;
pub const LOOP_MANAGE_BOND_SUGGEST: u8 = 60;
pub const LOOP_MANAGE_BOND: u8 = 75;
pub const LOOP_MANAGE_CLEAN_RATIO: u64 = 3;
pub const LOOP_MANAGE_CLEAN_STREAK: usize = 5;
pub const LOOP_MANAGE_COOLDOWN_MS: i64 = 60 * 60 * 1000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoopResultEntry {
    pub ts_ms: i64,
    pub status: String,
    #[serde(default)]
    pub clean: bool,
    #[serde(default)]
    pub noisy: bool,
    #[serde(default)]
    pub blocked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RecentLoopResults {
    pub v: u8,
    pub entries: Vec<LoopResultEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedRecentLoopResults {
    pub entries: Vec<LoopResultEntry>,
    pub diagnostic: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LoopMetrics {
    pub clean_submissions: u64,
    pub noisy_churn: u64,
    pub blocked_runs: u64,
}

impl LoopMetrics {
    pub fn from_entries(entries: &[LoopResultEntry]) -> Self {
        Self {
            clean_submissions: entries.iter().filter(|entry| entry.clean).count() as u64,
            noisy_churn: entries.iter().filter(|entry| entry.noisy).count() as u64,
            blocked_runs: entries.iter().filter(|entry| entry.blocked).count() as u64,
        }
    }

    pub fn clean_streak(entries: &[LoopResultEntry]) -> usize {
        entries
            .iter()
            .rev()
            .take_while(|entry| entry.clean && !entry.noisy && !entry.blocked)
            .count()
    }
}

impl RecentLoopResults {
    pub fn new(entries: Vec<LoopResultEntry>) -> Self {
        Self {
            v: 1,
            entries: cap_entries(entries),
        }
    }

    pub fn to_json(&self) -> anyhow::Result<String> {
        Ok(serde_json::to_string(self)?)
    }
}

#[derive(Debug, Deserialize)]
struct RecentLoopResultsInput {
    v: Option<u8>,
    #[serde(default)]
    entries: Vec<LoopResultEntry>,
}

pub fn parse_recent_results(raw: &str) -> ParsedRecentLoopResults {
    let input = match serde_json::from_str::<RecentLoopResultsInput>(raw) {
        Ok(input) => input,
        Err(error) => {
            return ParsedRecentLoopResults {
                entries: Vec::new(),
                diagnostic: Some(format!("malformed recent_results_json: {error}")),
            };
        }
    };

    match input.v {
        Some(1) => ParsedRecentLoopResults {
            entries: cap_entries(input.entries),
            diagnostic: None,
        },
        Some(version) => ParsedRecentLoopResults {
            entries: Vec::new(),
            diagnostic: Some(format!(
                "unsupported recent_results_json version: {version}"
            )),
        },
        None => ParsedRecentLoopResults {
            entries: Vec::new(),
            diagnostic: None,
        },
    }
}

pub fn loop_management_strategy(
    ticks_managed: i64,
    bond: u8,
    metrics: LoopMetrics,
) -> LoopDelegationStrategy {
    if ticks_managed >= LOOP_MANAGE_TICKS
        && bond >= LOOP_MANAGE_BOND
        && metrics.clean_submissions > metrics.noisy_churn.saturating_mul(LOOP_MANAGE_CLEAN_RATIO)
        && metrics.blocked_runs <= metrics.clean_submissions
    {
        LoopDelegationStrategy::Manage
    } else if ticks_managed >= LOOP_MANAGE_SUGGEST_TICKS && bond >= LOOP_MANAGE_BOND_SUGGEST {
        LoopDelegationStrategy::Suggest
    } else {
        LoopDelegationStrategy::Observe
    }
}

pub fn has_consecutive_blocked(entries: &[LoopResultEntry], count: usize) -> bool {
    count > 0
        && entries.len() >= count
        && entries.iter().rev().take(count).all(|entry| entry.blocked)
}

pub fn can_resume_after_suspension(
    entries: &[LoopResultEntry],
    cooldown_until_ms: Option<i64>,
    now_ms: i64,
) -> bool {
    LoopMetrics::clean_streak(entries) >= LOOP_MANAGE_CLEAN_STREAK
        && cooldown_until_ms.is_none_or(|cooldown| cooldown <= now_ms)
}

fn cap_entries(mut entries: Vec<LoopResultEntry>) -> Vec<LoopResultEntry> {
    if entries.len() > LOOP_METRICS_WINDOW {
        let keep_from = entries.len() - LOOP_METRICS_WINDOW;
        entries.drain(..keep_from);
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(clean: bool, noisy: bool, blocked: bool) -> LoopResultEntry {
        LoopResultEntry {
            ts_ms: 1,
            status: "test".to_string(),
            clean,
            noisy,
            blocked,
        }
    }

    #[test]
    fn legacy_array_or_missing_version_fails_closed_to_empty() {
        assert_eq!(parse_recent_results("[]").entries, Vec::new());
        assert_eq!(
            parse_recent_results(r#"{"entries":[]}"#).entries,
            Vec::new()
        );
    }

    #[test]
    fn malformed_and_unknown_versions_return_diagnostics_and_empty_metrics() {
        assert!(parse_recent_results("not-json").diagnostic.is_some());
        let parsed = parse_recent_results(r#"{"v":2,"entries":[{"clean":true}]}"#);
        assert!(parsed.entries.is_empty());
        assert!(parsed.diagnostic.is_some());
    }

    #[test]
    fn typed_results_are_capped_to_the_latest_twenty_entries() {
        let entries = (0..25)
            .map(|ts_ms| LoopResultEntry {
                ts_ms,
                status: "done".to_string(),
                clean: true,
                noisy: false,
                blocked: false,
            })
            .collect();
        let parsed = parse_recent_results(&RecentLoopResults::new(entries).to_json().unwrap());
        assert_eq!(parsed.entries.len(), LOOP_METRICS_WINDOW);
        assert_eq!(parsed.entries.first().map(|entry| entry.ts_ms), Some(5));
    }

    #[test]
    fn manage_gate_requires_all_numeric_conditions() {
        let metrics = LoopMetrics {
            clean_submissions: 4,
            noisy_churn: 1,
            blocked_runs: 1,
        };
        assert_eq!(
            loop_management_strategy(4, 100, metrics),
            LoopDelegationStrategy::Observe
        );
        assert_eq!(
            loop_management_strategy(5, 60, metrics),
            LoopDelegationStrategy::Suggest
        );
        assert_eq!(
            loop_management_strategy(15, 75, metrics),
            LoopDelegationStrategy::Manage
        );
        assert_eq!(
            loop_management_strategy(
                15,
                75,
                LoopMetrics {
                    noisy_churn: 2,
                    ..metrics
                }
            ),
            LoopDelegationStrategy::Suggest
        );
    }

    #[test]
    fn clean_streak_is_derived_from_the_tail_only() {
        let entries = vec![entry(true, false, false), entry(false, false, true)];
        assert_eq!(LoopMetrics::clean_streak(&entries), 0);
    }

    #[test]
    fn suspension_helpers_are_hysteresis_sensitive() {
        let failed = vec![entry(false, false, true); 3];
        assert!(has_consecutive_blocked(&failed, 3));
        let clean = vec![entry(true, false, false); LOOP_MANAGE_CLEAN_STREAK];
        assert!(!can_resume_after_suspension(&clean, Some(101), 100));
        assert!(can_resume_after_suspension(&clean, Some(100), 100));
    }
}
