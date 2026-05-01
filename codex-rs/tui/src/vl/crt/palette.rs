use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Slot {
    Face,
    Signal,
    Dim,
    Alert,
    Transparent,
}

#[derive(Clone, Copy)]
pub(crate) struct Palette {
    pub(crate) face: Style,
    pub(crate) signal: Style,
    pub(crate) dim: Style,
    pub(crate) alert: Style,
}

impl Palette {
    pub(crate) fn codex() -> Self {
        Self {
            face: Style::default()
                .fg(Color::LightYellow)
                .add_modifier(Modifier::BOLD),
            signal: Style::default().fg(Color::Cyan),
            dim: Style::default().fg(Color::DarkGray),
            alert: Style::default()
                .fg(Color::LightYellow)
                .add_modifier(Modifier::BOLD)
                .add_modifier(Modifier::REVERSED),
        }
    }

    pub(crate) fn style_for(&self, slot: Slot) -> Option<Style> {
        match slot {
            Slot::Face => Some(self.face),
            Slot::Signal => Some(self.signal),
            Slot::Dim => Some(self.dim),
            Slot::Alert => Some(self.alert),
            Slot::Transparent => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transparent_slot_has_no_style() {
        let palette = Palette::codex();
        assert!(palette.style_for(Slot::Transparent).is_none());
    }

    #[test]
    fn opaque_slots_resolve_to_palette_styles() {
        let palette = Palette::codex();
        assert_eq!(palette.style_for(Slot::Face), Some(palette.face));
        assert_eq!(palette.style_for(Slot::Signal), Some(palette.signal));
        assert_eq!(palette.style_for(Slot::Dim), Some(palette.dim));
        assert_eq!(palette.style_for(Slot::Alert), Some(palette.alert));
    }
}
