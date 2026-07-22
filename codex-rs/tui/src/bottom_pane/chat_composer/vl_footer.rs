//! codex-vl footer extension lane.
//!
//! Fork-owned passive context must never replace the configurable upstream
//! status line. This module reserves one extra row below the upstream footer
//! when VL context is present. The parent composer contains only small layout
//! and render hooks, keeping future upstream merges localized.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;

use super::ChatComposer;
use crate::bottom_pane::footer::render_footer_line;
use crate::line_truncation::truncate_line_with_ellipsis_if_overflow;
use crate::ui_consts::FOOTER_INDENT_COLS;

impl ChatComposer {
    pub(crate) fn set_loop_context_label(&mut self, label: Option<String>) -> bool {
        if self.footer.loop_context_label == label {
            return false;
        }
        self.footer.loop_context_label = label;
        true
    }

    /// codex-vl: read-only accessor used by Vivling live-context sync.
    pub(crate) fn active_agent_label(&self) -> Option<&str> {
        self.footer.active_agent_label.as_deref()
    }

    fn has_vl_footer_context(&self) -> bool {
        self.footer
            .loop_context_label
            .as_deref()
            .is_some_and(|label| !label.trim().is_empty())
    }

    /// Add a fork-owned row only when the upstream footer already has a row.
    /// A zero-height upstream footer remains authoritative (for example while
    /// shutdown UI intentionally suppresses footer output).
    pub(super) fn footer_height_with_vl_context(&self, base_height: u16) -> u16 {
        base_height.saturating_add(u16::from(base_height > 0 && self.has_vl_footer_context()))
    }

    /// Split the allocated footer into the upstream row(s) followed by the VL
    /// context row. If the terminal cannot provide both, preserve the upstream
    /// footer and omit VL context for that frame.
    pub(super) fn split_vl_footer_area(&self, area: Rect) -> (Rect, Option<Rect>) {
        if !self.has_vl_footer_context() || area.height < 2 {
            return (area, None);
        }

        let base_height = area.height.saturating_sub(1);
        let base_area = Rect {
            height: base_height,
            ..area
        };
        let vl_area = Rect {
            y: area.y.saturating_add(base_height),
            height: 1,
            ..area
        };
        (base_area, Some(vl_area))
    }

    pub(super) fn render_vl_footer_context(&self, area: Option<Rect>, buf: &mut Buffer) {
        let Some(area) = area else {
            return;
        };
        let Some(label) = self
            .footer
            .loop_context_label
            .as_deref()
            .filter(|label| !label.trim().is_empty())
        else {
            return;
        };
        let available_width = area.width.saturating_sub(FOOTER_INDENT_COLS as u16) as usize;
        let line = truncate_line_with_ellipsis_if_overflow(
            Line::from(label.to_string()).dim(),
            available_width,
        );
        render_footer_line(area, buf, line);
    }
}

#[cfg(test)]
mod tests {
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::text::Line;
    use tokio::sync::mpsc::unbounded_channel;

    use super::ChatComposer;
    use crate::app_event::AppEvent;
    use crate::app_event_sender::AppEventSender;
    use crate::render::renderable::Renderable;

    fn rendered_rows(buf: &Buffer, area: Rect) -> Vec<String> {
        (0..area.height)
            .map(|y| {
                (0..area.width)
                    .map(|x| buf[(x, y)].symbol().chars().next().unwrap_or(' '))
                    .collect()
            })
            .collect()
    }

    fn composer_with_status_line() -> ChatComposer {
        let (tx, _rx) = unbounded_channel::<AppEvent>();
        let sender = AppEventSender::new(tx);
        let mut composer = ChatComposer::new(
            /*has_input_focus*/ true,
            sender,
            /*enhanced_keys_supported*/ false,
            "Ask Codex to do anything".to_string(),
            /*disable_paste_burst*/ false,
        );
        composer.set_status_line_enabled(/*enabled*/ true);
        composer.set_status_line(Some(Line::from("model · cwd · Context 0% used")));
        composer
    }

    #[test]
    fn loop_context_renders_below_status_line_without_replacing_it() {
        let mut composer = composer_with_status_line();
        let base_height = composer.desired_height(57);
        assert!(composer.set_loop_context_label(Some(
            "loops: 1 · owner: main · next: codex-vl-manager-boundary".to_string(),
        )));
        assert_eq!(composer.desired_height(57), base_height + 1);

        let area = Rect::new(0, 0, 57, composer.desired_height(57));
        let mut buf = Buffer::empty(area);
        composer.render(area, &mut buf);
        let rows = rendered_rows(&buf, area);
        let status_row = rows
            .iter()
            .position(|row| row.contains("model · cwd"))
            .expect("status line should remain visible");
        let loop_row = rows
            .iter()
            .position(|row| row.contains("loops: 1"))
            .expect("loop context should be visible");

        assert_eq!(loop_row, status_row + 1);
    }

    #[test]
    fn constrained_footer_preserves_upstream_row_before_vl_context() {
        let mut composer = composer_with_status_line();
        composer.set_loop_context_label(Some("loops: 1".to_string()));
        let area = Rect::new(0, 0, 57, 1);

        let (base, vl) = composer.split_vl_footer_area(area);

        assert_eq!(base, area);
        assert_eq!(vl, None);
    }
}
