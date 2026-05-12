use super::*;

use crate::vl::crt::BOOT_STRIP_HEIGHT;
use crate::vl::crt::sprites::boot::BOOT_SPRITE_WIDTH;

const VIVLING_STRIP_HEIGHT: u16 = 3;
const VIVLING_STRIP_MIN_WIDTH: u16 = 12;

impl Vivling {
    pub(crate) fn mark_recent_activity(&self, tail: Duration) {
        let now = Instant::now();
        if !self.is_active_at(now) {
            self.active_started_at.set(Some(now));
        }
        let deadline = now + tail;
        let current = self.active_until.get();
        if current.is_none_or(|existing| existing < deadline) {
            self.active_until.set(Some(deadline));
        }
        self.request_frame();
    }

    pub(crate) fn request_frame(&self) {
        if let Some(frame_requester) = &self.frame_requester {
            frame_requester.schedule_frame();
        }
    }

    pub(crate) fn is_active_at(&self, now: Instant) -> bool {
        self.task_running.get()
            || self
                .active_until
                .get()
                .is_some_and(|deadline| deadline > now)
    }

    pub(crate) fn current_sprite(&self, state: &VivlingState, now: Instant) -> String {
        let species = species_for_id(&state.species);
        if !self.animations_enabled {
            *self.next_scheduled_frame_at.borrow_mut() = None;
            return match state.stage() {
                Stage::Baby => species.ascii_baby.clone(),
                Stage::Juvenile => species.ascii_juvenile.clone(),
                Stage::Adult => species.ascii_adult.clone(),
            };
        }

        let frames = active_footer_sprites_for_species(species, state.stage());
        let started = self.active_started_at.get().unwrap_or_else(|| {
            self.active_started_at.set(Some(now));
            now
        });
        let elapsed = now.saturating_duration_since(started);
        let frame_idx =
            (((elapsed.as_millis() / ACTIVE_FOOTER_FRAME_INTERVAL.as_millis()) as usize) + 1)
                % frames.len();
        let next_deadline = now + ACTIVE_FOOTER_FRAME_INTERVAL;
        let should_schedule = self
            .next_scheduled_frame_at
            .borrow()
            .is_none_or(|deadline| deadline <= now);
        if should_schedule {
            if let Some(frame_requester) = &self.frame_requester {
                frame_requester.schedule_frame_in(ACTIVE_FOOTER_FRAME_INTERVAL);
            }
            *self.next_scheduled_frame_at.borrow_mut() = Some(next_deadline);
        }
        frames[frame_idx].clone()
    }

    fn crt_seed(&self, state: &VivlingState) -> u32 {
        state
            .vivling_id
            .bytes()
            .fold(state.level as u32, |acc, byte| {
                acc.wrapping_add(byte as u32)
            })
    }

    fn crt_elapsed_ms(&self, now: Instant) -> u64 {
        let started = self.active_started_at.get().unwrap_or(now);
        now.saturating_duration_since(started).as_millis() as u64
    }

    /// Skip the boot animation. Idempotent. Triggered by any keypress
    /// observed while boot is still in progress.
    pub(crate) fn skip_crt_boot(&self) {
        self.crt_animation_ledger.skip_boot();
        self.request_frame();
    }

    /// Whether the boot animation is currently active for the active
    /// Vivling. Used by callers (BottomPane, etc.) to decide whether a
    /// keypress should be treated as a skip.
    pub(crate) fn crt_boot_in_progress(&self) -> bool {
        self.crt_animation_ledger
            .boot_phase(Instant::now())
            .is_some()
    }

    /// Whether the boot animation needs the strip to be tall enough for
    /// the per-species sprite. False once boot has completed or is
    /// disabled by config.
    fn boot_needs_expanded_strip(&self, area_width: u16) -> bool {
        if !self.crt_config.boot_animation || !self.animations_enabled {
            return false;
        }
        if area_width < BOOT_SPRITE_WIDTH {
            return false;
        }
        if self.crt_animation_ledger.boot_skipped() {
            return false;
        }
        self.crt_animation_ledger
            .boot_phase(Instant::now())
            .is_some()
    }
}

impl Renderable for Vivling {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let Some(state) = self.visible_state() else {
            return;
        };
        if area.height == 0 || area.width < VIVLING_STRIP_MIN_WIDTH {
            return;
        }
        let now = Instant::now();

        // Decide whether to render the boot strip first. Boot starts on
        // the first render of an app run and is skippable.
        if self.crt_config.boot_animation
            && self.animations_enabled
            && area.width >= BOOT_SPRITE_WIDTH
        {
            self.crt_animation_ledger.ensure_boot_started(now);
        }
        if let Some(boot_phase) = self.crt_animation_ledger.boot_phase(now)
            && area.height >= BOOT_STRIP_HEIGHT
            && area.width >= BOOT_SPRITE_WIDTH
            && self.crt_config.boot_animation
            && self.animations_enabled
        {
            let palette = crate::vl::crt::palette::Palette::codex();
            let mut surface = crate::vl::crt::CrtSurface::new(
                area.width,
                BOOT_STRIP_HEIGHT,
                ratatui::style::Style::default(),
            );
            crate::vl::crt::render_boot_strip(&mut surface, &palette, &state.species, boot_phase);
            let render_h = area.height.min(BOOT_STRIP_HEIGHT);
            let render_area = Rect {
                x: area.x,
                y: area.y,
                width: area.width,
                height: render_h,
            };
            surface.render(render_area, buf);
            self.schedule_animation_wake(now);
            return;
        }

        let sprite = self.current_sprite(state, now);
        let live_context = self.live_context.borrow();
        let insight = super::crt_insight::compute_insight(state, live_context.as_ref());
        let animation_text = self.current_animation_text_at(now);
        let animation_phrase = animation_text
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let last_message = insight
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .or(animation_phrase);
        let activity = *self.activity.borrow();
        let mode_for_observe = derive_mode(state, activity);

        // Update the animation ledger before rendering so transition
        // phases reflect the new inputs.
        self.crt_animation_ledger
            .observe(now, mode_for_observe, last_message, insight.as_deref());
        let transitions = self.crt_animation_ledger.phases(now);

        let mut surface = crate::vl::crt::CrtSurface::new(
            area.width,
            VIVLING_STRIP_HEIGHT,
            ratatui::style::Style::default(),
        );
        let scene = crate::vl::crt::CrtScene {
            species_id: &state.species,
            stage: state.stage(),
            name: &state.name,
            level: state.level,
            role: state.dominant_archetype().label(),
            mood: state.mood(),
            energy: state.energy,
            hunger: state.hunger,
            loop_count: state.loop_exposure,
            sprite: &sprite,
            seed: self.crt_seed(state),
            elapsed_ms: self.crt_elapsed_ms(now),
            last_message,
            activity,
            tier: crate::vl::crt::CrtTier::detect(),
            crt_config: &self.crt_config,
            transitions,
        };
        crate::vl::crt::render_crt_scene(&mut surface, &scene);
        let strip_h = area.height.min(VIVLING_STRIP_HEIGHT);
        let render_area = Rect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: strip_h,
        };
        surface.render(render_area, buf);

        self.schedule_animation_wake(now);
    }

    fn desired_height(&self, width: u16) -> u16 {
        if self.visible_state().is_none() || width < VIVLING_STRIP_MIN_WIDTH {
            return 0;
        }
        if self.boot_needs_expanded_strip(width) {
            return BOOT_STRIP_HEIGHT;
        }
        VIVLING_STRIP_HEIGHT
    }
}

impl Vivling {
    fn schedule_animation_wake(&self, now: Instant) {
        let Some(frame_requester) = &self.frame_requester else {
            return;
        };
        let target = self.crt_frame_target.get();
        if !target.schedules_frames() {
            return;
        }
        let ledger_wake = self.crt_animation_ledger.next_wake(now);
        let tick = target.tick();
        let wake = match (ledger_wake, tick) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (Some(a), None) => Some(a),
            (None, Some(b)) => {
                // Idle but pacing is enabled: schedule a long lazy tick
                // so breathing/flicker can cycle without burning CPU.
                Some(b.max(Duration::from_millis(800)))
            }
            (None, None) => None,
        };
        if let Some(d) = wake {
            frame_requester.schedule_frame_in(d);
        }
    }
}

fn derive_mode(
    state: &VivlingState,
    activity: Option<crate::vl::VivlingActivity>,
) -> crate::vl::crt::director::CrtMode {
    use crate::vl::crt::director::CrtMode;
    match activity {
        Some(crate::vl::VivlingActivity::Eating) => return CrtMode::Hungry,
        Some(crate::vl::VivlingActivity::Sleeping) => return CrtMode::Tired,
        Some(crate::vl::VivlingActivity::Playing) => return CrtMode::Thinking,
        Some(crate::vl::VivlingActivity::Working) => return CrtMode::Working,
        Some(crate::vl::VivlingActivity::Idle) | None => {}
    }
    if state.energy <= 12 {
        return CrtMode::Alert;
    }
    if state.hunger >= 90 {
        return CrtMode::Hungry;
    }
    if state.energy <= 28 {
        return CrtMode::Tired;
    }
    if state.mood().eq_ignore_ascii_case("curious") {
        return CrtMode::Thinking;
    }
    CrtMode::Idle
}
