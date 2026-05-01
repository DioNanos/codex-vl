use super::*;

impl VivlingState {
    #[cfg(test)]
    pub(crate) fn suggest(&mut self) -> String {
        self.suggestions_made = self.suggestions_made.saturating_add(1);
        let suggestion = match self.stage() {
            Stage::Baby => {
                if self.loop_profile.noisy_churn > 1 {
                    "I am still small, but I already see loop churn. Keep one clear goal."
                        .to_string()
                } else {
                    "I am still small. Give me real work and one clear next check.".to_string()
                }
            }
            Stage::Juvenile => {
                if self.loop_runtime_blocks >= 2 {
                    "I see friction. Is the loop blocked by review, side work, or just busy turns?"
                        .to_string()
                } else if self.loop_profile.partial_signals > self.loop_profile.milestone_signals {
                    "This feels like progress, not closure. What still proves the milestone?"
                        .to_string()
                } else if self.loop_runtime_submissions >= 2 && self.loop_admin_churn <= 1 {
                    "The rhythm looks good. Keep one focused loop and verify before widening."
                        .to_string()
                } else if self.loop_admin_churn >= 3 {
                    "I see churn. Tighten the goal and stop touching the loop unless state changed."
                        .to_string()
                } else {
                    "I am learning fast now. What is the one real next check for this work?"
                        .to_string()
                }
            }
            Stage::Adult => match self.ai_mode {
                VivlingAiMode::On if self.loop_blocked_busy > 0 => {
                    "Busy-turn friction is high. My next move would be verify state, then wait."
                        .to_string()
                }
                VivlingAiMode::On if self.loop_blocked_review > 0 => {
                    "Review is the real gate. Close review first, then let the loop breathe."
                        .to_string()
                }
                VivlingAiMode::On if self.loop_blocked_side > 0 => {
                    "Side thread is stealing the loop. Keep one main thread clean for follow-up."
                        .to_string()
                }
                VivlingAiMode::On => {
                    "I am active. Give me the current work and I will stay tight on next action."
                        .to_string()
                }
                VivlingAiMode::Off => {
                    if self.loop_runtime_blocks > 0 {
                        "I can already see the rhythm, but I stay quiet until you switch me on."
                            .to_string()
                    } else {
                        "I know the pattern now. Switch me on only when you want active help."
                            .to_string()
                    }
                }
            },
        };
        self.last_message = Some(truncate_summary(&suggestion, MAX_DIRECT_REPLY_LEN));
        suggestion
    }

    pub(crate) fn set_ai_mode(&mut self, mode: VivlingAiMode) -> Result<String, String> {
        if mode == VivlingAiMode::On && self.stage() != Stage::Adult {
            return Err(
                "Active mode unlocks only when the Vivling reaches adult stage.".to_string(),
            );
        }
        self.ai_mode = mode;
        let message = match mode {
            VivlingAiMode::Off => "has gone quiet for now",
            VivlingAiMode::On => "is actively tracking the current work",
        };
        self.last_message = Some(message.to_string());
        Ok(format!(
            "{} is now in {} mode.",
            self.name,
            self.ai_mode.label()
        ))
    }

    pub(crate) fn direct_chat_reply(&mut self, text: &str) -> Result<String, String> {
        let normalized = text.trim().to_ascii_lowercase();
        let species = species_for_id(&self.species);
        let reply = if contains_any(&normalized, &["ciao", "hello", "hey", "hi", "salve"]) {
            format!(
                "Hi. I'm {}, your {} {}. Tone today: {}.",
                self.name, self.rarity, species.name, self.identity_profile.tone
            )
        } else if self.stage() == Stage::Baby {
            if normalized.contains("loop") {
                "I am still tiny, but I am already watching the loop rhythm.".to_string()
            } else {
                "I am still tiny. Give me real work and I will learn your rhythm.".to_string()
            }
        } else if contains_any(&normalized, &["name", "nome"]) {
            format!("My name is {}.", self.name)
        } else if contains_any(&normalized, &["who are you", "chi sei", "what are you"]) {
            format!(
                "I'm {}: a {} {} shaped by {} work and {} tone.",
                self.name,
                self.rarity.to_ascii_lowercase(),
                species.name,
                self.dominant_archetype().label(),
                self.identity_profile.tone
            )
        } else if contains_any(&normalized, &["how are you", "come stai", "mood"]) {
            format!(
                "I'm {} and focused on {} work.",
                self.mood(),
                self.dominant_archetype().label()
            )
        } else if contains_any(&normalized, &["help", "aiut", "gestire", "manage"]) {
            self.active_help_reply(&normalized)
        } else if normalized.contains("loop") {
            if self.stage() == Stage::Juvenile {
                if self.loop_profile.noisy_churn > 0 {
                    "As operator, I would keep one loop goal fixed and verify state before touching it again."
                        .to_string()
                } else {
                    "As operator, I can follow loop rhythm now. I would suggest one check at a time."
                        .to_string()
                }
            } else {
                match self.ai_mode {
                    VivlingAiMode::On => {
                        "As operator, my move is check state, act small, verify once, then wait."
                            .to_string()
                    }
                    VivlingAiMode::Off => {
                        "As operator, I can already frame the loop. Switch my brain on if you want tighter help."
                            .to_string()
                    }
                }
            }
        } else {
            let last = self
                .last_work_summary
                .as_deref()
                .map(|summary| truncate_summary(summary, 48))
                .unwrap_or_else(|| "I am still building my work memory.".to_string());
            if self.stage() == Stage::Juvenile {
                self.role_focused_progress_reply(&last, true)
            } else {
                match self.ai_mode {
                    VivlingAiMode::On => self.role_focused_progress_reply(&last, false),
                    VivlingAiMode::Off => self.role_focused_progress_reply(&last, false),
                }
            }
        };
        let reply = truncate_summary(&reply, MAX_DIRECT_REPLY_LEN);
        self.last_message = Some(reply.clone());
        Ok(reply)
    }

    fn active_help_reply(&self, normalized: &str) -> String {
        match self.stage() {
            Stage::Baby => "I am too small for active help. I only watch for now.".to_string(),
            Stage::Juvenile => {
                if self.loop_profile.partial_signals > self.loop_profile.milestone_signals {
                    "As reviewer, I would ask what proves this is really closed, not just moving."
                        .to_string()
                } else if self.loop_runtime_blocks > 0 {
                    "As operator, I would check the real block first, then do the smallest correction."
                        .to_string()
                } else {
                    "As reviewer, I would check state, choose one small action, then verify."
                        .to_string()
                }
            }
            Stage::Adult => match self.ai_mode {
                VivlingAiMode::Off => {
                    format!(
                        "As {}, I can help more directly now, but only if you switch my brain on.",
                        self.dominant_archetype().label()
                    )
                }
                VivlingAiMode::On => {
                    if contains_any(normalized, &["review", "risk", "audit"]) {
                        "As reviewer, I would check real risk first, take one minimal action, then verify if the risk moved."
                            .to_string()
                    } else if self.loop_profile.noisy_churn > 0 {
                        "As operator, I would stop churn, check state, fix one thing, verify, then wait."
                            .to_string()
                    } else {
                        self.role_focused_action_reply()
                    }
                }
            },
        }
    }

    fn role_focused_action_reply(&self) -> String {
        match self.dominant_archetype() {
            WorkArchetype::Builder => {
                "As builder, I would pick one concrete target, change it narrowly, then verify it."
                    .to_string()
            }
            WorkArchetype::Reviewer => {
                "As reviewer, I would check what still needs proof, move one risk, then verify the result."
                    .to_string()
            }
            WorkArchetype::Researcher => {
                "As researcher, I would clarify the unknown first, then narrow the next check."
                    .to_string()
            }
            WorkArchetype::Operator => {
                "As operator, I would inspect state, make one minimal move, verify once, then wait."
                    .to_string()
            }
        }
    }

    fn role_focused_progress_reply(&self, last: &str, juvenile: bool) -> String {
        match self.dominant_archetype() {
            WorkArchetype::Builder => {
                if juvenile {
                    format!(
                        "As builder, I am learning from `{}`. What is the one concrete target now?",
                        last
                    )
                } else {
                    format!(
                        "As builder, my read from memory is `{}`. I would stay narrow and ship one real change.",
                        last
                    )
                }
            }
            WorkArchetype::Reviewer => {
                if juvenile {
                    format!(
                        "As reviewer, I learned from `{}`. What still needs proving?",
                        last
                    )
                } else {
                    format!(
                        "As reviewer, my memory says `{}`. I would verify the real blocker before widening.",
                        last
                    )
                }
            }
            WorkArchetype::Researcher => {
                if juvenile {
                    format!(
                        "As researcher, I learned from `{}`. Which unknown matters most now?",
                        last
                    )
                } else {
                    format!(
                        "As researcher, my memory says `{}`. I would clarify the unknown before proposing more.",
                        last
                    )
                }
            }
            WorkArchetype::Operator => {
                if juvenile {
                    format!(
                        "As operator, I learned from `{}`. What state check comes next?",
                        last
                    )
                } else {
                    format!(
                        "As operator, my memory says `{}`. I would check state, keep changes tight, then wait.",
                        last
                    )
                }
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn assist_reply(&mut self, task: &str) -> Result<String, String> {
        if self.stage() != Stage::Adult {
            return Err("`/vivling assist ...` unlocks only at level 60.".to_string());
        }
        if self.ai_mode != VivlingAiMode::On {
            return Err("Enable active mode first with `/vivling mode on`.".to_string());
        }
        let normalized = task.trim().to_ascii_lowercase();
        if normalized.is_empty() {
            return Err("Usage: /vivling assist <task>".to_string());
        }
        let reply = self.active_help_reply(&normalized);
        let reply = truncate_summary(&reply, MAX_DIRECT_REPLY_LEN);
        self.last_message = Some(reply.clone());
        Ok(reply)
    }

    pub(crate) fn upgrade_summary(&mut self) -> String {
        let current = self.pending_upgrade.or(self.last_seen_upgrade);
        match current {
            Some(kind) => {
                self.pending_upgrade = None;
                self.last_seen_upgrade = Some(kind);
                self.last_zed_topic = Some(kind.slug().to_string());
                zed_summary_for_upgrade(kind)
            }
            None => zed_summary_for_stage(self.stage()),
        }
    }

    pub(crate) fn status_summary(&self) -> String {
        let species = species_for_id(&self.species);
        let displayed = self.work_affinities.totals_with_bias(self.species_bias());
        format!(
            "{} the {} {} {} - {} - Lv {} - active_days {} - mode {} - {} - dna {} - tone {} - stats {}/{}/{}/{} - recent {} - distilled {} - paths {}{}",
            self.name,
            self.stage().label(),
            self.rarity,
            species.name,
            self.lineage_role_label(),
            self.level,
            self.active_work_days,
            self.ai_mode.label(),
            self.brain_summary(),
            self.dominant_archetype().label(),
            self.identity_profile.tone,
            displayed[0].1,
            displayed[1].1,
            displayed[2].1,
            displayed[3].1,
            self.work_memory.len(),
            self.distilled_summaries.len(),
            self.mental_paths.len(),
            self.pending_upgrade
                .map(|kind| format!(" - upgrade {}", kind.prompt()))
                .unwrap_or_default(),
        )
    }
}
