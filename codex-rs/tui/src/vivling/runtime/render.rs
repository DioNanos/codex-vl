use super::*;

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
        let sprite = self.current_sprite(state, now);
        let live_context = self.live_context.borrow();
        let insight = super::crt_insight::compute_insight(state, live_context.as_ref());
        let last_message = insight.as_deref().map(str::trim).filter(|s| !s.is_empty());
        let activity = *self.activity.borrow();
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
        };
        crate::vl::crt::render_crt_scene(&mut surface, &scene);
        surface.render(area, buf);
    }

    fn desired_height(&self, width: u16) -> u16 {
        if self.visible_state().is_some() && width >= VIVLING_STRIP_MIN_WIDTH {
            VIVLING_STRIP_HEIGHT
        } else {
            0
        }
    }
}
