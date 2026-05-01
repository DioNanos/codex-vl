use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct VivlingPanelData {
    pub(crate) title: String,
    pub(crate) narrow_lines: Vec<String>,
    pub(crate) wide_lines: Vec<String>,
}

pub(crate) fn render_vivling_card(state: &mut VivlingState) -> VivlingPanelData {
    let species = species_for_id(&state.species);
    let displayed = state.work_affinities.totals_with_bias(state.species_bias());
    let memory = state
        .last_work_summary
        .as_deref()
        .map(|summary| truncate_summary(summary, MAX_CARD_REPLY_LEN))
        .unwrap_or_else(|| "No work memory yet.".to_string());

    let build_lines = |width_hint: usize| {
        let art = card_art_for_species(species, state.stage(), width_hint);
        let mut lines = Vec::new();
        lines.push(format!(
            "{} · {} {} {} · Lv {}",
            state.name,
            state.stage().label(),
            state.rarity,
            species.name,
            state.level
        ));
        lines.push(format!(
            "DNA {} · mood {} · mode {} · active days {}",
            state.dominant_archetype().label(),
            state.mood(),
            state.ai_mode.label(),
            state.active_work_days
        ));
        lines.push(format!(
            "Tone {} · recent {} · distilled {} · paths {}",
            state.identity_profile.tone,
            state.work_memory.len(),
            state.distilled_summaries.len(),
            state.mental_paths.len()
        ));
        if let Some(upgrade) = state.pending_upgrade {
            lines.push(format!("Upgrade ready: {}", upgrade.prompt()));
        }
        lines.push(String::new());
        lines.extend(art.lines);
        lines.push(String::new());
        lines.push(format!(
            "Stats  B:{}  R:{}  D:{}  O:{}",
            displayed[0].1, displayed[1].1, displayed[2].1, displayed[3].1
        ));
        lines.push(format!(
            "Loops {} · blocks {} · churn {} · turns {}",
            state.loop_exposure,
            state.loop_runtime_blocks,
            state.loop_profile.noisy_churn,
            state.turns_observed
        ));
        lines.push(format!("Last: {}", memory));
        lines
    };

    VivlingPanelData {
        title: format!("{} · Card", state.name),
        narrow_lines: build_lines(64),
        wide_lines: build_lines(120),
    }
}

pub(crate) fn render_upgrade_card(state: &mut VivlingState) -> VivlingPanelData {
    let pending_or_seen_topic = state
        .pending_upgrade
        .map(|upgrade| ZedTopic::from_slug(upgrade.slug()))
        .or_else(|| {
            state
                .last_seen_upgrade
                .map(|upgrade| ZedTopic::from_slug(upgrade.slug()))
        })
        .or_else(|| state.last_zed_topic.as_deref().map(ZedTopic::from_slug));
    let (topic, summary) = if let Some(topic) = pending_or_seen_topic {
        (topic, state.upgrade_summary())
    } else {
        let topic = if state.stage() == Stage::Juvenile
            && state.loop_runtime_submissions == 0
            && state.turns_observed >= 3
            && state.loop_admin_churn == 0
        {
            ZedTopic::LoopOnboarding
        } else if state.loop_runtime_blocks >= 2
            || (state.loop_admin_churn >= 3 && state.loop_runtime_submissions == 0)
        {
            ZedTopic::LoopRhythm
        } else if state.stage() == Stage::Adult
            && (state.loop_runtime_submissions > 0 || state.loop_exposure > 0)
        {
            ZedTopic::LoopAssistReady
        } else {
            ZedTopic::Growth
        };
        (topic, zed_summary_for_topic(topic))
    };
    let panel = zed_panel_data(topic, &summary);
    VivlingPanelData {
        title: panel.title,
        narrow_lines: panel.narrow_lines,
        wide_lines: panel.wide_lines,
    }
}
