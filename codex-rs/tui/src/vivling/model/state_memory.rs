use chrono::DateTime;
use chrono::Utc;
use std::collections::BTreeMap;

use super::*;

impl VivlingState {
    pub(super) fn normalize_species(&mut self) {
        let species = species_for_id(&self.species);
        self.species = species.id.clone();
        self.rarity = species.rarity.label().to_string();
    }

    pub(super) fn backfill_capsule_metadata(&mut self) {
        for capsule in &mut self.work_memory {
            if capsule.weight == 0
                && !matches!(
                    capsule.kind.as_str(),
                    "loop_config"
                        | "loop_blocked_review"
                        | "loop_blocked_side"
                        | "loop_blocked_busy"
                )
            {
                capsule.weight = 12;
            }
            if capsule.summary.trim().is_empty() {
                capsule.summary = "remembered an older work step".to_string();
            }
            if matches!(capsule.archetype, WorkArchetype::Builder)
                && capsule.kind == "turn"
                && capsule.summary.contains("docs")
            {
                capsule.archetype = WorkArchetype::Researcher;
            } else if capsule.kind.starts_with("loop") {
                capsule.archetype = WorkArchetype::Operator;
            }
        }
    }

    pub(super) fn recompute_progress_from_memory(&mut self) {
        let mut active_days = std::collections::HashSet::new();
        let mut daily_xp: Vec<(String, u64)> = Vec::new();
        self.work_affinities = WorkAffinitySet::default();
        self.work_xp = 0;
        self.loop_exposure = 0;
        self.loop_runtime_submissions = 0;
        self.loop_runtime_blocks = 0;
        self.loop_admin_churn = 0;
        self.loop_blocked_review = 0;
        self.loop_blocked_side = 0;
        self.loop_blocked_busy = 0;
        self.turns_observed = 0;
        for capsule in &self.work_memory {
            let day_key = capsule.created_at.date_naive().format("%F").to_string();
            active_days.insert(day_key.clone());
            self.work_affinities.add(capsule.archetype, capsule.weight);
            if capsule.kind.starts_with("loop") {
                self.loop_exposure = self.loop_exposure.saturating_add(1);
            }
            match capsule.kind.as_str() {
                "loop_runtime" => {
                    self.loop_runtime_submissions = self.loop_runtime_submissions.saturating_add(1);
                }
                "loop_config" => {
                    self.loop_admin_churn = self.loop_admin_churn.saturating_add(1);
                }
                "loop_blocked_review" => {
                    self.loop_runtime_blocks = self.loop_runtime_blocks.saturating_add(1);
                    self.loop_blocked_review = self.loop_blocked_review.saturating_add(1);
                }
                "loop_blocked_side" => {
                    self.loop_runtime_blocks = self.loop_runtime_blocks.saturating_add(1);
                    self.loop_blocked_side = self.loop_blocked_side.saturating_add(1);
                }
                "loop_blocked_busy" => {
                    self.loop_runtime_blocks = self.loop_runtime_blocks.saturating_add(1);
                    self.loop_blocked_busy = self.loop_blocked_busy.saturating_add(1);
                }
                _ => {}
            }
            if capsule.kind == "turn" {
                self.turns_observed = self.turns_observed.saturating_add(1);
            }
            if let Some((_, total)) = daily_xp.iter_mut().find(|(day, _)| *day == day_key) {
                *total = (*total).saturating_add(capsule.weight);
            } else {
                daily_xp.push((day_key, capsule.weight));
            }
        }
        self.active_work_days = active_days.len() as u64;
        self.work_xp = daily_xp
            .into_iter()
            .map(|(_, total)| total.min(DAILY_WORK_XP_CAP))
            .sum();
        self.xp = self.work_xp;
    }

    pub(super) fn rebuild_learning_profiles(&mut self) {
        let dominant = self.dominant_archetype();
        let verification_bias = self
            .work_memory
            .iter()
            .filter(|capsule| {
                contains_any(
                    &capsule.summary.to_ascii_lowercase(),
                    &["verify", "verified", "smoke", "check", "status real"],
                )
            })
            .count() as u64
            + self.loop_profile.verification_signals;
        let caution_bias = self.loop_runtime_blocks + self.loop_admin_churn;
        let question_bias = self.loop_profile.partial_signals
            + self.loop_runtime_blocks
            + (self.turns_observed / 3);
        let tone = if caution_bias >= 4 {
            "skeptical"
        } else if verification_bias >= 3 {
            "precise"
        } else if dominant == WorkArchetype::Researcher {
            "curious"
        } else if dominant == WorkArchetype::Reviewer {
            "sharp"
        } else {
            "steady"
        };
        self.identity_profile = VivlingIdentityProfile {
            tone: tone.to_string(),
            dominant_focus: dominant,
            question_bias,
            caution_bias,
            verification_bias,
        };
    }

    pub(super) fn reinforce_mental_path(
        &mut self,
        from: impl Into<String>,
        to: impl Into<String>,
        weight: u64,
        now: DateTime<Utc>,
    ) {
        let from = from.into();
        let to = to.into();
        if let Some(existing) = self
            .mental_paths
            .iter_mut()
            .find(|entry| entry.from == from && entry.to == to)
        {
            existing.weight = existing.weight.saturating_add(weight);
            existing.last_seen_at = now;
        } else {
            self.mental_paths.push(VivlingMentalPath {
                from,
                to,
                weight,
                last_seen_at: now,
            });
        }
        self.mental_paths.sort_by(|a, b| {
            b.weight
                .cmp(&a.weight)
                .then_with(|| b.last_seen_at.cmp(&a.last_seen_at))
        });
        if self.mental_paths.len() > MAX_MENTAL_PATH_ENTRIES {
            self.mental_paths.truncate(MAX_MENTAL_PATH_ENTRIES);
        }
    }

    pub(super) fn infer_semantic_topic(kind: &str, summary: &str) -> &'static str {
        let normalized = summary.to_ascii_lowercase();
        if contains_any(&normalized, &["milestone", "ready to test", "closed"]) {
            "milestone"
        } else if contains_any(
            &normalized,
            &["partial", "started", "in progress", "parallel"],
        ) {
            "partial_progress"
        } else if contains_any(&normalized, &["verify", "verified", "check", "smoke"]) {
            "verify"
        } else if contains_any(&normalized, &["wait", "waiting", "pending"]) {
            "wait"
        } else if kind.contains("blocked") {
            "block"
        } else if kind == "loop_config" {
            "churn"
        } else {
            "work_pattern"
        }
    }

    pub(super) fn record_semantic_signal(&mut self, topic: &str) {
        match topic {
            "milestone" => self.loop_profile.milestone_signals += 1,
            "partial_progress" => self.loop_profile.partial_signals += 1,
            "verify" => self.loop_profile.verification_signals += 1,
            "wait" => self.loop_profile.wait_signals += 1,
            "block" => self.loop_profile.blocked_runs += 1,
            "churn" => self.loop_profile.noisy_churn += 1,
            _ => {}
        }
    }

    pub(super) fn maybe_distill_memory(&mut self) {
        let should_distill = self.capsules_since_distill >= DISTILL_TRIGGER_CAPSULES
            || self.work_memory.len() >= MAX_WORK_MEMORY_ENTRIES.saturating_sub(8);
        if !should_distill {
            return;
        }
        self.distill_memory();
    }

    pub(super) fn distill_memory(&mut self) {
        if self.work_memory.len() < 4 {
            return;
        }
        let now = Utc::now();
        let keep_recent = 8usize.min(self.work_memory.len());
        let distill_len = self.work_memory.len().saturating_sub(keep_recent);
        if distill_len == 0 {
            return;
        }
        let candidates = self.work_memory[..distill_len].to_vec();
        let mut grouped: BTreeMap<(String, WorkArchetype, String), Vec<VivlingWorkMemoryEntry>> =
            BTreeMap::new();
        for capsule in &candidates {
            let topic = Self::infer_semantic_topic(&capsule.kind, &capsule.summary).to_string();
            grouped
                .entry((capsule.kind.clone(), capsule.archetype, topic))
                .or_default()
                .push(capsule.clone());
        }
        for ((kind, archetype, topic), group) in grouped {
            let observations = group.len() as u64;
            let total_weight = group.iter().map(|entry| entry.weight).sum::<u64>();
            let first_seen_at = group.first().map(|entry| entry.created_at).unwrap_or(now);
            let last_seen_at = group.last().map(|entry| entry.created_at).unwrap_or(now);
            let latest = group
                .last()
                .map(|entry| truncate_summary(&entry.summary, 72))
                .unwrap_or_else(|| "tracked work rhythm".to_string());
            let summary =
                format!("observed {observations} {kind} patterns around {topic}; latest: {latest}");
            if let Some(existing) = self
                .distilled_summaries
                .iter_mut()
                .find(|entry| entry.kind == kind && entry.topic == topic)
            {
                existing.summary = summary.clone();
                existing.total_weight = existing.total_weight.saturating_add(total_weight);
                existing.observations = existing.observations.saturating_add(observations);
                existing.last_seen_at = last_seen_at;
            } else {
                self.distilled_summaries.push(VivlingDistilledSummary {
                    topic: topic.clone(),
                    summary: summary.clone(),
                    kind: kind.clone(),
                    archetype,
                    total_weight,
                    observations,
                    first_seen_at,
                    last_seen_at,
                });
            }
            self.record_semantic_signal(&topic);
            self.reinforce_mental_path(
                format!("kind:{kind}"),
                format!("topic:{topic}"),
                observations.max(1),
                last_seen_at,
            );
            self.reinforce_mental_path(
                format!("topic:{topic}"),
                format!("focus:{}", archetype.label()),
                total_weight.max(1),
                last_seen_at,
            );
        }
        for path in &mut self.mental_paths {
            path.weight = path.weight.saturating_sub(1);
        }
        self.mental_paths.retain(|path| path.weight > 0);
        self.distilled_summaries.sort_by(|a, b| {
            b.total_weight
                .cmp(&a.total_weight)
                .then_with(|| b.last_seen_at.cmp(&a.last_seen_at))
        });
        if self.distilled_summaries.len() > MAX_DISTILLED_MEMORY_ENTRIES {
            self.distilled_summaries
                .truncate(MAX_DISTILLED_MEMORY_ENTRIES);
        }
        self.capsules_since_distill = 0;
        self.rebuild_learning_profiles();
    }
}
