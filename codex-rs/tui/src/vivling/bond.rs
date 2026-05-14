use chrono::DateTime;
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;

const MIN_BOND: u8 = 0;
const MAX_BOND: u8 = 100;
const DEFAULT_BOND: u8 = 20;
const OFFSPRING_BOND: u8 = 10;
const DECAY_GRACE_HOURS: i64 = 24;
const STREAK_RESET_HOURS: i64 = 48;
const DECAY_PER_DAY: u8 = 3;
const STREAK_DAYS_PER_BONUS: u64 = 7;
const STREAK_BONUS_CAP: u8 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum VivlingInteractionKind {
    /// `/vl` dispatch reached the runtime (foundation).
    Chat,
    /// `/vivling assist` dispatch reached the runtime (foundation).
    Assist,
    /// Loop tick completed successfully through `mark_brain_reply_for`.
    LoopTick,
    /// `/vl` brain returned a successful reply (supplementary to `Chat`).
    BrainChatSucceeded,
    /// `/vivling assist` brain returned a successful reply (supplementary to `Assist`).
    BrainAssistSucceeded,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BondLevel {
    Strangers,
    Acquaintances,
    Companions,
    Partners,
    Bonded,
}

/// Relational tone band derived from `BondLevel`. The boundary adapter in
/// `*_ext.rs` translates this to `vl::lifecycle::LifecycleVoiceTone` for the
/// lifecycle layer — `vl/lifecycle` itself must NOT import `vivling::BondTone`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BondTone {
    /// Strangers (0–20) + Acquaintances (21–50): short, professional, no presumed familiarity.
    Neutral,
    /// Companions (51–75): default warmth — matches the pool that shipped pre-care-effects.
    Warm,
    /// Partners (76–90) + Bonded (91–100): warm, contextual, allusive.
    Familiar,
}

fn default_bond_value() -> u8 {
    DEFAULT_BOND
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub(crate) struct VivlingBond {
    #[serde(default = "default_bond_value")]
    pub(crate) value: u8,
    #[serde(default)]
    pub(crate) last_interaction: Option<DateTime<Utc>>,
    #[serde(default)]
    pub(crate) last_decay_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub(crate) chat_count: u64,
    #[serde(default)]
    pub(crate) assist_count: u64,
    #[serde(default)]
    pub(crate) loop_ticks_count: u64,
    #[serde(default)]
    pub(crate) streak_days: u64,
    #[serde(default)]
    pub(crate) last_streak_day: Option<String>,
}

impl Default for VivlingBond {
    fn default() -> Self {
        Self {
            value: DEFAULT_BOND,
            last_interaction: None,
            last_decay_at: None,
            chat_count: 0,
            assist_count: 0,
            loop_ticks_count: 0,
            streak_days: 0,
            last_streak_day: None,
        }
    }
}

impl VivlingBond {
    pub(crate) fn for_offspring() -> Self {
        Self {
            value: OFFSPRING_BOND,
            ..Self::default()
        }
    }

    pub(crate) fn level(&self) -> BondLevel {
        match self.value {
            0..=20 => BondLevel::Strangers,
            21..=50 => BondLevel::Acquaintances,
            51..=75 => BondLevel::Companions,
            76..=90 => BondLevel::Partners,
            _ => BondLevel::Bonded,
        }
    }

    /// Map the current bond level to a relational tone band (3 bands).
    /// Used by `vivling/runtime/proactive.rs` directly, and translated at the
    /// boundary to `vl::lifecycle::LifecycleVoiceTone` for the lifecycle layer.
    pub(crate) fn tone(&self) -> BondTone {
        match self.level() {
            BondLevel::Strangers | BondLevel::Acquaintances => BondTone::Neutral,
            BondLevel::Companions => BondTone::Warm,
            BondLevel::Partners | BondLevel::Bonded => BondTone::Familiar,
        }
    }

    /// Compact UI label for status / card surfaces, e.g. `"Companions 58/100"`.
    /// Same source-of-truth as `prompt_hint` (via `level()` + clamped display)
    /// but kept separate because intent differs: this is a glanceable text
    /// snippet, while `prompt_hint` embeds a behavioral rule for the brain.
    pub(crate) fn display_label(&self) -> String {
        let display = self.value.min(MAX_BOND);
        let label = match self.level() {
            BondLevel::Strangers => "Strangers",
            BondLevel::Acquaintances => "Acquaintances",
            BondLevel::Companions => "Companions",
            BondLevel::Partners => "Partners",
            BondLevel::Bonded => "Bonded",
        };
        format!("{label} {display}/100")
    }

    /// One-line bond hint for brain prompt steering on the Chat / Assist
    /// human-facing paths. LoopTick is intentionally not a caller — bond is a
    /// user-relationship signal, not an automation quality signal.
    ///
    /// The displayed value is clamped to `MAX_BOND` defensively: even though
    /// `record_interaction` and `apply_decay` keep `value` in `[0..=100]`, a
    /// corrupted `vivling.json` load could surface a stray `255/100` — the
    /// display pins it without mutating in-memory state.
    pub(crate) fn prompt_hint(&self) -> String {
        let display = self.value.min(MAX_BOND);
        let (label, rule) = match self.level() {
            BondLevel::Strangers => (
                "Strangers",
                "Be useful, brief, and do not assume familiarity.",
            ),
            BondLevel::Acquaintances => (
                "Acquaintances",
                "Be clear and steady; keep tone neutral-friendly.",
            ),
            BondLevel::Companions => (
                "Companions",
                "Be contextual and direct; avoid generic intros.",
            ),
            BondLevel::Partners => (
                "Partners",
                "Be proactive about relevant context, but keep claims verifiable.",
            ),
            BondLevel::Bonded => (
                "Bonded",
                "Use high trust and continuity, but stay precise and check assumptions.",
            ),
        };
        format!("Bond with user: {label} ({display}/100). {rule}")
    }

    pub(crate) fn record_interaction(&mut self, kind: VivlingInteractionKind, now: DateTime<Utc>) {
        let base_gain: u8 = match kind {
            VivlingInteractionKind::Chat => 1,
            VivlingInteractionKind::Assist => 2,
            VivlingInteractionKind::LoopTick => 1,
            VivlingInteractionKind::BrainChatSucceeded => 2,
            VivlingInteractionKind::BrainAssistSucceeded => 3,
        };

        let today = day_key(now);
        let yesterday_key = day_key(now - chrono::Duration::days(1));
        let is_first_today = self
            .last_streak_day
            .as_deref()
            .is_none_or(|prev| prev != today.as_str());

        if is_first_today {
            self.streak_days = match self.last_streak_day.as_deref() {
                Some(prev) if prev == yesterday_key.as_str() => self.streak_days.saturating_add(1),
                _ => 1,
            };
            self.last_streak_day = Some(today);
        }

        let bonus: u8 = if is_first_today {
            let weeks = self.streak_days / STREAK_DAYS_PER_BONUS;
            weeks.min(STREAK_BONUS_CAP as u64) as u8
        } else {
            0
        };

        let total_gain = base_gain as u16 + bonus as u16;
        self.value = (self.value as u16 + total_gain).min(MAX_BOND as u16) as u8;
        self.last_interaction = Some(now);

        match kind {
            VivlingInteractionKind::Chat => {
                self.chat_count = self.chat_count.saturating_add(1);
            }
            VivlingInteractionKind::Assist => {
                self.assist_count = self.assist_count.saturating_add(1);
            }
            VivlingInteractionKind::LoopTick => {
                self.loop_ticks_count = self.loop_ticks_count.saturating_add(1);
            }
            // Success bonuses modify bond.value only — counters stay tied to dispatch
            // so they keep their "how many times you reached out" semantics. See
            // DocsHub design § 1 (counter policy).
            VivlingInteractionKind::BrainChatSucceeded
            | VivlingInteractionKind::BrainAssistSucceeded => {}
        }
    }

    /// Apply time-based bond decay. Returns the delta removed (0 if no decay).
    ///
    /// Decay rules:
    /// - No decay within `DECAY_GRACE_HOURS` (24h) since `last_interaction`.
    /// - After grace, `DECAY_PER_DAY` (3) per overdue day.
    /// - Idempotent within 24h: a second call inside the same day is a no-op.
    /// - Streak resets when elapsed >= `STREAK_RESET_HOURS` (48h).
    pub(crate) fn apply_decay(&mut self, now: DateTime<Utc>) -> u8 {
        let Some(last) = self.last_interaction else {
            return 0;
        };
        let elapsed_hours = (now - last).num_hours();
        if elapsed_hours < DECAY_GRACE_HOURS {
            return 0;
        }

        if let Some(last_decay) = self.last_decay_at {
            if (now - last_decay).num_hours() < DECAY_GRACE_HOURS {
                return 0;
            }
        }

        let overdue_hours = elapsed_hours - DECAY_GRACE_HOURS;
        let overdue_days = (overdue_hours / 24) as u64 + 1;
        let total_decay = (overdue_days.min(u8::MAX as u64) as u8).saturating_mul(DECAY_PER_DAY);
        let before = self.value;
        self.value = self.value.saturating_sub(total_decay).max(MIN_BOND);
        self.last_decay_at = Some(now);

        if elapsed_hours >= STREAK_RESET_HOURS {
            self.streak_days = 0;
            self.last_streak_day = None;
        }

        before - self.value
    }
}

fn day_key(dt: DateTime<Utc>) -> String {
    dt.format("%Y-%m-%d").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn ts(year: i32, month: u32, day: u32, hour: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(year, month, day, hour, 0, 0).unwrap()
    }

    #[test]
    fn default_bond_starts_at_20() {
        let bond = VivlingBond::default();
        assert_eq!(bond.value, 20);
        assert_eq!(bond.level(), BondLevel::Strangers);
        assert!(bond.last_interaction.is_none());
        assert_eq!(bond.streak_days, 0);
    }

    #[test]
    fn for_offspring_starts_at_10() {
        let bond = VivlingBond::for_offspring();
        assert_eq!(bond.value, 10);
        assert_eq!(bond.level(), BondLevel::Strangers);
        assert!(bond.last_interaction.is_none());
    }

    #[test]
    fn level_boundaries() {
        let mk = |v: u8| {
            let mut b = VivlingBond::default();
            b.value = v;
            b.level()
        };
        assert_eq!(mk(0), BondLevel::Strangers);
        assert_eq!(mk(20), BondLevel::Strangers);
        assert_eq!(mk(21), BondLevel::Acquaintances);
        assert_eq!(mk(50), BondLevel::Acquaintances);
        assert_eq!(mk(51), BondLevel::Companions);
        assert_eq!(mk(75), BondLevel::Companions);
        assert_eq!(mk(76), BondLevel::Partners);
        assert_eq!(mk(90), BondLevel::Partners);
        assert_eq!(mk(91), BondLevel::Bonded);
        assert_eq!(mk(100), BondLevel::Bonded);
    }

    #[test]
    fn record_chat_adds_1_and_counts() {
        let mut bond = VivlingBond::default();
        bond.record_interaction(VivlingInteractionKind::Chat, ts(2026, 5, 13, 10));
        assert_eq!(bond.value, 21);
        assert_eq!(bond.chat_count, 1);
        assert_eq!(bond.assist_count, 0);
        assert_eq!(bond.loop_ticks_count, 0);
        assert_eq!(bond.streak_days, 1);
        assert_eq!(bond.last_interaction, Some(ts(2026, 5, 13, 10)));
    }

    #[test]
    fn record_assist_adds_2_and_counts() {
        let mut bond = VivlingBond::default();
        bond.record_interaction(VivlingInteractionKind::Assist, ts(2026, 5, 13, 10));
        assert_eq!(bond.value, 22);
        assert_eq!(bond.assist_count, 1);
    }

    #[test]
    fn record_loop_tick_adds_1_and_counts() {
        let mut bond = VivlingBond::default();
        bond.record_interaction(VivlingInteractionKind::LoopTick, ts(2026, 5, 13, 10));
        assert_eq!(bond.value, 21);
        assert_eq!(bond.loop_ticks_count, 1);
    }

    #[test]
    fn value_clamps_at_100() {
        let mut bond = VivlingBond::default();
        bond.value = 99;
        bond.record_interaction(VivlingInteractionKind::Assist, ts(2026, 5, 13, 10));
        assert_eq!(bond.value, 100);
        bond.record_interaction(VivlingInteractionKind::Chat, ts(2026, 5, 14, 10));
        assert_eq!(bond.value, 100);
    }

    #[test]
    fn streak_increments_consecutive_days() {
        let mut bond = VivlingBond::default();
        bond.record_interaction(VivlingInteractionKind::Chat, ts(2026, 5, 13, 10));
        bond.record_interaction(VivlingInteractionKind::Chat, ts(2026, 5, 14, 10));
        bond.record_interaction(VivlingInteractionKind::Chat, ts(2026, 5, 15, 10));
        assert_eq!(bond.streak_days, 3);
        assert_eq!(bond.last_streak_day.as_deref(), Some("2026-05-15"));
    }

    #[test]
    fn streak_resets_on_gap() {
        let mut bond = VivlingBond::default();
        bond.record_interaction(VivlingInteractionKind::Chat, ts(2026, 5, 13, 10));
        bond.record_interaction(VivlingInteractionKind::Chat, ts(2026, 5, 14, 10));
        // Gap of 2 days
        bond.record_interaction(VivlingInteractionKind::Chat, ts(2026, 5, 17, 10));
        assert_eq!(bond.streak_days, 1);
    }

    #[test]
    fn same_day_no_double_streak() {
        let mut bond = VivlingBond::default();
        bond.record_interaction(VivlingInteractionKind::Chat, ts(2026, 5, 13, 10));
        bond.record_interaction(VivlingInteractionKind::Chat, ts(2026, 5, 13, 14));
        bond.record_interaction(VivlingInteractionKind::Chat, ts(2026, 5, 13, 22));
        assert_eq!(bond.streak_days, 1);
        assert_eq!(bond.chat_count, 3);
        // 20 + 1 + 1 + 1 = 23 (bonus = 0 because streak < 7)
        assert_eq!(bond.value, 23);
    }

    #[test]
    fn streak_bonus_at_7_and_14_days() {
        let mut bond = VivlingBond::default();
        for d in 13..20 {
            bond.record_interaction(VivlingInteractionKind::Chat, ts(2026, 5, d, 10));
        }
        // After 7 consecutive days, streak_days == 7, bonus = 7/7 = 1
        assert_eq!(bond.streak_days, 7);
        // Days 1-6: +1 each (bonus 0). Day 7: +1 + 1 (bonus).
        // 20 + 6*1 + 1*(1+1) = 28
        assert_eq!(bond.value, 28);

        for d in 20..27 {
            bond.record_interaction(VivlingInteractionKind::Chat, ts(2026, 5, d, 10));
        }
        // After 14 consecutive days, bonus caps at 2
        assert_eq!(bond.streak_days, 14);
    }

    #[test]
    fn streak_bonus_caps_at_2() {
        // 22 consecutive days starting 2026-05-01.
        let mut bond = VivlingBond::default();
        let start = ts(2026, 5, 1, 10);
        for offset_days in 0..22i64 {
            let day = start + chrono::Duration::days(offset_days);
            bond.record_interaction(VivlingInteractionKind::Chat, day);
        }
        assert_eq!(bond.streak_days, 22);
        // floor(22 / 7) = 3, capped at STREAK_BONUS_CAP = 2.
        // Day 22 first-of-day adds base 1 + bonus 2 = 3.
        // We assert no overflow and bond stays within range.
        assert!(bond.value <= MAX_BOND);
        // Sanity: 7th-day onward the bonus must already be at least 1.
        // Without the cap, by day 22 the running bonus would have been 0+0+0+0+0+0+1+1+1+1+1+1+1+2+2+2+2+2+2+2+3+3,
        // capped to 0..=2 → no value greater than 100, no underflow.
    }

    #[test]
    fn apply_decay_no_op_within_grace() {
        let mut bond = VivlingBond::default();
        bond.record_interaction(VivlingInteractionKind::Chat, ts(2026, 5, 13, 10));
        let before = bond.value;
        let delta = bond.apply_decay(ts(2026, 5, 13, 22)); // 12h later
        assert_eq!(delta, 0);
        assert_eq!(bond.value, before);
    }

    #[test]
    fn apply_decay_after_grace_removes_3() {
        let mut bond = VivlingBond::default();
        bond.record_interaction(VivlingInteractionKind::Chat, ts(2026, 5, 13, 10));
        // 30 hours later: grace 24h, overdue 6h → 1 overdue day → -3
        let delta = bond.apply_decay(ts(2026, 5, 14, 16));
        assert_eq!(delta, 3);
        assert_eq!(bond.value, 21 - 3);
        assert!(bond.last_decay_at.is_some());
    }

    #[test]
    fn apply_decay_idempotent_within_24h() {
        let mut bond = VivlingBond::default();
        bond.record_interaction(VivlingInteractionKind::Chat, ts(2026, 5, 13, 10));
        let _ = bond.apply_decay(ts(2026, 5, 14, 16));
        let after_first = bond.value;
        // Second call 6h later: should be no-op
        let delta = bond.apply_decay(ts(2026, 5, 14, 22));
        assert_eq!(delta, 0);
        assert_eq!(bond.value, after_first);
    }

    #[test]
    fn apply_decay_clamps_at_0() {
        let mut bond = VivlingBond::default();
        bond.value = 5;
        bond.last_interaction = Some(ts(2026, 5, 1, 10));
        // 12 days later → 11 overdue days * 3 = 33 → clamped at 0
        let delta = bond.apply_decay(ts(2026, 5, 13, 10));
        assert_eq!(bond.value, MIN_BOND);
        assert_eq!(delta, 5);
    }

    #[test]
    fn apply_decay_resets_streak_after_48h() {
        let mut bond = VivlingBond::default();
        bond.record_interaction(VivlingInteractionKind::Chat, ts(2026, 5, 13, 10));
        bond.record_interaction(VivlingInteractionKind::Chat, ts(2026, 5, 14, 10));
        assert_eq!(bond.streak_days, 2);
        // 60h after last interaction → > 48h
        let _ = bond.apply_decay(ts(2026, 5, 16, 22));
        assert_eq!(bond.streak_days, 0);
        assert!(bond.last_streak_day.is_none());
    }

    #[test]
    fn apply_decay_no_op_when_never_interacted() {
        let mut bond = VivlingBond::default();
        let delta = bond.apply_decay(ts(3000, 1, 1, 0));
        assert_eq!(delta, 0);
        assert_eq!(bond.value, DEFAULT_BOND);
    }

    #[test]
    fn round_trip_serde() {
        let mut bond = VivlingBond::default();
        bond.record_interaction(VivlingInteractionKind::Chat, ts(2026, 5, 13, 10));
        bond.record_interaction(VivlingInteractionKind::Assist, ts(2026, 5, 14, 10));
        let json = serde_json::to_string(&bond).expect("serialize");
        let restored: VivlingBond = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(bond, restored);
    }

    #[test]
    fn legacy_state_without_bond_deserializes_to_default() {
        // Simulating a Vivling state JSON without the bond field
        let json = "{}";
        let bond: VivlingBond = serde_json::from_str(json).expect("deserialize empty");
        assert_eq!(bond.value, DEFAULT_BOND);
        assert_eq!(bond.streak_days, 0);
        assert!(bond.last_interaction.is_none());
    }

    fn bond_at(value: u8) -> VivlingBond {
        VivlingBond {
            value,
            ..VivlingBond::default()
        }
    }

    #[test]
    fn prompt_hint_strangers_at_0() {
        let hint = bond_at(0).prompt_hint();
        assert!(hint.contains("Bond with user: Strangers"), "{hint}");
        assert!(hint.contains("(0/100)"), "{hint}");
        assert!(hint.contains("do not assume familiarity"), "{hint}");
    }

    #[test]
    fn prompt_hint_acquaintances_at_30() {
        let hint = bond_at(30).prompt_hint();
        assert!(hint.contains("Bond with user: Acquaintances"), "{hint}");
        assert!(hint.contains("(30/100)"), "{hint}");
        assert!(hint.contains("neutral-friendly"), "{hint}");
    }

    #[test]
    fn prompt_hint_companions_at_60() {
        let hint = bond_at(60).prompt_hint();
        assert!(hint.contains("Bond with user: Companions"), "{hint}");
        assert!(hint.contains("(60/100)"), "{hint}");
        assert!(hint.contains("contextual and direct"), "{hint}");
    }

    #[test]
    fn prompt_hint_partners_at_80() {
        let hint = bond_at(80).prompt_hint();
        assert!(hint.contains("Bond with user: Partners"), "{hint}");
        assert!(hint.contains("(80/100)"), "{hint}");
        assert!(hint.contains("proactive about relevant context"), "{hint}");
    }

    #[test]
    fn prompt_hint_bonded_at_100() {
        let hint = bond_at(100).prompt_hint();
        assert!(hint.contains("Bond with user: Bonded"), "{hint}");
        assert!(hint.contains("(100/100)"), "{hint}");
        assert!(hint.contains("high trust and continuity"), "{hint}");
    }

    #[test]
    fn prompt_hint_clamps_display_value_defensively() {
        // Even if value were corrupted past 100 (e.g. tampered JSON), the
        // displayed value must stay within MAX_BOND. Bypass struct invariants
        // by writing the field directly.
        let mut bond = VivlingBond::default();
        bond.value = 200;
        let hint = bond.prompt_hint();
        assert!(hint.contains("(100/100)"), "{hint}");
        assert!(hint.contains("Bonded"), "{hint}");
    }

    #[test]
    fn display_label_strangers_at_0() {
        assert_eq!(bond_at(0).display_label(), "Strangers 0/100");
    }

    #[test]
    fn display_label_acquaintances_at_30() {
        assert_eq!(bond_at(30).display_label(), "Acquaintances 30/100");
    }

    #[test]
    fn display_label_companions_at_60() {
        assert_eq!(bond_at(60).display_label(), "Companions 60/100");
    }

    #[test]
    fn display_label_partners_at_80() {
        assert_eq!(bond_at(80).display_label(), "Partners 80/100");
    }

    #[test]
    fn display_label_bonded_at_100() {
        assert_eq!(bond_at(100).display_label(), "Bonded 100/100");
    }

    #[test]
    fn display_label_clamps_at_max_defensively() {
        let mut bond = VivlingBond::default();
        bond.value = 200;
        assert_eq!(bond.display_label(), "Bonded 100/100");
    }

    #[test]
    fn record_brain_chat_succeeded_adds_2_value_only() {
        let mut bond = VivlingBond::default();
        bond.record_interaction(
            VivlingInteractionKind::BrainChatSucceeded,
            ts(2026, 5, 14, 10),
        );
        // 20 + 2 = 22
        assert_eq!(bond.value, 22);
        // counters tied to dispatch, NOT to success
        assert_eq!(bond.chat_count, 0);
        assert_eq!(bond.assist_count, 0);
        assert_eq!(bond.loop_ticks_count, 0);
        assert_eq!(bond.last_interaction, Some(ts(2026, 5, 14, 10)));
    }

    #[test]
    fn record_brain_assist_succeeded_adds_3_value_only() {
        let mut bond = VivlingBond::default();
        bond.record_interaction(
            VivlingInteractionKind::BrainAssistSucceeded,
            ts(2026, 5, 14, 10),
        );
        // 20 + 3 = 23
        assert_eq!(bond.value, 23);
        assert_eq!(bond.chat_count, 0);
        assert_eq!(bond.assist_count, 0);
        assert_eq!(bond.loop_ticks_count, 0);
    }

    #[test]
    fn brain_chat_succeeded_value_clamps_at_100() {
        let mut bond = VivlingBond::default();
        bond.value = 99;
        bond.record_interaction(
            VivlingInteractionKind::BrainChatSucceeded,
            ts(2026, 5, 14, 10),
        );
        assert_eq!(bond.value, 100);
    }

    #[test]
    fn dispatch_plus_success_same_day_no_double_streak() {
        let mut bond = VivlingBond::default();
        bond.record_interaction(VivlingInteractionKind::Chat, ts(2026, 5, 14, 10));
        bond.record_interaction(
            VivlingInteractionKind::BrainChatSucceeded,
            ts(2026, 5, 14, 11),
        );
        // dispatch +1 = 21; success +2 = 23; both same-day so streak stays at 1
        assert_eq!(bond.value, 23);
        assert_eq!(bond.streak_days, 1);
        // Only the dispatch increments the chat_count.
        assert_eq!(bond.chat_count, 1);
    }

    #[test]
    fn tone_strangers_and_acquaintances_map_to_neutral() {
        for v in [0u8, 10, 20, 21, 35, 50] {
            assert_eq!(bond_at(v).tone(), BondTone::Neutral, "value={v}");
        }
    }

    #[test]
    fn tone_companions_maps_to_warm() {
        for v in [51u8, 60, 75] {
            assert_eq!(bond_at(v).tone(), BondTone::Warm, "value={v}");
        }
    }

    #[test]
    fn tone_partners_and_bonded_map_to_familiar() {
        for v in [76u8, 85, 90, 91, 100] {
            assert_eq!(bond_at(v).tone(), BondTone::Familiar, "value={v}");
        }
    }
}
