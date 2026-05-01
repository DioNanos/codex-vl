pub(crate) struct BouncingBall {
    x: f32,
    vx: f32,
    hits: u32,
}

impl BouncingBall {
    pub(crate) fn new_with_seed(seed: u32) -> Self {
        let x = (seed % 5) as f32 + 1.0;
        let vx = if seed % 2 == 0 { 0.8 } else { -0.6 };
        Self { x, vx, hits: 0 }
    }

    pub(crate) fn step(&mut self, dt_ms: u64, width: usize) {
        if width < 3 {
            return;
        }
        let dt_s = dt_ms as f32 / 1000.0;
        let max_x = (width - 1) as f32;
        self.x += self.vx * dt_s * 8.0;
        if self.x <= 0.0 {
            self.x = 0.0;
            self.vx = self.vx.abs();
            self.hits = self.hits.saturating_add(1);
        } else if self.x >= max_x {
            self.x = max_x;
            self.vx = -self.vx.abs();
            self.hits = self.hits.saturating_add(1);
        }
    }

    pub(crate) fn frame(&self, width: usize) -> String {
        if width < 3 {
            return String::new();
        }
        let pos = self.x.round() as usize;
        let pos = pos.min(width - 1);
        let mut buf = vec![b' '; width];
        buf[pos] = b'o';
        String::from_utf8(buf).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_bounces_on_left_border() {
        let mut ball = BouncingBall {
            x: 0.5,
            vx: -1.0,
            hits: 0,
        };
        ball.step(1000, 10);
        assert!(ball.vx > 0.0);
        assert!(ball.hits > 0);
    }

    #[test]
    fn step_bounces_on_right_border() {
        let mut ball = BouncingBall {
            x: 9.0,
            vx: 1.0,
            hits: 0,
        };
        ball.step(1000, 10);
        assert!(ball.vx < 0.0);
        assert!(ball.hits > 0);
    }

    #[test]
    fn frame_produces_exact_width() {
        let ball = BouncingBall::new_with_seed(42);
        let f = ball.frame(10);
        assert_eq!(f.len(), 10);
        assert!(f.contains('o'));
    }

    #[test]
    fn new_with_seed_deterministic() {
        let a = BouncingBall::new_with_seed(7);
        let b = BouncingBall::new_with_seed(7);
        assert_eq!(a.frame(10), b.frame(10));
    }

    #[test]
    fn narrow_width_returns_empty() {
        let ball = BouncingBall::new_with_seed(1);
        assert!(ball.frame(2).is_empty());
    }
}
