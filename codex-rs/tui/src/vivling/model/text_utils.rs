use super::types::WorkArchetype;

pub(crate) fn classify_work_archetype(summary: &str) -> WorkArchetype {
    let normalized = summary.to_ascii_lowercase();
    if contains_any(
        &normalized,
        &["review", "audit", "risk", "finding", "severity", "analyze"],
    ) {
        WorkArchetype::Reviewer
    } else if contains_any(
        &normalized,
        &[
            "docs", "document", "readme", "research", "study", "spec", "plan",
        ],
    ) {
        WorkArchetype::Researcher
    } else if contains_any(
        &normalized,
        &[
            "loop",
            "runner",
            "ci",
            "deploy",
            "monitor",
            "ops",
            "automation",
        ],
    ) {
        WorkArchetype::Operator
    } else {
        WorkArchetype::Builder
    }
}

pub(crate) fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

pub(crate) fn truncate_summary(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars();
    let truncated: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

pub(crate) fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}
