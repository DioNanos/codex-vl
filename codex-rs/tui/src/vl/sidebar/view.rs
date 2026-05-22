use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Block;
use ratatui::widgets::Borders;
use ratatui::widgets::Widget;

use super::log::VivlingChatLog;
use super::log::VivlingLogKind;
use crate::render::renderable::Renderable;

/// Memory V2 Step 12.B.P — default panel height. Bumped 20 → 25 to
/// give long Vivling replies more breathing room. Override at
/// runtime via the `CODEX_VL_VIVLING_PANEL_HEIGHT` env var (clamped
/// 10–50) so users on tall terminals (or DAG on a 50-row console)
/// can stretch the panel without a code change.
const DEFAULT_EXPANDED_HEIGHT: u16 = 25;
const MIN_EXPANDED_HEIGHT: u16 = 10;
const MAX_EXPANDED_HEIGHT: u16 = 50;

fn expanded_height_from_env() -> u16 {
    match std::env::var("CODEX_VL_VIVLING_PANEL_HEIGHT")
        .ok()
        .and_then(|s| s.trim().parse::<u16>().ok())
    {
        Some(n) => n.clamp(MIN_EXPANDED_HEIGHT, MAX_EXPANDED_HEIGHT),
        None => DEFAULT_EXPANDED_HEIGHT,
    }
}

/// Memory V2 Step 12.B.P — terminal-first responsive layout tier.
/// The render path picks one based on the available width so the
/// panel stays usable on narrow Termux portrait sessions (~30 col)
/// as well as wide desktop terminals.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LayoutTier {
    /// `< 30` col — drop borders, drop sender prefix, drop age.
    Minimal,
    /// `30..50` col — borders + sender prefix, no age.
    Compact,
    /// `50..80` col — borders + sender prefix + age suffix.
    Normal,
    /// `>= 80` col — full layout including the `[N/M]` scroll
    /// position in the title.
    Full,
}

impl LayoutTier {
    fn from_width(width: u16) -> Self {
        match width {
            0..=29 => LayoutTier::Minimal,
            30..=49 => LayoutTier::Compact,
            50..=79 => LayoutTier::Normal,
            _ => LayoutTier::Full,
        }
    }

    fn show_borders(self) -> bool {
        !matches!(self, LayoutTier::Minimal)
    }

    fn show_sender_prefix(self) -> bool {
        !matches!(self, LayoutTier::Minimal)
    }

    fn show_age(self) -> bool {
        matches!(self, LayoutTier::Normal | LayoutTier::Full)
    }

    fn show_scroll_position(self) -> bool {
        matches!(self, LayoutTier::Full)
    }
}

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
                let tier = LayoutTier::from_width(area.width);
                let unread = self.log.unread_count();
                let last_age = self
                    .log
                    .last_entry()
                    .map(|e| e.ts.elapsed().as_secs().min(9999));
                let last_vivling_id = self
                    .log
                    .last_entry()
                    .and_then(|e| e.vivling_id.as_deref().map(|s| truncate_str(s, 16)));

                // Step 12.B.P — Block borders + title with responsive
                // header. On `Minimal` tier we skip the block entirely
                // so we keep every row for chat content.
                let (inner_area, has_borders) = if tier.show_borders() {
                    let title = format_header(
                        tier,
                        self.log.len(),
                        unread,
                        last_age,
                        last_vivling_id.as_deref(),
                    );
                    let block = Block::default()
                        .borders(Borders::ALL)
                        .title(Line::from(title.dim()));
                    let inner = block.inner(area);
                    block.render(area, buf);
                    (inner, true)
                } else {
                    // Render a single-line header inline (no block).
                    let header = format_header(
                        tier,
                        self.log.len(),
                        unread,
                        last_age,
                        last_vivling_id.as_deref(),
                    );
                    render_row(area, buf, 0, Line::from(header.dim()));
                    (
                        Rect {
                            x: area.x,
                            y: area.y + 1,
                            width: area.width,
                            height: area.height.saturating_sub(1),
                        },
                        false,
                    )
                };

                if self.log.is_empty() {
                    if inner_area.height > 0 {
                        render_row(inner_area, buf, 0, Line::from("> ready".dim()));
                    }
                    return;
                }
                let visible_rows = inner_area.height as usize;
                if visible_rows == 0 {
                    return;
                }
                let inner_width = (inner_area.width as usize).saturating_sub(2);
                let mut all_lines: Vec<Line<'static>> = Vec::new();
                for entry in self.log.iter_recent(self.log.len()) {
                    let entry_lines = wrap_entry(entry, inner_width, tier);
                    all_lines.extend(entry_lines);
                    // Separator between entries on Normal/Full tier.
                    if matches!(tier, LayoutTier::Normal | LayoutTier::Full) {
                        all_lines.push(Line::from(""));
                    }
                }
                // Drop trailing empty line so the last entry sits
                // flush at the bottom.
                if all_lines
                    .last()
                    .map(|l| l.spans.is_empty())
                    .unwrap_or(false)
                {
                    all_lines.pop();
                }
                let total_lines = all_lines.len();
                let max_offset = total_lines.saturating_sub(visible_rows);
                let scroll = (*scroll_offset).min(max_offset);
                let end = total_lines.saturating_sub(scroll);
                let start = end.saturating_sub(visible_rows);
                for (row, line) in all_lines[start..end].iter().enumerate() {
                    let row = row as u16;
                    if row >= inner_area.height {
                        break;
                    }
                    render_row(inner_area, buf, row, line.clone());
                }

                // Step 12.B.P — Full tier shows scroll position in
                // the bottom-right corner of the block. Hidden on
                // narrower tiers to preserve content width.
                if tier.show_scroll_position() && has_borders && total_lines > visible_rows {
                    let position = format!("[{}/{}]", start + 1, total_lines);
                    let label_width = position.chars().count() as u16;
                    if label_width + 4 < area.width && area.height >= 2 {
                        let pos_rect = Rect {
                            x: area.x + area.width.saturating_sub(label_width + 2),
                            y: area.y + area.height - 1,
                            width: label_width,
                            height: 1,
                        };
                        Line::from(position.dim()).render(pos_rect, buf);
                    }
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
            SidebarMode::Expanded { .. } => expanded_height_from_env(),
        }
    }
}

/// Step 12.B.P — single source for the responsive header line. The
/// `Minimal` tier already returns an inline string (no borders), the
/// others feed this into the `Block::title`.
fn format_header(
    tier: LayoutTier,
    msg_count: usize,
    unread: usize,
    last_age: Option<u64>,
    last_vivling_id: Option<&str>,
) -> String {
    let unread_suffix = if unread > 0 {
        format!(" · {unread} new")
    } else {
        String::new()
    };
    let age_part = if tier.show_age() {
        match last_age {
            Some(age) => format!(" · last {age}s ago"),
            None => " · ready".to_string(),
        }
    } else {
        String::new()
    };
    let id_part = match (tier, last_vivling_id) {
        (LayoutTier::Full, Some(id)) => format!(" · {id}"),
        _ => String::new(),
    };
    format!(" Vivling chat · {msg_count} msg{unread_suffix}{age_part}{id_part} ")
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
fn wrap_entry(
    entry: &super::log::VivlingLogEntry,
    inner_width: usize,
    tier: LayoutTier,
) -> Vec<Line<'static>> {
    // Step 12.B.P — sender prefix: ● Vivling chat / assist reply,
    // ■ system, ▸ user (kept around in case a future `User` variant
    // is added; today push() rejects `Life`). The `Minimal` tier
    // drops the prefix entirely to recover one column of body width.
    let prefix: &str = if tier.show_sender_prefix() {
        match entry.kind {
            VivlingLogKind::Chat => "●",
            VivlingLogKind::Assist => "●",
            VivlingLogKind::Life => "■",
        }
    } else {
        ""
    };
    let head_width = if prefix.is_empty() { 0 } else { 2 }; // "● "
    let body_width = inner_width.saturating_sub(head_width).max(1);
    let wrapped = wrap_text_lines(&entry.text, body_width);
    let mut out: Vec<Line<'static>> = Vec::with_capacity(wrapped.len().max(1));
    for (idx, chunk) in wrapped.iter().enumerate() {
        let line = if prefix.is_empty() {
            chunk.clone()
        } else if idx == 0 {
            format!("{prefix} {chunk}")
        } else {
            format!("  {chunk}")
        };
        out.push(Line::from(line.dim()));
    }
    if out.is_empty() {
        out.push(Line::from(prefix.to_string().dim()));
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
    fn expanded_empty_sidebar_renders_default_height() {
        let mut sidebar = VivlingSidebar::new();
        sidebar.toggle();
        assert!(sidebar.should_render());
        // Step 12.B.P — default panel height bumped 20 → 25.
        // SAFETY: clear env first so the default kicks in regardless
        // of test runner state.
        unsafe { std::env::remove_var("CODEX_VL_VIVLING_PANEL_HEIGHT") };
        assert_eq!(sidebar.desired_height(80), DEFAULT_EXPANDED_HEIGHT);
    }

    #[test]
    fn expanded_sidebar_uses_default_expanded_height() {
        let mut sidebar = VivlingSidebar::new();
        for i in 0..40 {
            sidebar.push(VivlingLogKind::Chat, format!("msg-{i}"), None);
        }
        sidebar.toggle();
        // Step 12.B.P — default panel height bumped 20 → 25.
        // SAFETY: clear env first so the default kicks in regardless
        // of test runner state.
        unsafe { std::env::remove_var("CODEX_VL_VIVLING_PANEL_HEIGHT") };
        assert_eq!(sidebar.desired_height(80), DEFAULT_EXPANDED_HEIGHT);
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
        let lines = wrap_entry(&entry, 40, LayoutTier::Normal);
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
        let lines = wrap_entry(&entry, 40, LayoutTier::Normal);
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
        let lines = wrap_entry(&entry, 20, LayoutTier::Normal);
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

    // ---- Memory V2 Step 12.B.P responsive layout tiers ------------

    #[test]
    fn layout_tier_from_width_assigns_correct_buckets() {
        assert_eq!(LayoutTier::from_width(20), LayoutTier::Minimal);
        assert_eq!(LayoutTier::from_width(29), LayoutTier::Minimal);
        assert_eq!(LayoutTier::from_width(30), LayoutTier::Compact);
        assert_eq!(LayoutTier::from_width(49), LayoutTier::Compact);
        assert_eq!(LayoutTier::from_width(50), LayoutTier::Normal);
        assert_eq!(LayoutTier::from_width(79), LayoutTier::Normal);
        assert_eq!(LayoutTier::from_width(80), LayoutTier::Full);
        assert_eq!(LayoutTier::from_width(200), LayoutTier::Full);
    }

    #[test]
    fn minimal_tier_drops_borders_and_sender_prefix() {
        // Termux portrait ~25 cols: every column must go to content.
        use crate::vl::sidebar::log::VivlingLogEntry;
        use std::time::Instant;
        let entry = VivlingLogEntry {
            kind: VivlingLogKind::Chat,
            text: "ciao".to_string(),
            ts: Instant::now(),
            vivling_id: None,
        };
        let lines = wrap_entry(&entry, 20, LayoutTier::Minimal);
        assert_eq!(lines.len(), 1);
        let body: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        // No sender prefix, no leading space — pure content.
        assert!(
            !body.starts_with('●') && !body.starts_with("● "),
            "minimal tier must not emit sender prefix: {body:?}"
        );
        assert!(body.contains("ciao"));
    }

    #[test]
    fn compact_tier_keeps_sender_prefix() {
        use crate::vl::sidebar::log::VivlingLogEntry;
        use std::time::Instant;
        let entry = VivlingLogEntry {
            kind: VivlingLogKind::Chat,
            text: "ok".to_string(),
            ts: Instant::now(),
            vivling_id: None,
        };
        let lines = wrap_entry(&entry, 30, LayoutTier::Compact);
        let body: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(
            body.starts_with('●'),
            "compact tier must emit sender prefix: {body:?}"
        );
    }

    #[test]
    fn format_header_full_tier_includes_vivling_id_and_age() {
        let h = format_header(LayoutTier::Full, 9, 0, Some(42), Some("viv-13ba0093"));
        assert!(h.contains("9 msg"), "{h}");
        assert!(h.contains("42s ago"), "{h}");
        assert!(h.contains("viv-13ba0093"), "{h}");
    }

    #[test]
    fn format_header_compact_tier_omits_age_and_id() {
        let h = format_header(LayoutTier::Compact, 9, 0, Some(42), Some("viv-13ba0093"));
        assert!(h.contains("9 msg"), "{h}");
        assert!(!h.contains("42s ago"), "compact omits age: {h}");
        assert!(!h.contains("viv-13ba0093"), "compact omits id: {h}");
    }

    #[test]
    fn expanded_height_env_override_clamps_to_range() {
        // SAFETY: tests in this module are not parallel-sensitive
        // because the env is read on every render, but we should
        // restore. Use a unique value within range and check upper
        // clamp via a deliberately out-of-range string.
        unsafe { std::env::set_var("CODEX_VL_VIVLING_PANEL_HEIGHT", "9999") };
        assert_eq!(expanded_height_from_env(), MAX_EXPANDED_HEIGHT);
        unsafe { std::env::set_var("CODEX_VL_VIVLING_PANEL_HEIGHT", "1") };
        assert_eq!(expanded_height_from_env(), MIN_EXPANDED_HEIGHT);
        unsafe { std::env::set_var("CODEX_VL_VIVLING_PANEL_HEIGHT", "30") };
        assert_eq!(expanded_height_from_env(), 30);
        unsafe { std::env::remove_var("CODEX_VL_VIVLING_PANEL_HEIGHT") };
        assert_eq!(expanded_height_from_env(), DEFAULT_EXPANDED_HEIGHT);
    }
}
