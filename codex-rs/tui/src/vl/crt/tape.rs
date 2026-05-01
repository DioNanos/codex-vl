#![allow(dead_code)]

use super::glyphs::code;
use super::glyphs::fixed_sprite;
use super::glyphs::playfield;
use super::glyphs::role_code;
use super::scene::CrtScene;

const PLAYFIELD_WIDTH: usize = 18;

pub(crate) struct CrtTape {
    cells: Vec<char>,
}

impl CrtTape {
    pub(crate) fn from_segments(segments: &[String]) -> Self {
        let content = segments
            .iter()
            .filter(|segment| !segment.is_empty())
            .map(|segment| sanitize(segment))
            .collect::<Vec<_>>()
            .join("  |  ");
        Self::new(content)
    }

    pub(crate) fn new(content: String) -> Self {
        let mut cells: Vec<char> = content.chars().collect();
        if cells.is_empty() {
            cells.push(' ');
        }
        Self { cells }
    }

    pub(crate) fn viewport(
        &self,
        width: usize,
        elapsed_ms: u64,
        speed_ms: u64,
        seed: u32,
    ) -> String {
        if width == 0 {
            return String::new();
        }
        let speed_ms = speed_ms.max(1);
        let offset = ((elapsed_ms / speed_ms) as usize + seed as usize) % self.cells.len();
        (0..width)
            .map(|idx| self.cells[(offset + idx) % self.cells.len()])
            .collect()
    }
}

pub(crate) fn scene_tape(scene: &CrtScene<'_>) -> CrtTape {
    CrtTape::from_segments(&[
        fixed_sprite(scene.sprite),
        playfield(scene.seed, scene.elapsed_ms, PLAYFIELD_WIDTH),
        format!("MOOD {}", code(scene.mood, 8)),
        format!("EN{:02}", scene.energy.clamp(0, 99)),
        format!("HU{:02}", scene.hunger.clamp(0, 99)),
        format!("LOOP{:02}", scene.loop_count.min(99)),
        format!("{}{:02}", role_code(scene.role), scene.level.min(99)),
    ])
}

fn sanitize(raw: &str) -> String {
    raw.chars()
        .map(|ch| {
            if ch.is_ascii_graphic() || ch == ' ' {
                ch
            } else {
                '?'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viewport_is_exact_width() {
        let tape = CrtTape::new("abc".to_string());
        assert_eq!(tape.viewport(8, 0, 120, 0).chars().count(), 8);
    }

    #[test]
    fn viewport_scrolls_with_time() {
        let tape = CrtTape::new("abcdef".to_string());
        assert_ne!(tape.viewport(4, 0, 100, 0), tape.viewport(4, 100, 100, 0));
    }

    #[test]
    fn viewport_wraps_short_content() {
        let tape = CrtTape::new("ab".to_string());
        assert_eq!(tape.viewport(5, 0, 100, 0), "ababa");
    }
}
