//! Pool of short autonomous thoughts for baby and juvenile Vivlings
//! during prolonged idle periods.

/// Returns a thought string for the given stage and tick counter.
/// Tick counter should increment slowly (e.g. every 15-30s of idle).
pub(crate) fn idle_thought(is_juvenile: bool, tick: usize) -> &'static str {
    if is_juvenile {
        const JUVENILE: &[&str] = &[
            "verify first?",
            "check the loop",
            "pattern seen..",
            "almost adult",
            "keep one goal",
            "growing fast",
            "what changed?",
            "ready to help",
        ];
        JUVENILE[tick % JUVENILE.len()]
    } else {
        const BABY: &[&str] = &[
            "what is this..",
            "learning rhythm",
            "small but here",
            "tell me more",
            "ready to learn",
            "watching you",
            "what's next?",
            "feed me work",
        ];
        BABY[tick % BABY.len()]
    }
}
