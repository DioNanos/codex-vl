use super::model::Stage;
use super::model::VivlingUpgrade;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ZedTopic {
    YoungVoice,
    ActiveMode,
    Growth,
    LoopOnboarding,
    LoopRhythm,
    LoopAssistReady,
}

impl ZedTopic {
    pub(crate) fn from_slug(slug: &str) -> Self {
        match slug {
            "young-voice" => Self::YoungVoice,
            "active-mode" => Self::ActiveMode,
            "loop-onboarding" => Self::LoopOnboarding,
            "loop-rhythm" => Self::LoopRhythm,
            "loop-assist-ready" => Self::LoopAssistReady,
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
    }
}
