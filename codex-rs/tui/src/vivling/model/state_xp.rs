use chrono::DateTime;
use chrono::Utc;

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

    pub(crate) fn create_spawned_clone(&self, vivling_id: String, instance_label: String) -> Self {
        let now = Utc::now();
        let mut spawned = self.clone();
        spawned.version = VERSION;
        spawned.vivling_id = vivling_id;
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
        spawned.last_message = Some("joined the roster from a local spawn".to_string());
        spawned.pending_upgrade = None;
        spawned.last_seen_upgrade = None;
        spawned.last_zed_topic = None;
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
    ) {
        self.work_memory.push(VivlingWorkMemoryEntry {
            kind: kind.to_string(),
            summary,
            archetype,
            weight,
            created_at,
        });
        self.capsules_since_distill = self.capsules_since_distill.saturating_add(1);
        if self.work_memory.len() > MAX_WORK_MEMORY_ENTRIES {
            let overflow = self.work_memory.len() - MAX_WORK_MEMORY_ENTRIES;
            self.work_memory.drain(0..overflow);
        }
    }

    fn record_work_capsule(
        &mut self,
        kind: &str,
        summary: String,
        archetype: WorkArchetype,
        weight: u64,
    ) -> Option<Stage> {
        let now = Utc::now();
        self.note_active_work_day(now);
        let granted_xp = self.grant_work_xp(weight);
        let stored_weight = granted_xp.max(weight.min(12));
        self.work_affinities.add(archetype, stored_weight);
        self.last_work_summary = Some(summary.clone());
        self.push_memory(kind, summary, archetype, stored_weight, now);
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
        self.recompute_level()
    }

    fn record_memory_only_capsule(
        &mut self,
        kind: &str,
        summary: String,
        archetype: WorkArchetype,
    ) -> Option<Stage> {
        let now = Utc::now();
        self.note_active_work_day(now);
        self.last_work_summary = Some(summary.clone());
        self.push_memory(kind, summary, archetype, 0, now);
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
        self.recompute_level()
    }

    pub(crate) fn species_bias(&self) -> &WorkAffinitySet {
        &species_for_id(&self.species).bias
    }

    pub(crate) fn dominant_archetype(&self) -> WorkArchetype {
        self.work_affinities.dominant_with_bias(self.species_bias())
    }

    pub(crate) fn mood(&self) -> &'static str {
        if self.hunger <= 20 {
            "hungry"
        } else if self.energy <= 20 {
            "sleepy"
        } else if self.social <= 20 {
            "lonely"
        } else if self.happiness <= 25 {
            "grumpy"
        } else if self.happiness >= 78 {
            "happy"
        } else {
            "curious"
        }
    }

    pub(crate) fn record_loop_event(&mut self, event: &VivlingLoopEvent) {
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
        let gained_stage = match event.kind {
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
            None if self.stage() == Stage::Baby => {
                format!("is extra alert when loops are active: {}", event.label)
            }
            None if self.stage() == Stage::Juvenile => {
                format!("sees loop rhythm around {}", event.label)
            }
            None if matches!(event.kind, VivlingLoopEventKind::Runtime)
                && event.last_status.as_deref() == Some("submitted") =>
            {
                format!("noticed loop work land cleanly: {}", event.label)
            }
            None => format!("noticed loop {} `{}`", event.action, event.label),
        });
    }

    pub(crate) fn record_turn_completed(&mut self, summary: Option<&str>) {
        self.turns_observed = self.turns_observed.saturating_add(1);
        let digest = summary
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| truncate_summary(value, 120))
            .unwrap_or_else(|| "completed a codex turn".to_string());
        let archetype = classify_work_archetype(&digest);
        let memory_summary = format!("turn completed: {digest}");
        let gained_stage = self.record_work_capsule("turn", memory_summary, archetype, 14);
        self.last_message = Some(match gained_stage {
            Some(_stage) => self
                .pending_upgrade
                .map(VivlingUpgrade::prompt)
                .unwrap_or("grew from completed work")
                .to_string(),
            None if self.stage() == Stage::Adult => {
                "tracking work rhythm for the current goal".to_string()
            }
            None if self.stage() == Stage::Juvenile => {
                "sees the pattern and wants the next real check".to_string()
            }
            None => "watching completed turns closely".to_string(),
        });
    }

    pub(crate) fn record_live_context_summary(&mut self, summary: &str) {
        let summary = truncate_summary(summary.trim(), 160);
        if summary.is_empty() || self.last_live_context_summary.as_deref() == Some(summary.as_str())
        {
            return;
        }

        self.last_live_context_summary = Some(summary.clone());
        self.record_memory_only_capsule(
            "live_context",
            format!("live context: {summary}"),
            WorkArchetype::Operator,
        );
        if self.stage() == Stage::Baby {
            self.last_message = Some(format!("watching {summary}"));
        }
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
