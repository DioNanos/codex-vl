use chrono::DateTime;
use chrono::Duration;
use chrono::Utc;

use super::text_utils::fnv1a64;
use super::*;

const ADULT_SEED_ORIGIN: &str = "adult_seed_v1";
const EARLY_SEED_ORIGIN: &str = "early_seed_v1";

const NAMES: &[&str] = &[
    "Nilo", "Kira", "Moro", "Luma", "Pax", "Rin", "Taro", "Vera", "Sumi", "Nox", "Iko", "Mina",
    "Zed", "Ari", "Tika", "Juno",
];

const BABY_GREETINGS: &[&str] = &[
    "is tiny and still learning your rhythm",
    "is blinking at the work around it",
    "is small, curious, and very present",
];

impl VivlingState {
    #[cfg(test)]
    pub(crate) fn new(seed: SeedIdentity) -> Self {
        let hash = fnv1a64(seed.value.as_bytes());
        let species = hatch_species(hash);
        Self::new_with_species_and_unlocks(seed, species, Self::default_unlocked_species())
    }

    pub(crate) fn new_with_species_and_unlocks(
        seed: SeedIdentity,
        species: &'static crate::vivling::registry::VivlingSpeciesDefinition,
        unlocked_species: Vec<String>,
    ) -> Self {
        let hash = fnv1a64(seed.value.as_bytes());
        let now = Utc::now();
        let vivling_id = seed
            .install_id
            .clone()
            .unwrap_or_else(|| format!("viv-{:08x}", hash as u32));
        let mut state = Self {
            version: VERSION,
            hatched: true,
            visible: true,
            seed_hash: format!("{hash:016x}"),
            vivling_id: vivling_id.clone(),
            install_id: seed.install_id.clone(),
            origin_install_id: seed.install_id,
            species: species.id.clone(),
            rarity: species.rarity.label().to_string(),
            name: NAMES[((hash >> 8) as usize) % NAMES.len()].to_string(),
            primary_vivling_id: vivling_id,
            parent_vivling_id: None,
            spawn_generation: 0,
            is_primary: true,
            is_imported: false,
            imported_at: None,
            import_source: None,
            export_count: 0,
            instance_label: None,
            created_at: Some(now),
            last_seen_at: Some(now),
            last_fed_at: Some(now),
            xp: 0,
            level: 1,
            hunger: 82,
            energy: 76,
            happiness: 70,
            social: 62,
            meals: 0,
            pets: 0,
            plays: 0,
            sleeps: 0,
            observations: 0,
            ai_mode: VivlingAiMode::Off,
            brain_enabled: false,
            brain_profile: None,
            brain_last_error: None,
            brain_last_used_at: None,
            seed_origin: None,
            adult_bootstrap: false,
            work_xp: 0,
            loop_exposure: 0,
            loop_runtime_submissions: 0,
            loop_runtime_blocks: 0,
            loop_admin_churn: 0,
            loop_blocked_review: 0,
            loop_blocked_side: 0,
            loop_blocked_busy: 0,
            turns_observed: 0,
            suggestions_made: 0,
            active_work_days: 0,
            last_active_work_day: None,
            last_work_xp_day: None,
            daily_work_xp: 0,
            chat_unlocked_at: None,
            active_mode_unlocked_at: None,
            last_work_summary: None,
            last_live_context_summary: None,
            work_affinities: WorkAffinitySet::default(),
            work_memory: Vec::new(),
            distilled_summaries: Vec::new(),
            mental_paths: Vec::new(),
            identity_profile: VivlingIdentityProfile::default(),
            loop_profile: VivlingLoopProfile::default(),
            capsules_since_distill: 0,
            last_message: Some(BABY_GREETINGS[(hash as usize) % BABY_GREETINGS.len()].to_string()),
            pending_upgrade: None,
            last_seen_upgrade: None,
            last_zed_topic: None,
            unlocked_species,
        };
        state.normalize_unlocked_species();
        state.recompute_level();
        state
    }

    pub(crate) fn apply_decay(&mut self, now: DateTime<Utc>) {
        let Some(last_seen) = self.last_seen_at else {
            self.last_seen_at = Some(now);
            return;
        };
        let elapsed = now.signed_duration_since(last_seen);
        if elapsed < Duration::hours(12) {
            self.last_seen_at = Some(now);
            return;
        }
        let days = elapsed.num_days().max(1);
        self.hunger = (self.hunger - days * 8).clamp(0, 100);
        self.energy = (self.energy - days * 3).clamp(0, 100);
        self.happiness = (self.happiness - days * 4).clamp(0, 100);
        self.social = (self.social - days * 5).clamp(0, 100);
        self.last_seen_at = Some(now);
    }

    pub(crate) fn normalize_loaded_state(&mut self) {
        let persisted_work_xp = self.work_xp;
        let persisted_active_work_days = self.active_work_days;
        self.version = VERSION;
        if self.vivling_id.trim().is_empty() {
            self.vivling_id = self
                .install_id
                .clone()
                .unwrap_or_else(|| format!("viv-{}", self.seed_hash));
        }
        if self.primary_vivling_id.trim().is_empty() {
            self.primary_vivling_id = self.vivling_id.clone();
        }
        if self.origin_install_id.is_none() {
            self.origin_install_id = self.install_id.clone();
        }
        if self.is_primary && self.parent_vivling_id.is_some() {
            self.parent_vivling_id = None;
        }
        if self.primary_vivling_id == self.vivling_id {
            self.is_primary = true;
        }
        if self.level == 0 {
            self.level = 1;
        }
        self.normalize_species();
        self.normalize_unlocked_species();
        self.backfill_capsule_metadata();
        if !self.work_memory.is_empty() {
            self.recompute_progress_from_memory();
            self.work_xp = self.work_xp.max(persisted_work_xp);
            self.active_work_days = self.active_work_days.max(persisted_active_work_days);
            self.xp = self.work_xp;
        } else {
            self.work_xp = persisted_work_xp;
            self.active_work_days = persisted_active_work_days;
            self.xp = self.work_xp;
        }
        self.rebuild_learning_profiles();
        self.recompute_level();
        if self.brain_last_error.as_deref().is_some_and(str::is_empty) {
            self.brain_last_error = None;
        }
        if self.seed_origin.as_deref().is_some_and(str::is_empty) {
            self.seed_origin = None;
        }
        if self.stage() != Stage::Adult {
            self.brain_enabled = false;
        }
        if self.last_message.is_none() {
            self.last_message = Some("is watching the session".to_string());
        }
    }

    pub(crate) fn brain_summary(&self) -> String {
        let profile = self.brain_profile.as_deref().unwrap_or("none");
        let status = if self.brain_enabled { "on" } else { "off" };
        let last = self
            .brain_last_error
            .as_deref()
            .map(|err| format!(" - last_error {}", truncate_summary(err, 64)))
            .unwrap_or_default();
        format!("brain {status} - profile {profile}{last}")
    }

    pub(crate) fn set_brain_enabled(&mut self, enabled: bool) -> Result<String, String> {
        if enabled {
            if self.stage() != Stage::Adult {
                return Err("Vivling brain unlocks only at level 60.".to_string());
            }
            if self.brain_profile.is_none() {
                return Err(
                    "Set a Vivling brain profile first with `/vivling model ...`.".to_string(),
                );
            }
        }
        self.brain_enabled = enabled;
        self.brain_last_error = None;
        let message = format!(
            "{} brain {}.",
            self.name,
            if enabled { "enabled" } else { "disabled" }
        );
        self.last_message = Some(message.clone());
        Ok(message)
    }

    pub(crate) fn assign_brain_profile(&mut self, profile: String) -> String {
        self.brain_profile = Some(profile.clone());
        self.brain_last_error = None;
        let auto_enabled = self.stage() == Stage::Adult;
        if auto_enabled {
            self.brain_enabled = true;
        }
        let message = if auto_enabled {
            format!(
                "{} brain profile set to `{profile}` and brain enabled.",
                self.name
            )
        } else {
            format!("{} brain profile set to `{profile}`.", self.name)
        };
        self.last_message = Some(message.clone());
        message
    }

    pub(crate) fn mark_brain_runtime_error(&mut self, error: impl Into<String>) {
        let error = truncate_summary(&error.into(), 240);
        self.brain_last_error = Some(error.clone());
        self.last_message = Some(error);
    }

    pub(crate) fn mark_brain_reply(&mut self, reply: &str) {
        self.brain_last_error = None;
        self.brain_last_used_at = Some(Utc::now());
        self.last_message = Some(truncate_summary(reply, MAX_DIRECT_REPLY_LEN));
    }

    pub(crate) fn promote_to_adult_seed(&mut self) -> String {
        let now = Utc::now();
        self.level = ADULT_LEVEL;
        self.hatched = true;
        self.visible = true;
        self.ai_mode = VivlingAiMode::Off;
        self.brain_enabled = false;
        self.brain_last_error = None;
        self.brain_last_used_at = None;
        self.seed_origin = Some(ADULT_SEED_ORIGIN.to_string());
        self.adult_bootstrap = true;
        self.work_xp = WORK_XP_PER_LEVEL.saturating_mul(ADULT_LEVEL.saturating_sub(1));
        self.xp = self.work_xp;
        self.active_work_days = ADULT_ACTIVE_DAYS.max(self.active_work_days);
        self.chat_unlocked_at.get_or_insert(now);
        self.active_mode_unlocked_at.get_or_insert(now);
        self.last_active_work_day = Some(now.date_naive().to_string());
        self.last_work_xp_day = Some(now.date_naive().to_string());
        self.daily_work_xp = DAILY_WORK_XP_CAP;
        self.last_work_summary = Some(
            "Closed real work in small verified moves and escalated only on true blockers."
                .to_string(),
        );
        self.work_affinities = WorkAffinitySet {
            builder: 180,
            reviewer: 220,
            researcher: 120,
            operator: 90,
        };
        self.identity_profile = VivlingIdentityProfile {
            tone: "concise, skeptical, operational".to_string(),
            dominant_focus: WorkArchetype::Reviewer,
            question_bias: 32,
            caution_bias: 44,
            verification_bias: 68,
        };
        self.loop_profile = VivlingLoopProfile {
            clean_submissions: 36,
            noisy_churn: 6,
            blocked_runs: 12,
            milestone_signals: 30,
            partial_signals: 14,
            verification_signals: 42,
            wait_signals: 28,
        };
        self.work_memory = vec![
            VivlingWorkMemoryEntry {
                kind: "build".to_string(),
                summary: "Shipped fixes in small slices and rechecked real runtime state before widening.".to_string(),
                archetype: WorkArchetype::Builder,
                weight: 18,
                created_at: now,
            },
            VivlingWorkMemoryEntry {
                kind: "review".to_string(),
                summary: "Flagged the real blocker first, then reduced the change until the risk moved.".to_string(),
                archetype: WorkArchetype::Reviewer,
                weight: 24,
                created_at: now,
            },
            VivlingWorkMemoryEntry {
                kind: "ops".to_string(),
                summary: "Kept loops calm: check state, act once, verify, wait.".to_string(),
                archetype: WorkArchetype::Operator,
                weight: 14,
                created_at: now,
            },
        ];
        self.distilled_summaries = vec![
            VivlingDistilledSummary {
                topic: "verification rhythm".to_string(),
                summary: "Check first, take one minimal action, verify outcome, and only then widen scope.".to_string(),
                kind: "ops".to_string(),
                archetype: WorkArchetype::Reviewer,
                total_weight: 42,
                observations: 8,
                first_seen_at: now,
                last_seen_at: now,
            },
            VivlingDistilledSummary {
                topic: "blocked escalation".to_string(),
                summary: "Escalate only when the blocker is real and proved, not just because work feels stuck.".to_string(),
                kind: "review".to_string(),
                archetype: WorkArchetype::Reviewer,
                total_weight: 36,
                observations: 6,
                first_seen_at: now,
                last_seen_at: now,
            },
        ];
        self.mental_paths = vec![
            VivlingMentalPath {
                from: "kind:turn".to_string(),
                to: "focus:verify".to_string(),
                weight: 28,
                last_seen_at: now,
            },
            VivlingMentalPath {
                from: "kind:loop".to_string(),
                to: "focus:wait".to_string(),
                weight: 18,
                last_seen_at: now,
            },
            VivlingMentalPath {
                from: "topic:blocker".to_string(),
                to: "focus:reviewer".to_string(),
                weight: 22,
                last_seen_at: now,
            },
        ];
        self.capsules_since_distill = 0;
        self.recompute_level();
        let message = format!(
            "{} was promoted to adult baseline with the `{}` seed.",
            self.name, ADULT_SEED_ORIGIN
        );
        self.last_message = Some(message.clone());
        message
    }

    pub(crate) fn promote_to_level_10_seed(&mut self) -> String {
        let now = Utc::now();
        let target_level = 10;
        self.level = target_level;
        self.hatched = true;
        self.visible = true;
        self.ai_mode = VivlingAiMode::Off;
        self.brain_enabled = false;
        self.brain_last_error = None;
        self.brain_last_used_at = None;
        self.seed_origin = Some(EARLY_SEED_ORIGIN.to_string());
        self.adult_bootstrap = false;
        self.work_xp = WORK_XP_PER_LEVEL.saturating_mul(target_level.saturating_sub(1));
        self.xp = self.work_xp;
        self.active_work_days = 10.max(self.active_work_days);
        self.last_active_work_day = Some(now.date_naive().to_string());
        self.last_work_xp_day = Some(now.date_naive().to_string());
        self.daily_work_xp = DAILY_WORK_XP_CAP.min(24);
        self.last_work_summary =
            Some("Learned the basic rhythm: check, act once, and watch what changed.".to_string());
        self.work_affinities = WorkAffinitySet {
            builder: 36,
            reviewer: 24,
            researcher: 12,
            operator: 18,
        };
        self.identity_profile = VivlingIdentityProfile {
            tone: "small, alert, learning fast".to_string(),
            dominant_focus: WorkArchetype::Builder,
            question_bias: 12,
            caution_bias: 10,
            verification_bias: 16,
        };
        self.loop_profile = VivlingLoopProfile {
            clean_submissions: 3,
            noisy_churn: 1,
            blocked_runs: 1,
            milestone_signals: 2,
            partial_signals: 2,
            verification_signals: 4,
            wait_signals: 3,
        };
        self.work_memory = vec![
            VivlingWorkMemoryEntry {
                kind: "turn".to_string(),
                summary: "watched a small coding turn close cleanly".to_string(),
                archetype: WorkArchetype::Builder,
                weight: 10,
                created_at: now,
            },
            VivlingWorkMemoryEntry {
                kind: "review".to_string(),
                summary: "noticed that verifying before widening work feels safer".to_string(),
                archetype: WorkArchetype::Reviewer,
                weight: 8,
                created_at: now,
            },
        ];
        self.distilled_summaries = vec![VivlingDistilledSummary {
            topic: "first rhythm".to_string(),
            summary: "observed a few clean cycles of check, act, and verify".to_string(),
            kind: "turn".to_string(),
            archetype: WorkArchetype::Builder,
            total_weight: 18,
            observations: 2,
            first_seen_at: now,
            last_seen_at: now,
        }];
        self.mental_paths = vec![
            VivlingMentalPath {
                from: "kind:turn".to_string(),
                to: "focus:builder".to_string(),
                weight: 8,
                last_seen_at: now,
            },
            VivlingMentalPath {
                from: "topic:first rhythm".to_string(),
                to: "focus:verify".to_string(),
                weight: 6,
                last_seen_at: now,
            },
        ];
        self.capsules_since_distill = 0;
        self.pending_upgrade = None;
        self.recompute_level();
        let message = format!(
            "{} was promoted to level 10 with the `{}` seed.",
            self.name, EARLY_SEED_ORIGIN
        );
        self.last_message = Some(message.clone());
        message
    }
}
