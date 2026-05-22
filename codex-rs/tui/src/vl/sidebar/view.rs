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
            // Memory V2 Step 12.B.N — scroll is now measured in
            // wrapped lines, not in entries, so a single long
            // message that wraps to 10+ lines remains reachable.
            // The actual upper bound depends on render width; we
            // clamp to a generous ceiling here and let the render
            // path re-clamp against the real `total_lines -
            // visible_rows` once the area is known.
            let ceiling = (self.log.len().saturating_mul(20)) as i32;
            let new = *scroll_offset as i32 + delta;
            *scroll_offset = new.clamp(0, ceiling) as usize;
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
                let visible_rows = area.height.saturating_sub(1) as usize;
                if visible_rows == 0 {
                    return;
                }
                // Memory V2 Step 12.B.N — wrap entries to fit the
                // pane width instead of truncating with "…". Build
                // all wrapped lines for the recent N entries (we
                // walk the whole log so scroll_offset can land on
                // any earlier message), then take a window of
                // `visible_rows` ending at
                // `total_wrapped_lines - scroll_offset_in_lines`.
                let inner_width = (area.width as usize).saturating_sub(2);
                let mut all_lines: Vec<Line<'static>> = Vec::new();
                for entry in self.log.iter_recent(self.log.len()) {
                    let entry_lines = wrap_entry(entry, inner_width);
                    all_lines.extend(entry_lines);
                }
                // Per-line scroll. Each `entry` may produce 1..N
                // wrapped lines; one notch of `scroll_offset` moves
                // the window by one wrapped line, not one entry —
                // this is the only way long messages remain
                // reachable when the view shows ~20 lines and a
                // single message spans more than that.
                let total_lines = all_lines.len();
                let max_offset = total_lines.saturating_sub(visible_rows);
                let scroll = (*scroll_offset).min(max_offset);
                let end = total_lines.saturating_sub(scroll);
                let start = end.saturating_sub(visible_rows);
                for (row, line) in all_lines[start..end].iter().enumerate() {
                    let row = row as u16 + 1;
                    if row >= area.height {
                        break;
                    }
                    render_row(area, buf, row, line.clone());
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

/// Memory V2 Step 12.B.N — render a single chat entry as wrapped
/// lines so long messages stay readable inside the Ctrl+J pane.
/// The first line carries the kind prefix; continuation lines are
/// padded with two spaces so the column under the prefix stays
/// blank, matching the visual rhythm of multi-line entries in the
/// main chat history.
fn wrap_entry(entry: &super::log::VivlingLogEntry, inner_width: usize) -> Vec<Line<'static>> {
    let prefix = match entry.kind {
        VivlingLogKind::Chat => " ",
        VivlingLogKind::Assist => "*",
        VivlingLogKind::Life => "^",
    };
    let body_width = inner_width.saturating_sub(2).max(1);
    let wrapped = wrap_text_lines(&entry.text, body_width);
    let mut out: Vec<Line<'static>> = Vec::with_capacity(wrapped.len().max(1));
    for (idx, chunk) in wrapped.iter().enumerate() {
        let head = if idx == 0 { prefix } else { " " };
        out.push(Line::from(format!("{head} {chunk}").dim()));
    }
    if out.is_empty() {
        // Defensive: an empty trimmed body still gets a single
        // prefix-only line so the entry occupies one visible row.
        out.push(Line::from(format!("{prefix} ").dim()));
    }
    out
}

/// Simple greedy word-wrap. Walks Unicode words, restarts on
/// whitespace, breaks any token that exceeds `width` characters
/// (URLs, long identifiers) at the character boundary so nothing
/// silently overflows the pane.
fn wrap_text_lines(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }
    let mut out: Vec<String> = Vec::new();
    for raw_line in text.split('\n') {
        let trimmed = raw_line.trim_end_matches('\r');
        if trimmed.is_empty() {
            out.push(String::new());
            continue;
        }
        let mut current = String::new();
        for word in trimmed.split_whitespace() {
            if word.chars().count() > width {
                // Token longer than the pane: flush current and
                // hard-break the token into width-sized slices.
                if !current.is_empty() {
                    out.push(std::mem::take(&mut current));
                }
                let mut slice = String::new();
                for ch in word.chars() {
                    slice.push(ch);
                    if slice.chars().count() == width {
                        out.push(std::mem::take(&mut slice));
                    }
                }
                if !slice.is_empty() {
                    current = slice;
                }
                continue;
            }
            let need = if current.is_empty() {
                word.chars().count()
            } else {
                current.chars().count() + 1 + word.chars().count()
            };
            if need > width {
                out.push(std::mem::take(&mut current));
                current.push_str(word);
            } else {
                if !current.is_empty() {
                    current.push(' ');
                }
                current.push_str(word);
            }
        }
        if !current.is_empty() {
            out.push(current);
        }
    }
    out
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
        assert_eq!(sidebar.log.unread_count(), 0);
        assert_eq!(sidebar.desired_height(80), 0);
    }

    // ---- Memory V2 Step 12.B.N — wrap + scroll on wrapped lines ----

    #[test]
    fn wrap_entry_breaks_long_message_across_multiple_lines() {
        // A message longer than the inner width must produce >1
        // wrapped lines instead of truncating with `…`. Smoke
        // 2026-05-22 DAG: long chat replies were being cut off in
        // the Ctrl+J pane.
        use crate::vl::sidebar::log::VivlingLogEntry;
        use std::time::Instant;
        let entry = VivlingLogEntry {
            kind: VivlingLogKind::Chat,
            text: "ciao davide... tu il creatore. io nilo. *annuisce piano* perché mi hai chiamato così? c'è un motivo dietro al nome?".to_string(),
            ts: Instant::now(),
            vivling_id: None,
        };
        let lines = wrap_entry(&entry, 40);
        assert!(
            lines.len() > 1,
            "long entry must wrap into more than one line, got {}",
            lines.len()
        );
        for line in &lines {
            let width = line
                .spans
                .iter()
                .map(|s| s.content.chars().count())
                .sum::<usize>();
            assert!(
                width <= 40,
                "wrapped line exceeds inner width: {width} > 40"
            );
        }
    }

    #[test]
    fn wrap_entry_short_message_renders_as_single_line() {
        use crate::vl::sidebar::log::VivlingLogEntry;
        use std::time::Instant;
        let entry = VivlingLogEntry {
            kind: VivlingLogKind::Chat,
            text: "hi".to_string(),
            ts: Instant::now(),
            vivling_id: None,
        };
        let lines = wrap_entry(&entry, 40);
        assert_eq!(lines.len(), 1, "short entry must stay on one line");
    }

    #[test]
    fn wrap_entry_hard_breaks_token_longer_than_width() {
        // Pathological case: a URL or identifier exceeds the
        // available inner width. Must hard-break inside the token
        // so nothing overflows the pane silently.
        use crate::vl::sidebar::log::VivlingLogEntry;
        use std::time::Instant;
        let huge = "supercalifragilisticexpialidocious".repeat(2);
        let entry = VivlingLogEntry {
            kind: VivlingLogKind::Chat,
            text: huge.clone(),
            ts: Instant::now(),
            vivling_id: None,
        };
        let lines = wrap_entry(&entry, 20);
        for line in &lines {
            let width = line
                .spans
                .iter()
                .map(|s| s.content.chars().count())
                .sum::<usize>();
            assert!(
                width <= 20,
                "hard-break must respect pane width: {width} > 20"
            );
        }
    }
}
