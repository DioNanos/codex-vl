use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VivlingActivity {
    Idle,
    Eating,
    Sleeping,
    Playing,
    Working,
}

pub(crate) struct ActivityTransition {
    pub(crate) new_activity: VivlingActivity,
}

pub(crate) fn compute_activity(
    current: VivlingActivity,
    energy: u8,
    idle_secs: u64,
    is_baby_or_juvenile: bool,
    sidebar_collapsed: bool,
    worker_turn_observed: bool,
    loop_tick_running: bool,
) -> Option<ActivityTransition> {
    // Working trumps all
    if loop_tick_running && current != VivlingActivity::Working {
        return Some(ActivityTransition {
            new_activity: VivlingActivity::Working,
        });
    }

    // Worker turn observed -> Eating for a burst
    if worker_turn_observed && current != VivlingActivity::Eating {
        return Some(ActivityTransition {
            new_activity: VivlingActivity::Eating,
        });
    }

    match current {
        VivlingActivity::Working => {
            if !loop_tick_running {
                Some(ActivityTransition {
                    new_activity: VivlingActivity::Idle,
                })
            } else {
                None
            }
        }
        VivlingActivity::Eating => {
            // Eating lasts 8 seconds (handled externally via last_state_change)
            None // transition handled by tick.rs based on elapsed time
        }
        VivlingActivity::Sleeping => {
            if energy >= 80 {
                Some(ActivityTransition {
                    new_activity: VivlingActivity::Idle,
                })
            } else {
                None
            }
        }
        VivlingActivity::Playing => None, // transition handled by tick.rs based on elapsed time
        VivlingActivity::Idle => {
            // Idle > 60s + low energy -> Sleeping
            let sleep_threshold = if is_baby_or_juvenile { 60 } else { 30 };
            if idle_secs > 60 && energy < sleep_threshold {
                return Some(ActivityTransition {
                    new_activity: VivlingActivity::Sleeping,
                });
            }
            // Idle > 90s + high energy + Baby/Juvenile + sidebar collapsed -> Playing
            if idle_secs > 90 && energy > 60 && is_baby_or_juvenile && sidebar_collapsed {
                return Some(ActivityTransition {
                    new_activity: VivlingActivity::Playing,
                });
            }
            None
        }
    }
}

pub(crate) const EATING_DURATION: Duration = Duration::from_secs(8);
pub(crate) const PLAYING_MIN_DURATION: Duration = Duration::from_secs(30);
pub(crate) const PLAYING_MAX_DURATION: Duration = Duration::from_secs(120);
pub(crate) const SLEEP_ENERGY_INTERVAL: Duration = Duration::from_secs(5);
pub(crate) const SLEEP_ENERGY_GAIN: u8 = 1;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_to_sleeping_with_low_energy() {
        let result = compute_activity(
            VivlingActivity::Idle,
            25, // energy < 30
            65, // idle > 60s
            false,
            false,
            false,
            false,
        );
        let t = result.expect("should transition");
        assert_eq!(t.new_activity, VivlingActivity::Sleeping);
    }

    #[test]
    fn baby_sleeps_at_higher_threshold() {
        let result = compute_activity(
            VivlingActivity::Idle,
            55, // energy < 60 for baby
            65,
            true, // baby
            false,
            false,
            false,
        );
        assert!(result.is_some());
        assert_eq!(result.unwrap().new_activity, VivlingActivity::Sleeping);
    }

    #[test]
    fn sleeping_to_idle_when_energy_restored() {
        let result = compute_activity(
            VivlingActivity::Sleeping,
            85, // energy >= 80
            0,
            false,
            false,
            false,
            false,
        );
        assert_eq!(
            result.expect("should wake").new_activity,
            VivlingActivity::Idle
        );
    }

    #[test]
    fn working_trumps_all() {
        let result = compute_activity(
            VivlingActivity::Sleeping,
            10,
            120,
            false,
            false,
            false,
            true, // loop tick running
        );
        assert_eq!(result.unwrap().new_activity, VivlingActivity::Working);
    }

    #[test]
    fn worker_turn_observed_triggers_eating() {
        let result = compute_activity(
            VivlingActivity::Idle,
            80,
            0,
            false,
            false,
            true, // worker turn
            false,
        );
        assert_eq!(result.unwrap().new_activity, VivlingActivity::Eating);
    }

    #[test]
    fn playing_only_for_baby_juvenile_with_sidebar_collapsed() {
        // Baby, sidebar collapsed -> can play
        let result = compute_activity(VivlingActivity::Idle, 70, 95, true, true, false, false);
        assert_eq!(result.unwrap().new_activity, VivlingActivity::Playing);

        // Adult -> no playing
        let result = compute_activity(
            VivlingActivity::Idle,
            70,
            95,
            false, // adult
            true,
            false,
            false,
        );
        assert!(result.is_none());

        // Baby, sidebar expanded -> no playing
        let result = compute_activity(
            VivlingActivity::Idle,
            70,
            95,
            true,
            false, // sidebar expanded
            false,
            false,
        );
        assert!(result.is_none());
    }

    #[test]
    fn no_transition_when_conditions_not_met() {
        let result = compute_activity(
            VivlingActivity::Idle,
            80, // high energy
            30, // short idle
            false,
            false,
            false,
            false,
        );
        assert!(result.is_none());
    }
}
