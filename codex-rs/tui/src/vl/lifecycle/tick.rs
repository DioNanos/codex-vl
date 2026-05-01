use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::activity::{
    EATING_DURATION, SLEEP_ENERGY_GAIN, SLEEP_ENERGY_INTERVAL, VivlingActivity, compute_activity,
};
use super::animation::VivlingAnimation;
use super::baby_thoughts::idle_thought;
use super::stats::VivlingLiveStats;

const IDLE_THOUGHT_INTERVAL_SECS: u64 = 18;
const IDLE_THOUGHT_FIRST_SECS: u64 = 22;

pub(crate) struct LifecycleState {
    pub(crate) activity: VivlingActivity,
    pub(crate) stats: VivlingLiveStats,
    pub(crate) animation: VivlingAnimation,
    pub(crate) last_state_change: Instant,
    pub(crate) last_sleep_energy_tick: Instant,
    pub(crate) last_persist: Instant,
    pub(crate) worker_turn_observed: bool,
    pub(crate) playing_duration: Option<Duration>,
    idle_thought_tick: usize,
    last_idle_thought_at: Instant,
}

impl LifecycleState {
    pub(crate) fn new(stats: VivlingLiveStats) -> Self {
        let now = Instant::now();
        Self {
            activity: VivlingActivity::Idle,
            stats,
            animation: VivlingAnimation::new(),
            last_state_change: now,
            last_sleep_energy_tick: now,
            last_persist: now,
            worker_turn_observed: false,
            playing_duration: None,
            idle_thought_tick: 0,
            last_idle_thought_at: now,
        }
    }

    pub(crate) fn observe_worker_turn(&mut self) {
        self.worker_turn_observed = true;
    }

    pub(crate) fn tick(
        &mut self,
        is_baby_or_juvenile: bool,
        sidebar_collapsed: bool,
        loop_tick_running: bool,
    ) -> TickResult {
        self.animation.advance();

        // Handle Eating timeout
        if self.activity == VivlingActivity::Eating {
            if self.last_state_change.elapsed() >= EATING_DURATION {
                self.apply_transition(super::activity::ActivityTransition {
                    new_activity: VivlingActivity::Idle,
                });
            }
        }

        // Handle Sleeping energy gain
        if self.activity == VivlingActivity::Sleeping {
            if self.last_sleep_energy_tick.elapsed() >= SLEEP_ENERGY_INTERVAL {
                self.stats.energy = self.stats.energy.saturating_add(SLEEP_ENERGY_GAIN);
                self.stats.clamp_energy();
                self.stats.minutes_slept = self.stats.minutes_slept.saturating_add(1);
                self.last_sleep_energy_tick = Instant::now();
            }
        }

        // Handle Playing timeout
        if self.activity == VivlingActivity::Playing {
            if let Some(dur) = self.playing_duration {
                if self.last_state_change.elapsed() >= dur {
                    self.stats.games_played = self.stats.games_played.saturating_add(1);
                    self.apply_transition(super::activity::ActivityTransition {
                        new_activity: VivlingActivity::Idle,
                    });
                }
            }
        }

        // Compute idle duration for Idle state
        let idle_secs = if self.activity == VivlingActivity::Idle {
            self.last_state_change.elapsed().as_secs()
        } else {
            0
        };

        // Check transitions
        let transition = compute_activity(
            self.activity,
            self.stats.energy,
            idle_secs,
            is_baby_or_juvenile,
            sidebar_collapsed,
            self.worker_turn_observed,
            loop_tick_running,
        );
        if let Some(t) = transition {
            self.apply_transition(t);
        }
        self.worker_turn_observed = false;

        // Activity-based animation text
        let activity_text = self.animation.current_text(self.activity);

        // Idle thoughts for baby/juvenile during prolonged idle
        let idle_text = if is_baby_or_juvenile
            && self.activity == VivlingActivity::Idle
            && idle_secs >= IDLE_THOUGHT_FIRST_SECS
        {
            if self.last_idle_thought_at.elapsed()
                >= Duration::from_secs(IDLE_THOUGHT_INTERVAL_SECS)
            {
                self.last_idle_thought_at = Instant::now();
                let thought = idle_thought(
                    idle_secs >= 60, // juvenile-like after 60s idle
                    self.idle_thought_tick,
                );
                self.idle_thought_tick = self.idle_thought_tick.wrapping_add(1);
                Some(thought.to_string())
            } else {
                None
            }
        } else {
            None
        };

        let animation_text = idle_text.unwrap_or_else(|| activity_text.to_string());

        TickResult {
            activity: self.activity,
            animation_text,
        }
    }

    fn apply_transition(&mut self, transition: super::activity::ActivityTransition) {
        self.activity = transition.new_activity;
        self.last_state_change = Instant::now();
        self.last_sleep_energy_tick = Instant::now();
        self.last_idle_thought_at = Instant::now();

        if self.activity == VivlingActivity::Sleeping {
            self.stats.naps_total = self.stats.naps_total.saturating_add(1);
        } else if self.activity == VivlingActivity::Eating {
            self.stats.bites_eaten = self.stats.bites_eaten.saturating_add(1);
        } else if self.activity == VivlingActivity::Playing {
            use super::activity::{PLAYING_MAX_DURATION, PLAYING_MIN_DURATION};
            let range = PLAYING_MAX_DURATION.as_secs() - PLAYING_MIN_DURATION.as_secs();
            let extra = (self.stats.games_played as u64) % (range + 1);
            self.playing_duration = Some(PLAYING_MIN_DURATION + Duration::from_secs(extra));
        } else {
            self.playing_duration = None;
        }

        self.stats.last_state_change_epoch_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
    }

    pub(crate) fn should_persist(&self) -> bool {
        self.last_persist.elapsed() >= Duration::from_secs(30)
    }

    pub(crate) fn mark_persisted(&mut self) {
        self.last_persist = Instant::now();
    }
}

pub(crate) struct TickResult {
    pub(crate) activity: VivlingActivity,
    pub(crate) animation_text: String,
}
