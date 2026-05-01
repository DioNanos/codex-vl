use super::activity::VivlingActivity;

pub(crate) struct VivlingAnimation {
    frame_index: usize,
}

impl VivlingAnimation {
    pub(crate) fn new() -> Self {
        Self { frame_index: 0 }
    }

    pub(crate) fn advance(&mut self) {
        self.frame_index = self.frame_index.wrapping_add(1);
    }

    pub(crate) fn current_text(&self, activity: VivlingActivity) -> &'static str {
        let idx = self.frame_index;
        match activity {
            VivlingActivity::Eating => {
                const EATING: &[&str] = &["*munch*", "*crunch*", "*nibble*", "*nom*"];
                EATING[idx % EATING.len()]
            }
            VivlingActivity::Sleeping => {
                const SLEEPING: &[&str] = &["zZz", "z..", ".z.", "z z"];
                SLEEPING[idx % SLEEPING.len()]
            }
            VivlingActivity::Playing => {
                const PLAYING: &[&str] = &["*hop*", "*bounce*", "*boing*", "*peek*"];
                PLAYING[idx % PLAYING.len()]
            }
            VivlingActivity::Working => {
                const WORKING: &[&str] = &["[>  ]", "[>> ]", "[>>>]", "[ >>]"];
                WORKING[idx % WORKING.len()]
            }
            VivlingActivity::Idle => "",
        }
    }
}
