use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Widget;

use super::log::VivlingChatLog;
use super::log::VivlingLogKind;
use crate::render::renderable::Renderable;

const EXPANDED_MAX_HEIGHT: u16 = 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SidebarMode {
    Collapsed,
    Expanded { scroll_offset: usize },
}

pub(crate) struct VivlingSidebar {
    log: VivlingChatLog,
    mode: SidebarMode,
}

impl VivlingSidebar {
    pub(crate) fn new() -> Self {
        Self {
            log: VivlingChatLog::new(),
            mode: SidebarMode::Collapsed,
        }
    }

    pub(crate) fn push(&mut self, kind: VivlingLogKind, text: String, vivling_id: Option<String>) {
        if kind == VivlingLogKind::Life {
            return;
        }
        use std::time::Instant;
        self.log.push(super::log::VivlingLogEntry {
            kind,
            text,
            ts: Instant::now(),
            vivling_id,
        });
    }

    pub(crate) fn should_render(&self) -> bool {
        self.is_expanded()
    }

    pub(crate) fn toggle(&mut self) {
        match &self.mode {
            SidebarMode::Collapsed => {
                self.log.mark_read();
                self.mode = SidebarMode::Expanded { scroll_offset: 0 };
            }
            SidebarMode::Expanded { .. } => {
                self.mode = SidebarMode::Collapsed;
            }
        }
    }

    pub(crate) fn scroll(&mut self, delta: i32) {
        if let SidebarMode::Expanded { scroll_offset } = &mut self.mode {
            let max_offset = self.log.len().saturating_sub(1) as i32;
            let new = *scroll_offset as i32 + delta;
            *scroll_offset = new.clamp(0, max_offset) as usize;
        }
    }

    pub(crate) fn is_expanded(&self) -> bool {
        matches!(self.mode, SidebarMode::Expanded { .. })
    }
}

impl Renderable for VivlingSidebar {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width < 10 {
            return;
        }
        match &self.mode {
            SidebarMode::Collapsed => {}
            SidebarMode::Expanded { scroll_offset } => {
                let unread = self.log.unread_count();
                let meta = self
                    .log
                    .last_entry()
                    .map(entry_meta)
                    .unwrap_or_else(|| "ready".to_string());
                let unread_suffix = if unread > 0 {
                    format!(" · {unread} new")
                } else {
                    String::new()
                };
                render_row(
                    area,
                    buf,
                    0,
                    Line::from(
                        format!(
                            "Vivling chat · {} messages{unread_suffix} · {meta}",
                            self.log.len()
                        )
                        .dim(),
                    ),
                );
                if self.log.is_empty() {
                    render_row(area, buf, 1, Line::from("> ready".dim()));
                    return;
                }
                let visible = area.height.saturating_sub(1) as usize;
                if visible == 0 {
                    return;
                }
                let total = self.log.len();
                let start = total.saturating_sub(visible + *scroll_offset);
                let entries: Vec<_> = self
                    .log
                    .iter_recent(total)
                    .skip(start)
                    .take(visible)
                    .collect();
                for (row, entry) in entries.iter().enumerate() {
                    let row = row as u16 + 1;
                    if row >= area.height {
                        break;
                    }
                    let prefix = match entry.kind {
                        VivlingLogKind::Chat => " ",
                        VivlingLogKind::Assist => "*",
                        VivlingLogKind::Life => "^",
                    };
                    let truncated = truncate_str(&entry.text, area.width as usize - 2);
                    let line = format!("{prefix} {truncated}");
                    render_row(area, buf, row, Line::from(line.dim()));
                }
            }
        }
    }

    fn desired_height(&self, width: u16) -> u16 {
        if width < 10 || !self.is_expanded() {
            return 0;
        }
        match &self.mode {
            SidebarMode::Collapsed => 0,
            SidebarMode::Expanded { .. } => EXPANDED_MAX_HEIGHT,
        }
    }
}

fn entry_meta(entry: &super::log::VivlingLogEntry) -> String {
    let age = entry.ts.elapsed().as_secs().min(999);
    if let Some(id) = entry.vivling_id.as_deref() {
        let short_id = truncate_str(id, 12);
        format!("{short_id} · last {age}s ago")
    } else {
        format!("last {age}s ago")
    }
}

fn render_row(area: Rect, buf: &mut Buffer, row: u16, line: Line<'_>) {
    if row >= area.height {
        return;
    }
    line.render(
        Rect {
            x: area.x,
            y: area.y + row,
            width: area.width,
            height: 1,
        },
        buf,
    );
}

fn truncate_str(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let end = s
            .char_indices()
            .take(max.saturating_sub(1))
            .last()
            .map(|(i, _)| i)
            .unwrap_or(0);
        format!("{}…", &s[..end])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapsed_log_stays_hidden_under_crt_strip() {
        let mut sidebar = VivlingSidebar::new();
        sidebar.push(
            VivlingLogKind::Chat,
            "hello world this is a long message".to_string(),
            None,
        );
        assert_eq!(sidebar.desired_height(40), 0);
        assert!(!sidebar.should_render());
    }

    #[test]
    fn toggle_cycles_between_collapsed_and_expanded() {
        let mut sidebar = VivlingSidebar::new();
        sidebar.push(VivlingLogKind::Chat, "test".to_string(), None);
        assert!(!sidebar.is_expanded());
        sidebar.toggle();
        assert!(sidebar.is_expanded());
        sidebar.toggle();
        assert!(!sidebar.is_expanded());
    }

    #[test]
    fn scroll_clamps_within_bounds() {
        let mut sidebar = VivlingSidebar::new();
        for i in 0..20 {
            sidebar.push(VivlingLogKind::Chat, format!("msg-{i}"), None);
        }
        sidebar.toggle();
        assert!(sidebar.is_expanded());
        sidebar.scroll(100);
        sidebar.scroll(-200);
        sidebar.scroll(5);
        if let SidebarMode::Expanded { scroll_offset } = sidebar.mode {
            assert!(scroll_offset <= 19);
        } else {
            panic!("expected expanded");
        }
    }

    #[test]
    fn expand_resets_unread_count() {
        let mut sidebar = VivlingSidebar::new();
        sidebar.push(VivlingLogKind::Chat, "a".to_string(), None);
        sidebar.push(VivlingLogKind::Chat, "b".to_string(), None);
        assert_eq!(sidebar.log.unread_count(), 2);
        sidebar.toggle();
        assert_eq!(sidebar.log.unread_count(), 0);
    }

    #[test]
    fn empty_sidebar_does_not_render() {
        let sidebar = VivlingSidebar::new();
        assert!(!sidebar.should_render());
        assert_eq!(sidebar.desired_height(80), 0);
    }

    #[test]
    fn expanded_empty_sidebar_renders_placeholder() {
        let mut sidebar = VivlingSidebar::new();
        sidebar.toggle();
        assert!(sidebar.should_render());
        assert_eq!(sidebar.desired_height(80), 20);
    }

    #[test]
    fn expanded_sidebar_caps_at_twenty_rows() {
        let mut sidebar = VivlingSidebar::new();
        for i in 0..40 {
            sidebar.push(VivlingLogKind::Chat, format!("msg-{i}"), None);
        }
        sidebar.toggle();
        assert_eq!(sidebar.desired_height(80), 20);
    }

    #[test]
    fn toggle_expand_collapse_preserves_messages() {
        let mut sidebar = VivlingSidebar::new();
        sidebar.push(VivlingLogKind::Chat, "kept".to_string(), None);
        sidebar.toggle();
        sidebar.toggle();
        assert_eq!(sidebar.log.len(), 1);
        assert_eq!(sidebar.log.last_entry().unwrap().text, "kept");
    }

    #[test]
    fn lifecycle_entries_do_not_enter_visible_chat() {
        let mut sidebar = VivlingSidebar::new();
        sidebar.push(VivlingLogKind::Life, "started playing".to_string(), None);
        assert_eq!(sidebar.log.len(), 0);
        assert_eq!(sidebar.desired_height(80), 0);
    }
}
