use super::model::Stage;
use super::model::VivlingState;
use super::model::VivlingUpgrade;
use crate::vivling::BondTone;
use chrono::DateTime;
use chrono::Utc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ZedTopic {
    YoungVoice,
    ActiveMode,
    Growth,
    LoopOnboarding,
    LoopRhythm,
    LoopAssistReady,
    Companion,
    Lineage,
}

impl ZedTopic {
    pub(crate) fn from_slug(slug: &str) -> Self {
        match slug {
            "young-voice" => Self::YoungVoice,
            "active-mode" => Self::ActiveMode,
            "companion" => Self::Companion,
            "loop-onboarding" => Self::LoopOnboarding,
            "loop-rhythm" => Self::LoopRhythm,
            "loop-assist-ready" => Self::LoopAssistReady,
            "lineage" => Self::Lineage,
            _ => Self::Growth,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ZedPanelData {
    pub(crate) title: String,
    pub(crate) narrow_lines: Vec<String>,
    pub(crate) wide_lines: Vec<String>,
}

pub(crate) fn zed_panel_data(topic: ZedTopic, summary: &str) -> ZedPanelData {
    let build_lines = |width_hint: usize| {
        let mut lines = zed_card_lines(width_hint);
        lines.push(String::new());
        lines.push(summary.to_string());
        lines.push(String::new());
        lines.push(zed_hint_for_topic(topic).to_string());
        lines.push(zed_next_step_for_topic(topic).to_string());
        lines
    };
    ZedPanelData {
        title: zed_title_for_topic(topic).to_string(),
        narrow_lines: build_lines(64),
        wide_lines: build_lines(120),
    }
}

fn zed_card_lines(width_hint: usize) -> Vec<String> {
    let full = vec![
        "         _....................._".to_string(),
        "      .                         .".to_string(),
        "     /         _______           \\".to_string(),
        "    /        .         .          \\".to_string(),
        "   /        /   _..._   \\          \\".to_string(),
        "  |        |  /       \\  |          |".to_string(),
        "  |        |  \\_______/  |          |".to_string(),
        "  |        |             |          |".to_string(),
        "  |        |      _      |          |".to_string(),
        "  |        |     / \\     |          |".to_string(),
        "  |        |    /   \\    |          |".to_string(),
        "  |        |   /_____\\   |          |".to_string(),
        "   \\        \\           /          /".to_string(),
        "    .        .         .          .".to_string(),
        "     `._      `._..._.'       _.'".to_string(),
        "        `----.         .----'".to_string(),
        "         __/ |         |\\__".to_string(),
        "     _.-'   /|========|\\  `-._".to_string(),
        "   .'     _/ |  ====  | \\_    `.".to_string(),
        "  /      /   |   ==   |   \\     \\".to_string(),
        " |      |    |  ====  |    |     |".to_string(),
        " |      |    |   ==   |    |     |".to_string(),
        " |      |    |________|    |     |".to_string(),
        " |      |     / |   |\\     |     |".to_string(),
        " |      |    /_ |___| \\    |     |".to_string(),
        " |      |     / |   | \\    |     |".to_string(),
        "  \\      \\___/ /     \\ \\___/     /".to_string(),
        "   .          /_______\\         .".to_string(),
        "    `-._                     _.-'".to_string(),
        "        `-------------------'".to_string(),
    ];

    if width_hint < 68 { full } else { full }
}

fn zed_title_for_topic(topic: ZedTopic) -> &'static str {
    match topic {
        ZedTopic::YoungVoice => "ZED THE PRIME · Young Voice",
        ZedTopic::ActiveMode => "ZED THE PRIME · Active Mode",
        ZedTopic::Growth => "ZED THE PRIME · Growth",
        ZedTopic::LoopOnboarding => "ZED THE PRIME · Loop Onboarding",
        ZedTopic::LoopRhythm => "ZED THE PRIME · Loop Rhythm",
        ZedTopic::LoopAssistReady => "ZED THE PRIME · Loop Assist Ready",
        ZedTopic::Companion => "ZED THE PRIME · Companion",
        ZedTopic::Lineage => "ZED THE PRIME · Lineage",
    }
}

pub(crate) fn zed_summary_for_upgrade(kind: VivlingUpgrade) -> String {
    match kind {
        VivlingUpgrade::YoungVoice => {
            "ZED: your Vivling reached juvenile stage. It speaks more, asks tighter questions, and learns faster from work.".to_string()
        }
        VivlingUpgrade::ActiveMode => {
            "ZED: your Vivling reached adult stage. Active help is available, but it stays quiet until you switch it on.".to_string()
        }
    }
}

pub(crate) fn zed_summary_for_stage(stage: Stage) -> String {
    match stage {
        Stage::Baby => {
            "ZED: baby stage is still active. Keep feeding it real work memory and active days.".to_string()
        }
        Stage::Juvenile => {
            "ZED: juvenile capability is already unlocked. Use `/vivling <message>` and it will follow the work more actively.".to_string()
        }
        Stage::Adult => {
            "ZED: adult capability is already unlocked. Use `/vivling mode on` only when you want active help.".to_string()
        }
    }
}

pub(crate) fn zed_summary_for_topic(topic: ZedTopic) -> String {
    match topic {
        ZedTopic::YoungVoice => {
            "ZED: young voice is unlocked. Your Vivling now follows the work, asks brief questions, and suggests tighter next steps."
                .to_string()
        }
        ZedTopic::ActiveMode => {
            "ZED: active help is available, but it remains opt-in until you switch it on."
                .to_string()
        }
        ZedTopic::Growth => {
            "ZED: growth comes from real work memory and active days. It is slow on purpose."
                .to_string()
        }
        ZedTopic::LoopOnboarding => {
            "ZED: your Vivling has seen enough repeated work to benefit from one small loop with one clear goal."
                .to_string()
        }
        ZedTopic::LoopRhythm => {
            "ZED: your loop history looks noisy. Consolidate the rhythm before adding more automation."
                .to_string()
        }
        ZedTopic::LoopAssistReady => {
            "ZED: your Vivling understands the loop rhythm well enough to support it more deliberately."
                .to_string()
        }
        ZedTopic::Companion => {
            "ZED: this is a snapshot of your bond and gene profile. The bond grows with real work and decays with silence; the gene profile shapes how the companion approaches different work archetypes."
                .to_string()
        }
        ZedTopic::Lineage => {
            "ZED: a lineage signal joined the roster. The newborn carries distilled traces from the active primary; the biological origin determines species, the primary determines culture."
                .to_string()
        }
    }
}

fn zed_hint_for_topic(topic: ZedTopic) -> &'static str {
    match topic {
        ZedTopic::YoungVoice => {
            "ZED: the young Vivling should stay short, specific, and tied to what it has actually learned."
        }
        ZedTopic::ActiveMode => {
            "ZED: active help should stay off by default. Switch it on only when you want a tighter operational read."
        }
        ZedTopic::Growth => {
            "ZED: growth comes from real work memory and active days. It is meant to feel earned."
        }
        ZedTopic::LoopOnboarding => {
            "ZED: start with one focused recurring check. A loop should have one goal, not many."
        }
        ZedTopic::LoopRhythm => {
            "ZED: too many updates or blocked runs make loops noisy. Tighten the goal before widening the automation."
        }
        ZedTopic::LoopAssistReady => {
            "ZED: your loop history is healthy enough that active help can be useful, but only if goals stay explicit."
        }
        ZedTopic::Companion => {
            "ZED: bond is the relationship signal; gene is the identity signal. They evolve at different rates. Both inform how your Vivling addresses you on Chat and Assist."
        }
        ZedTopic::Lineage => {
            "ZED: the newborn stays inactive. It learns from the primary's distilled summaries; it does not speak through Brain, Chat or Loop."
        }
    }
}

fn zed_next_step_for_topic(topic: ZedTopic) -> &'static str {
    match topic {
        ZedTopic::YoungVoice => "Next: `/vivling <message>`",
        ZedTopic::ActiveMode => "Next: `/vivling mode on` then `/vivling <message>`",
        ZedTopic::Growth => "Next: keep working, feed memory, and check `/vivling status`",
        ZedTopic::LoopOnboarding => "Next: create one loop with one explicit goal",
        ZedTopic::LoopRhythm => "Next: reduce loop churn and keep one focused loop per goal",
        ZedTopic::LoopAssistReady => {
            "Next: `/vivling mode on` only if you want active help with current work"
        }
        ZedTopic::Companion => "Next: keep working together — bond decays after 24h of silence",
        ZedTopic::Lineage => {
            "Next: keep working with the primary — the newborn will learn passively"
        }
    }
}

/// codex-vl iter 1C: dynamic Lineage narration for a fresh spawn.
///
/// Renders the ZED-as-presenter narration that opens after a successful
/// `/vivling spawn`. Stays scripted (no Brain), generic (no raw parent
/// distilled summaries leaked), and emphasises the cultural-vs-biological
/// split DAG codified on 2026-05-15.
pub(crate) fn zed_summary_for_lineage(
    parent_name: &str,
    child_name: &str,
    child_species: &str,
    origin_label: &str,
) -> String {
    let origin_note = match origin_label {
        "primary_child" => format!("biological parent: {parent_name} (primary)"),
        "veteran_child" => "biological parent: a roster veteran".to_string(),
        "zed_hatch" => "biological origin: ZED introduces a new bloodline".to_string(),
        _ => format!("biological origin: {origin_label}"),
    };
    format!(
        "ZED: lineage signal received.\n\
         {child_name} joined the roster as a {child_species}.\n\
         {origin_note}.\n\
         Cultural parent: {parent_name} — the newborn will learn passively from {parent_name}'s distilled summaries.\n\
         The newborn stays inactive: no Brain, no Chat, no Loop ownership."
    )
}

/// Compose the dynamic Companion summary using the current Vivling state.
///
/// Runtime entry: calls `Utc::now()`. Tests should target
/// `zed_companion_summary_at` directly with a fixed `now` so the
/// `Last seen: ...` rendering is deterministic.
pub(crate) fn zed_companion_summary(state: &VivlingState) -> String {
    zed_companion_summary_at(state, Utc::now())
}

/// Deterministic helper. All tests must use this variant with a fixed
/// `now` so the `Last seen` rendering is reproducible.
pub(crate) fn zed_companion_summary_at(state: &VivlingState, now: DateTime<Utc>) -> String {
    let mut lines: Vec<String> = Vec::new();
    lines.push("Bond:".to_string());
    lines.push(format!("  Level    : {}", bond_level_label(state)));
    lines.push(format!("  Value    : {}/100", state.bond.value.min(100)));
    lines.push(format!(
        "  Tone     : {}",
        bond_tone_label(state.bond.tone())
    ));
    if state.bond.streak_days > 0 {
        let last_streak = state.bond.last_streak_day.as_deref().unwrap_or("?");
        lines.push(format!(
            "  Streak   : {} days (last {})",
            state.bond.streak_days, last_streak
        ));
    }
    lines.push(format!("  Chat     : {} reach-outs", state.bond.chat_count));
    lines.push(format!(
        "  Assist   : {} reach-outs",
        state.bond.assist_count
    ));
    lines.push(format!(
        "  Loop tick: {} collaborations",
        state.bond.loop_ticks_count
    ));
    lines.push(format!(
        "  Last seen: {}",
        last_seen_label(state.bond.last_interaction, now)
    ));
    lines.push(String::new());
    lines.push("Gene:".to_string());
    lines.push(format!(
        "  Stripe        : {}",
        state.gene_vector.gene_stripe()
    ));
    lines.push(format!(
        "  Temperament   : {}",
        state.gene_vector.temperament_summary()
    ));
    lines.push(format!(
        "  Brain potential: {}",
        state.gene_vector.brain_potential_label()
    ));
    lines.join("\n")
}

fn bond_level_label(state: &VivlingState) -> &'static str {
    use crate::vivling::BondLevel;
    match state.bond.level() {
        BondLevel::Strangers => "Strangers",
        BondLevel::Acquaintances => "Acquaintances",
        BondLevel::Companions => "Companions",
        BondLevel::Partners => "Partners",
        BondLevel::Bonded => "Bonded",
    }
}

fn bond_tone_label(tone: BondTone) -> &'static str {
    match tone {
        BondTone::Neutral => "neutral",
        BondTone::Warm => "warm",
        BondTone::Familiar => "familiar",
    }
}

fn last_seen_label(last_interaction: Option<DateTime<Utc>>, now: DateTime<Utc>) -> String {
    let Some(last) = last_interaction else {
        return "never".to_string();
    };
    let delta = now.signed_duration_since(last);
    let secs = delta.num_seconds().max(0);
    if secs < 60 {
        return "just now".to_string();
    }
    if secs < 3600 {
        return format!("{}m ago", secs / 60);
    }
    if secs < 86_400 {
        return format!("{}h ago", secs / 3600);
    }
    let days = secs / 86_400;
    format!("{days} day{} ago", if days == 1 { "" } else { "s" })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vivling::VivlingInteractionKind;
    use crate::vivling::model::SeedIdentity;
    use chrono::TimeZone;

    fn seeded_state() -> VivlingState {
        VivlingState::new(SeedIdentity {
            value: "install:zed-companion-test".to_string(),
            install_id: Some("zed-companion-test".to_string()),
        })
    }

    fn ts(year: i32, month: u32, day: u32, hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(year, month, day, hour, 0, 0).unwrap()
    }

    #[test]
    fn companion_summary_at_includes_bond_level_value_tone() {
        let state = seeded_state();
        let summary = zed_companion_summary_at(&state, ts(2026, 5, 14, 18));
        // Default bond: value 20 → Strangers / Neutral.
        assert!(summary.contains("Bond:"), "{summary}");
        assert!(summary.contains("Level    : Strangers"), "{summary}");
        assert!(summary.contains("Value    : 20/100"), "{summary}");
        assert!(summary.contains("Tone     : neutral"), "{summary}");
    }

    #[test]
    fn companion_summary_at_omits_streak_when_zero() {
        let state = seeded_state();
        let summary = zed_companion_summary_at(&state, ts(2026, 5, 14, 18));
        assert!(!summary.contains("Streak"), "{summary}");
    }

    #[test]
    fn companion_summary_at_emits_streak_when_present() {
        let mut state = seeded_state();
        state
            .bond
            .record_interaction(VivlingInteractionKind::Chat, ts(2026, 5, 14, 10));
        let summary = zed_companion_summary_at(&state, ts(2026, 5, 14, 18));
        assert!(summary.contains("Streak"), "{summary}");
        assert!(summary.contains("1 days (last 2026-05-14)"), "{summary}");
    }

    #[test]
    fn companion_summary_at_includes_gene_stripe_and_temperament() {
        let state = seeded_state();
        let summary = zed_companion_summary_at(&state, ts(2026, 5, 14, 18));
        assert!(summary.contains("Gene:"), "{summary}");
        assert!(summary.contains("Stripe"), "{summary}");
        assert!(summary.contains("Temperament"), "{summary}");
        assert!(summary.contains("Brain potential"), "{summary}");
    }

    #[test]
    fn last_seen_label_renders_hours_for_2h_old_interaction() {
        let now = ts(2026, 5, 14, 18);
        let last = ts(2026, 5, 14, 16);
        assert_eq!(last_seen_label(Some(last), now), "2h ago");
    }

    #[test]
    fn last_seen_label_renders_minutes_for_recent_interaction() {
        let now = ts(2026, 5, 14, 18);
        let last = ts(2026, 5, 14, 17) + chrono::Duration::minutes(30);
        assert_eq!(last_seen_label(Some(last), now), "30m ago");
    }

    #[test]
    fn last_seen_label_renders_just_now_for_under_a_minute() {
        let now = ts(2026, 5, 14, 18);
        let last = now - chrono::Duration::seconds(30);
        assert_eq!(last_seen_label(Some(last), now), "just now");
    }

    #[test]
    fn last_seen_label_renders_days_with_pluralization() {
        let now = ts(2026, 5, 14, 18);
        let one_day_ago = now - chrono::Duration::days(1);
        let three_days_ago = now - chrono::Duration::days(3);
        assert_eq!(last_seen_label(Some(one_day_ago), now), "1 day ago");
        assert_eq!(last_seen_label(Some(three_days_ago), now), "3 days ago");
    }

    #[test]
    fn last_seen_label_renders_never_for_no_interaction() {
        let now = ts(2026, 5, 14, 18);
        assert_eq!(last_seen_label(None, now), "never");
    }
}
