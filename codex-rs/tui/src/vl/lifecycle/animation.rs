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

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression guard: animation text is scientific (eat/sleep/play/work
    /// activity glyphs) and must never be modulated by relational state.
    /// The `current_text` signature does not accept `LifecycleVoiceTone`, so
    /// this test mostly asserts the API surface stays narrow; if a future
    /// change adds a tone parameter here, the test will fail to compile and
    /// force a design conversation.
    #[test]
    fn animation_text_is_not_modulated_by_bond() {
        let mut anim = VivlingAnimation::new();
        anim.frame_index = 0;
        let eating = anim.current_text(VivlingActivity::Eating);
        let sleeping = anim.current_text(VivlingActivity::Sleeping);
        let playing = anim.current_text(VivlingActivity::Playing);
        let working = anim.current_text(VivlingActivity::Working);
        let idle = anim.current_text(VivlingActivity::Idle);

        // Verify these are the scientific pool entries, untouched by any tone.
        assert_eq!(eating, "*munch*");
        assert_eq!(sleeping, "zZz");
        assert_eq!(playing, "*hop*");
        assert_eq!(working, "[>  ]");
        assert_eq!(idle, "");
    }
}
