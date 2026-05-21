use chrono::DateTime;
use chrono::Utc;

use super::text_utils::fnv1a64;
use super::*;

impl VivlingState {
    pub(crate) fn stage(&self) -> Stage {
        if self.level >= ADULT_LEVEL {
            Stage::Adult
        } else if self.level >= JUVENILE_LEVEL {
            Stage::Juvenile
        } else {
            Stage::Baby
        }
    }

    pub(crate) fn local_spawn_slots_unlocked(&self) -> usize {
        (self.level / SPAWN_SLOT_LEVEL_STEP) as usize
    }

    pub(crate) fn export_unlocked(&self) -> bool {
        self.level >= JUVENILE_LEVEL
    }

    pub(crate) fn lineage_role_label(&self) -> &'static str {
        if self.is_imported {
            "imported"
        } else if self.is_primary {
            "primary"
        } else {
            "spawned"
        }
    }

    pub(crate) fn create_spawned_offspring(
        &self,
        vivling_id: String,
        instance_label: String,
    ) -> Self {
        let now = Utc::now();
        let hash = fnv1a64(format!("{}:{vivling_id}", self.primary_vivling_id).as_bytes());
        let mut spawned = self.clone();
        spawned.version = VERSION;
        spawned.hatched = true;
        spawned.visible = true;
        spawned.seed_hash = format!("{hash:016x}");
        spawned.gene_vector =
            VivlingGeneVector::inherit_from(&self.gene_vector, &spawned.seed_hash);
        spawned.vivling_id = vivling_id;
        spawned.install_id = None;
        spawned.origin_install_id = self.origin_install_id.clone();
        spawned.primary_vivling_id = self.primary_vivling_id.clone();
        spawned.parent_vivling_id = Some(self.vivling_id.clone());
        spawned.spawn_generation = self.spawn_generation.saturating_add(1);
        spawned.is_primary = false;
        spawned.is_imported = false;
        spawned.imported_at = None;
        spawned.import_source = None;
        spawned.export_count = 0;
        spawned.instance_label = Some(instance_label);
        spawned.created_at = Some(now);
        spawned.last_seen_at = Some(now);
        spawned.last_fed_at = Some(now);
        spawned.xp = 0;
        spawned.level = 1;
        spawned.hunger = 82;
        spawned.energy = 76;
        spawned.happiness = 70;
        spawned.social = 62;
        spawned.meals = 0;
        spawned.pets = 0;
        spawned.plays = 0;
        spawned.sleeps = 0;
        spawned.observations = 0;
        spawned.ai_mode = VivlingAiMode::Off;
        spawned.brain_enabled = false;
        spawned.brain_profile = None;
        spawned.brain_last_error = None;
        spawned.brain_last_used_at = None;
        spawned.seed_origin = None;
        spawned.adult_bootstrap = false;
        spawned.work_xp = 0;
        spawned.loop_exposure = 0;
        spawned.loop_runtime_submissions = 0;
        spawned.loop_runtime_blocks = 0;
        spawned.loop_admin_churn = 0;
        spawned.loop_blocked_review = 0;
        spawned.loop_blocked_side = 0;
        spawned.loop_blocked_busy = 0;
        spawned.turns_observed = 0;
        spawned.suggestions_made = 0;
        spawned.active_work_days = 0;
        spawned.last_active_work_day = None;
        spawned.last_work_xp_day = None;
        spawned.daily_work_xp = 0;
        spawned.chat_unlocked_at = None;
        spawned.active_mode_unlocked_at = None;
        spawned.last_work_summary = None;
        spawned.last_live_context_summary = None;
        spawned.work_affinities = WorkAffinitySet::default();
        spawned.work_memory = Vec::new();
        spawned.distilled_summaries = Vec::new();
        spawned.mental_paths = Vec::new();
        spawned.identity_profile = VivlingIdentityProfile::default();
        spawned.loop_profile = VivlingLoopProfile::default();
        spawned.capsules_since_distill = 0;
        spawned.last_message = Some("joined the roster from a local spawn".to_string());
        spawned.pending_upgrade = None;
        spawned.last_seen_upgrade = None;
        spawned.last_zed_topic = None;
        spawned.bond = crate::vivling::VivlingBond::for_offspring();
        spawned.lineage_seen_parent_summary_keys = Vec::new();
        spawned.lineage_rarity_pressure_pct = 0;
        spawned.cultural_parent_vivling_id = Some(self.vivling_id.clone());
        spawned.lineage_blessed = false;

        // Memory V2 Step 10.A — Axis D lineage inheritance seed.
        //
        // The child carries a snapshot of the parent's identity in a
        // dedicated field instead of inheriting voice/bias/cache/lang
        // as its own values. This lets a future runtime read the seed
        // and decide *how much* of the parent to honour, without
        // pretending the child has already lived the parent's life.
        //
        // The child therefore explicitly drops the V2 fields:
        //   - `self_voice` (parent's auto-written paragraph lives in
        //     `lineage_inheritance.voice_fragment` instead);
        //   - `accumulated_bias` / `recent_bias` counters (parent's
        //     emphasis lives in `lineage_inheritance.preference_seed`
        //     bias seeds);
        //   - `language_state` (detected language, language_override,
        //     mode, recent user samples). Round-2 fix: the previous
        //     build let the clone carry the parent's samples and
        //     override into the child, so a freshly hatched offspring
        //     would have inherited Strict/Spanish without ever having
        //     heard its owner speak. Language/culture inheritance
        //     runtime can hydrate from the seed in a later step.
        //   - the volatile CRT / proactive caches (already
        //     `#[serde(skip)]`, but we reset them so a clone-time
        //     value cannot leak across).
        spawned.lineage_inheritance = Some(build_lineage_inheritance_seed(self));
        spawned.self_voice = None;
        spawned.accumulated_bias = codex_vivling_core::model::BiasCounters::default();
        spawned.recent_bias = codex_vivling_core::model::BiasCounters::default();
        spawned.language_state = codex_vivling_core::model::VivlingLanguageState::default();
        spawned.cached_crt_phrase = None;
        spawned.cached_proactive = None;

        // codex-vl: lineage rarity pressure — dentro-specie quality roll.
        // Never swaps species (kept inherited via clone). May lift gene
        // temperaments and brain_potential when the deterministic trigger
        // fires. Pressure ≥ LINEAGE_BLESSED_PRESSURE_THRESHOLD on a
        // successful trigger marks the offspring as `lineage_blessed`.
        let pressure = self.lineage_rarity_pressure_pct;
        let triggered =
            super::lineage::apply_lineage_quality_roll(&mut spawned.gene_vector, hash, pressure);
        if triggered && super::lineage::is_lineage_blessed_threshold(pressure) {
            spawned.lineage_blessed = true;
        }
        spawned
    }

    fn level_cap_from_active_days(&self) -> u64 {
        if self.active_work_days < JUVENILE_ACTIVE_DAYS {
            JUVENILE_LEVEL - 1
        } else if self.active_work_days < ADULT_ACTIVE_DAYS {
            ADULT_LEVEL - 1
        } else {
            MAX_LEVEL
        }
    }

    pub(crate) fn recompute_level(&mut self) -> Option<Stage> {
        let previous_stage = self.stage();
        let raw_level = (self.work_xp / WORK_XP_PER_LEVEL)
            .saturating_add(1)
            .clamp(1, MAX_LEVEL);
        self.level = raw_level.min(self.level_cap_from_active_days()).max(1);
        self.xp = self.work_xp;
        let next_stage = self.stage();
        if next_stage != previous_stage {
            let now = Utc::now();
            if next_stage != Stage::Baby && self.chat_unlocked_at.is_none() {
                self.chat_unlocked_at = Some(now);
            }
            if next_stage == Stage::Adult && self.active_mode_unlocked_at.is_none() {
                self.active_mode_unlocked_at = Some(now);
            }
            self.pending_upgrade = match next_stage {
                Stage::Baby => None,
                Stage::Juvenile => Some(VivlingUpgrade::YoungVoice),
                Stage::Adult => Some(VivlingUpgrade::ActiveMode),
            };
            self.apply_stage_unlocks(next_stage);
            return Some(next_stage);
        }
        None
    }

    fn note_active_work_day(&mut self, now: DateTime<Utc>) {
        let day_key = now.date_naive().format("%F").to_string();
        if self.last_active_work_day.as_deref() != Some(day_key.as_str()) {
            self.active_work_days = self.active_work_days.saturating_add(1);
            self.last_active_work_day = Some(day_key.clone());
        }
        if self.last_work_xp_day.as_deref() != Some(day_key.as_str()) {
            self.last_work_xp_day = Some(day_key);
            self.daily_work_xp = 0;
        }
    }

    fn grant_work_xp(&mut self, weight: u64) -> u64 {
        let remaining = DAILY_WORK_XP_CAP.saturating_sub(self.daily_work_xp);
        let granted = remaining.min(weight);
        self.daily_work_xp = self.daily_work_xp.saturating_add(granted);
        self.work_xp = self.work_xp.saturating_add(granted);
        self.xp = self.work_xp;
        granted
    }

    fn push_memory(
        &mut self,
        kind: &str,
        summary: String,
        archetype: WorkArchetype,
        weight: u64,
        created_at: DateTime<Utc>,
    ) -> VivlingWorkMemoryEntry {
        let entry = VivlingWorkMemoryEntry {
            kind: kind.to_string(),
            summary,
            archetype,
            weight,
            created_at,
        };
        self.work_memory.push(entry.clone());
        self.capsules_since_distill = self.capsules_since_distill.saturating_add(1);
        if self.work_memory.len() > MAX_WORK_MEMORY_ENTRIES {
            let overflow = self.work_memory.len() - MAX_WORK_MEMORY_ENTRIES;
            self.work_memory.drain(0..overflow);
        }
        entry
    }

    fn record_work_capsule(
        &mut self,
        kind: &str,
        summary: String,
        archetype: WorkArchetype,
        weight: u64,
    ) -> (Option<Stage>, VivlingWorkMemoryEntry) {
        let now = Utc::now();
        self.note_active_work_day(now);
        let granted_xp = self.grant_work_xp(weight);
        let stored_weight = granted_xp.max(weight.min(12));
        self.work_affinities.add(archetype, stored_weight);
        self.last_work_summary = Some(summary.clone());
        let entry = self.push_memory(kind, summary, archetype, stored_weight, now);
        self.record_semantic_signal(Self::infer_semantic_topic(
            kind,
            self.last_work_summary.as_deref().unwrap_or(""),
        ));
        self.reinforce_mental_path(
            format!("kind:{kind}"),
            format!("focus:{}", archetype.label()),
            stored_weight.max(1),
            now,
        );
        self.maybe_distill_memory();
        self.rebuild_learning_profiles();
        (self.recompute_level(), entry)
    }

    fn record_memory_only_capsule(
        &mut self,
        kind: &str,
        summary: String,
        archetype: WorkArchetype,
    ) -> (Option<Stage>, VivlingWorkMemoryEntry) {
        let now = Utc::now();
        self.note_active_work_day(now);
        self.last_work_summary = Some(summary.clone());
        let entry = self.push_memory(kind, summary, archetype, 0, now);
        self.record_semantic_signal(Self::infer_semantic_topic(
            kind,
            self.last_work_summary.as_deref().unwrap_or(""),
        ));
        self.reinforce_mental_path(
            format!("kind:{kind}"),
            format!("focus:{}", archetype.label()),
            1,
            now,
        );
        self.maybe_distill_memory();
        self.rebuild_learning_profiles();
        (self.recompute_level(), entry)
    }

    pub(crate) fn species_bias(&self) -> &WorkAffinitySet {
        &species_for_id(&self.species).bias
    }

    pub(crate) fn dominant_archetype(&self) -> WorkArchetype {
        dominant_with_genes(
            &self.work_affinities,
            self.species_bias(),
            &self.gene_vector,
        )
    }

    pub(crate) fn mood(&self) -> &'static str {
        let lonely_threshold =
            15 + i64::from(90u8.saturating_sub(self.gene_vector.sociability) / 5);
        let grumpy_threshold = 20 + i64::from(90u8.saturating_sub(self.gene_vector.patience) / 6);
        if self.hunger <= 20 {
            "hungry"
        } else if self.energy <= 20 {
            "sleepy"
        } else if self.social <= lonely_threshold {
            "lonely"
        } else if self.happiness <= grumpy_threshold {
            "grumpy"
        } else if self.happiness >= 78 {
            "happy"
        } else {
            "curious"
        }
    }

    pub(crate) fn record_loop_event(
        &mut self,
        event: &VivlingLoopEvent,
    ) -> Vec<VivlingWorkMemoryEntry> {
        self.loop_exposure = self.loop_exposure.saturating_add(1);
        let source = match event.source {
            VivlingLoopEventSource::User => "user",
            VivlingLoopEventSource::Agent => "agent",
        };
        let summary = match (
            event.goal.as_deref(),
            event.runtime_state.as_deref(),
            event.last_status.as_deref(),
        ) {
            (Some(goal), Some(runtime_state), Some(last_status)) => format!(
                "loop {} `{}` for {goal} ({runtime_state}, status {last_status}, {source})",
                event.action, event.label
            ),
            (Some(goal), Some(runtime_state), None) => format!(
                "loop {} `{}` for {goal} ({runtime_state}, {source})",
                event.action, event.label
            ),
            (Some(goal), None, Some(last_status)) => format!(
                "loop {} `{}` for {goal} (status {last_status}, {source})",
                event.action, event.label
            ),
            (Some(goal), None, None) => {
                format!(
                    "loop {} `{}` for {goal} ({source})",
                    event.action, event.label
                )
            }
            (None, Some(runtime_state), Some(last_status)) => format!(
                "loop {} `{}` ({runtime_state}, status {last_status}, {source})",
                event.action, event.label
            ),
            (None, Some(runtime_state), None) => {
                format!(
                    "loop {} `{}` ({runtime_state}, {source})",
                    event.action, event.label
                )
            }
            (None, None, Some(last_status)) => format!(
                "loop {} `{}` (status {last_status}, {source})",
                event.action, event.label
            ),
            (None, None, None) => format!("loop {} `{}` ({source})", event.action, event.label),
        };
        let (gained_stage, entry) = match event.kind {
            VivlingLoopEventKind::Config => {
                self.loop_admin_churn = self.loop_admin_churn.saturating_add(1);
                let weight = match event.action.as_str() {
                    "add" | "enable" => 4,
                    "update" => 1,
                    "disable" | "remove" | "trigger" => 0,
                    _ => 0,
                };
                if weight == 0 {
                    self.record_memory_only_capsule(
                        "loop_config",
                        summary.clone(),
                        WorkArchetype::Operator,
                    )
                } else {
                    self.record_work_capsule(
                        "loop_config",
                        summary.clone(),
                        WorkArchetype::Operator,
                        weight,
                    )
                }
            }
            VivlingLoopEventKind::Runtime => match event.last_status.as_deref() {
                Some("submitted") => {
                    self.loop_runtime_submissions = self.loop_runtime_submissions.saturating_add(1);
                    self.record_work_capsule(
                        "loop_runtime",
                        summary.clone(),
                        WorkArchetype::Operator,
                        14,
                    )
                }
                Some("blocked_review") => {
                    self.loop_runtime_blocks = self.loop_runtime_blocks.saturating_add(1);
                    self.loop_blocked_review = self.loop_blocked_review.saturating_add(1);
                    self.record_memory_only_capsule(
                        "loop_blocked_review",
                        summary.clone(),
                        WorkArchetype::Operator,
                    )
                }
                Some("blocked_side") => {
                    self.loop_runtime_blocks = self.loop_runtime_blocks.saturating_add(1);
                    self.loop_blocked_side = self.loop_blocked_side.saturating_add(1);
                    self.record_memory_only_capsule(
                        "loop_blocked_side",
                        summary.clone(),
                        WorkArchetype::Operator,
                    )
                }
                Some("pending_busy") => {
                    self.loop_runtime_blocks = self.loop_runtime_blocks.saturating_add(1);
                    self.loop_blocked_busy = self.loop_blocked_busy.saturating_add(1);
                    self.record_memory_only_capsule(
                        "loop_blocked_busy",
                        summary.clone(),
                        WorkArchetype::Operator,
                    )
                }
                _ => self.record_memory_only_capsule(
                    "loop_runtime",
                    summary.clone(),
                    WorkArchetype::Operator,
                ),
            },
        };
        self.last_message = Some(match gained_stage {
            Some(_stage) => self
                .pending_upgrade
                .map(VivlingUpgrade::prompt)
                .unwrap_or("is growing with loop work")
                .to_string(),
            None if self.stage() == Stage::Baby => match event.kind {
                VivlingLoopEventKind::Config => {
                    format!("loop {}? keep one goal", event.label)
                }
                VivlingLoopEventKind::Runtime
                    if event.last_status.as_deref() == Some("submitted") =>
                {
                    format!("loop {} landed!", event.label)
                }
                VivlingLoopEventKind::Runtime => {
                    format!("loop {}.. what blocks it?", event.label)
                }
            },
            None if self.stage() == Stage::Juvenile => {
                if event.kind == VivlingLoopEventKind::Runtime
                    && event.last_status.as_deref() == Some("submitted")
                {
                    format!("{} clean. verify before next", event.label)
                } else if event.kind == VivlingLoopEventKind::Runtime {
                    format!("{} stuck. check the block", event.label)
                } else {
                    format!("loop {}. keep it tight", event.label)
                }
            }
            None if matches!(event.kind, VivlingLoopEventKind::Runtime)
                && event.last_status.as_deref() == Some("submitted") =>
            {
                format!("{} landed. rhythm good", event.label)
            }
            None if event.kind == VivlingLoopEventKind::Runtime => {
                format!("{} blocked. I can check", event.label)
            }
            None => format!("loop {} `{}` noted", event.action, event.label),
        });
        vec![entry]
    }

    pub(crate) fn record_turn_completed(
        &mut self,
        summary: Option<&str>,
    ) -> Vec<VivlingWorkMemoryEntry> {
        self.turns_observed = self.turns_observed.saturating_add(1);
        let digest = summary
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| truncate_summary(value, 120))
            .unwrap_or_else(|| "completed a codex turn".to_string());
        let archetype = classify_work_archetype(&digest);
        let memory_summary = format!("turn completed: {digest}");
        let (gained_stage, entry) = self.record_work_capsule("turn", memory_summary, archetype, 14);
        self.last_message = Some(match gained_stage {
            Some(_stage) => self
                .pending_upgrade
                .map(VivlingUpgrade::prompt)
                .unwrap_or("grew from completed work")
                .to_string(),
            None if self.stage() == Stage::Adult => match archetype {
                WorkArchetype::Builder => "build landed. keep the diff narrow".to_string(),
                WorkArchetype::Reviewer => "review moved. name remaining risk".to_string(),
                WorkArchetype::Researcher => "learned enough. choose one unknown".to_string(),
                WorkArchetype::Operator => "state changed. verify before next wake".to_string(),
            },
            None if self.stage() == Stage::Juvenile => match archetype {
                WorkArchetype::Builder => "built. test it now?".to_string(),
                WorkArchetype::Reviewer => "reviewed. risk moved?".to_string(),
                WorkArchetype::Researcher => "learned. clarify next?".to_string(),
                WorkArchetype::Operator => "ops done. state changed?".to_string(),
            },
            None => match archetype {
                WorkArchetype::Builder => "done! what's next?".to_string(),
                WorkArchetype::Reviewer => "checked. safe now?".to_string(),
                WorkArchetype::Researcher => "understood. more to learn?".to_string(),
                WorkArchetype::Operator => "done. loop ok?".to_string(),
            },
        });
        vec![entry]
    }

    pub(crate) fn record_live_context_summary(
        &mut self,
        summary: &str,
    ) -> Vec<VivlingWorkMemoryEntry> {
        let summary = truncate_summary(summary.trim(), 160);
        if summary.is_empty() || low_signal_live_context_summary(&summary) {
            return Vec::new();
        }
        let normalized = normalize_live_context_summary(&summary);
        let last_normalized = self
            .last_live_context_summary
            .as_deref()
            .map(normalize_live_context_summary);
        if last_normalized.as_deref() == Some(normalized.as_str()) {
            return Vec::new();
        }

        self.last_live_context_summary = Some(summary.clone());
        let (_gained_stage, entry) = self.record_memory_only_capsule(
            "live_context",
            format!("live context: {summary}"),
            WorkArchetype::Operator,
        );
        if self.stage() == Stage::Baby {
            self.last_message = Some(format!("context: {summary}"));
        } else if self.stage() == Stage::Juvenile {
            self.last_message = Some(format!("tracking {summary}"));
        }
        vec![entry]
    }

    pub(crate) fn memory_digest(&self) -> String {
        if self.work_memory.is_empty() {
            return format!("{} is still tiny. No work memory yet.", self.name);
        }
        let paths = self
            .mental_paths
            .iter()
            .take(3)
            .map(|path| format!("{} -> {} ({})", path.from, path.to, path.weight))
            .collect::<Vec<_>>()
            .join(", ");
        let recent = self
            .work_memory
            .iter()
            .rev()
            .take(5)
            .map(|entry| {
                format!(
                    "- {} [{}]: {}",
                    entry.kind,
                    entry.archetype.label(),
                    entry.summary
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            "level {} · active_days {} · dna {} · recent {} · distilled {} · paths {}\nstrongest: {}\n{}",
            self.level,
            self.active_work_days,
            self.dominant_archetype().label(),
            self.work_memory.len(),
            self.distilled_summaries.len(),
            self.mental_paths.len(),
            if paths.is_empty() {
                "still forming".to_string()
            } else {
                paths
            },
            recent
        )
    }

    pub(crate) fn memory_recap(&self) -> String {
        if self.work_memory.is_empty() {
            return format!(
                "{} is still tiny. No learned memory to recap yet.",
                self.name
            );
        }
        let strongest_summaries = if self.distilled_summaries.is_empty() {
            "still distilling patterns".to_string()
        } else {
            self.distilled_summaries
                .iter()
                .take(3)
                .map(|entry| format!("{}: {}", entry.topic, truncate_summary(&entry.summary, 72)))
                .collect::<Vec<_>>()
                .join(" | ")
        };
        let strongest_paths = if self.mental_paths.is_empty() {
            "paths still forming".to_string()
        } else {
            self.mental_paths
                .iter()
                .take(3)
                .map(|path| format!("{} -> {} ({})", path.from, path.to, path.weight))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let recent = self
            .work_memory
            .iter()
            .rev()
            .take(3)
            .map(|entry| truncate_summary(&entry.summary, 72))
            .collect::<Vec<_>>()
            .join(" | ");
        format!(
            "{} · stage {} · level {} · dna {}\nfocus: {}\nrecent: {}\ndistilled: {}\npaths: {}",
            self.name,
            self.stage().label(),
            self.level,
            self.dominant_archetype().label(),
            self.identity_profile.tone,
            recent,
            strongest_summaries,
            strongest_paths
        )
    }
}

/// Memory V2 Step 10.A — build the lineage inheritance seed a freshly
/// spawned child carries from its cultural parent.
///
/// Pure: no I/O, no schema bump. The `skills` slot is left empty by
/// design — `_skills.json` lives outside the model layer and a future
/// runtime-level step can hydrate it without changing this signature.
/// `voice_fragment` is bounded to 240 characters so a future
/// LLM-enriched voice cannot bloat a child's state file.
fn build_lineage_inheritance_seed(
    parent: &VivlingState,
) -> codex_vivling_core::model::LineageInheritance {
    use codex_vivling_core::model::LineageInheritance;
    use codex_vivling_core::model::VivlingPreferenceSeed;

    let voice_fragment = parent
        .self_voice
        .as_ref()
        .map(|voice| truncate_summary(voice.text.trim(), 240))
        .filter(|fragment| !fragment.trim().is_empty());

    let caution_bias_seed = if parent.accumulated_bias.caution > 0 {
        parent.accumulated_bias.caution
    } else {
        parent.identity_profile.caution_bias
    };
    let verification_bias_seed = if parent.accumulated_bias.verification > 0 {
        parent.accumulated_bias.verification
    } else {
        parent.identity_profile.verification_bias
    };

    LineageInheritance {
        voice_fragment,
        skills: Vec::new(),
        preference_seed: VivlingPreferenceSeed {
            caution_bias_seed,
            verification_bias_seed,
            preferred_archetype: parent.dominant_archetype(),
        },
        suggested_brain_profile: parent.brain_profile.clone(),
    }
}

fn normalize_live_context_summary(summary: &str) -> String {
    summary
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_matches(';')
        .to_ascii_lowercase()
}

fn low_signal_live_context_summary(summary: &str) -> bool {
    let normalized = normalize_live_context_summary(summary);
    let has_actionable_part = normalized.contains("task ")
        || normalized.contains("branch ")
        || normalized.contains("active ");
    if has_actionable_part {
        return false;
    }
    normalized.contains("state ") || normalized.contains("cwd ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_context_memory_skips_low_signal_state_only_updates() {
        let mut state = VivlingState::default();
        let entries = state.record_live_context_summary("state Working; cwd codex-vl");
        assert!(entries.is_empty());
        assert!(state.last_live_context_summary.is_none());
    }

    #[test]
    fn live_context_memory_deduplicates_normalized_repeats() {
        let mut state = VivlingState::default();
        let first = state.record_live_context_summary(
            "state Working; active main; task verify build; branch develop",
        );
        let second = state.record_live_context_summary(
            " state   Working; active main; task verify build; branch develop ",
        );

        assert_eq!(first.len(), 1);
        assert!(second.is_empty());
        assert_eq!(
            state
                .work_memory
                .iter()
                .filter(|entry| entry.kind == "live_context")
                .count(),
            1
        );
    }
}
