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
        match activity {
            VivlingActivity::Idle
            | VivlingActivity::Eating
            | VivlingActivity::Sleeping
            | VivlingActivity::Playing
            | VivlingActivity::Working => "",
        }
    }
}
