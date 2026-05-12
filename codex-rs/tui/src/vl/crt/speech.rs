use ratatui::style::Modifier;

use super::palette::Palette;
use super::surface::CrtSurface;

pub(crate) const PANEL_MIN_WIDTH: u16 = 6;
const BUBBLE_PREFIX: &str = "< ";
const BUBBLE_MAX_CHARS: usize = 18;

/// Animation hints for bubble rendering. Lets the renderer reveal the
/// message progressively (typewriter) and add a brief glow during the
/// initial frames after the message changed.
#[derive(Clone, Copy, Debug)]
pub(crate) struct BubbleAnim {
    /// Maximum number of characters to reveal from the prepared bubble
    /// text. `usize::MAX` means "show everything" (no typewriter).
    pub reveal_chars: usize,
    /// When true, the bubble row gets a `BOLD` accent for the brief glow
    /// effect that follows an insight change.
    pub glow: bool,
}

impl BubbleAnim {
    pub(crate) fn settled() -> Self {
        Self {
            reveal_chars: usize::MAX,
            glow: false,
        }
    }
}

pub(crate) fn draw_bubble(
    surface: &mut CrtSurface,
    panel_x: u16,
    panel_w: u16,
    last_message: Option<&str>,
    palette: &Palette,
) -> Option<u16> {
    draw_bubble_animated(
        surface,
        panel_x,
        panel_w,
        last_message,
        palette,
        BubbleAnim::settled(),
    )
}

pub(crate) fn draw_bubble_animated(
    surface: &mut CrtSurface,
    panel_x: u16,
    panel_w: u16,
    last_message: Option<&str>,
    palette: &Palette,
    anim: BubbleAnim,
) -> Option<u16> {
    if panel_w < PANEL_MIN_WIDTH || surface.height() < 3 {
        return None;
    }
    let full_text = last_message.and_then(prepare_bubble)?;
    let revealed: String = full_text.chars().take(anim.reveal_chars).collect();
    if revealed.is_empty() {
        return None;
    }
    let mut style = palette.signal;
    if anim.glow {
        style = style.add_modifier(Modifier::BOLD);
    }
    // Reserve the full text width so neighbouring widgets don't reflow as
    // characters get revealed by the typewriter.
    let used = full_text.chars().count().min(panel_w as usize) as u16;
    if write_line(surface, panel_x, 1, panel_w, &revealed, style) {
        Some(used)
    } else {
        None
    }
}

pub(crate) fn bubble_width(last_message: Option<&str>, panel_w: u16) -> u16 {
    if panel_w < PANEL_MIN_WIDTH {
        return 0;
    }
    last_message
        .and_then(prepare_bubble)
        .map(|text| text.chars().count().min(panel_w as usize) as u16)
        .unwrap_or(0)
}

fn prepare(raw: &str) -> Option<String> {
    let cleaned: String = raw
        .chars()
        .map(|c| {
            if c.is_control() || c.is_whitespace() {
                ' '
            } else {
                c
            }
        })
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn prepare_bubble(raw: &str) -> Option<String> {
    let cleaned = prepare(raw)?;
    let lowered = cleaned.to_ascii_lowercase();
    let compact = if lowered.contains("watching completed turns") {
        "watching".to_string()
    } else if lowered.contains("tracking work rhythm") {
        "tracking".to_string()
    } else if lowered.contains("sees the pattern") {
        "pattern".to_string()
    } else if lowered.contains("grew") || lowered.contains("growing") {
        "growing".to_string()
    } else if lowered.contains("loop") && lowered.contains("active") {
        "loop alert".to_string()
    } else if lowered.contains("noticed loop") {
        "loop seen".to_string()
    } else if lowered.contains("cleanly") || lowered.contains("completed") {
        "done".to_string()
    } else {
        cleaned
    };
    let mut out = String::from(BUBBLE_PREFIX);
    let available = BUBBLE_MAX_CHARS.saturating_sub(BUBBLE_PREFIX.chars().count());
    let count = compact.chars().count();
    if count > available {
        let cut = available.saturating_sub(2);
        out.extend(compact.chars().take(cut));
        out.push_str("..");
    } else {
        out.push_str(&compact);
    }
    Some(out)
}

fn write_line(
    surface: &mut CrtSurface,
    x: u16,
    y: u16,
    max_w: u16,
    text: &str,
    style: ratatui::style::Style,
) -> bool {
    if max_w == 0 {
        return false;
    }
    let count = text.chars().count();
    let final_text = if count > max_w as usize {
        let cut = (max_w as usize).saturating_sub(2);
        let mut s: String = text.chars().take(cut).collect();
        s.push_str("..");
        s
    } else {
        text.to_string()
    };
    surface.put_clipped(x, y, max_w, &final_text, style);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::style::Style;

    fn render(panel_w: u16, msg: Option<&str>) -> (bool, String) {
        let palette = Palette::codex();
        let total = 4 + panel_w;
        let mut surface = CrtSurface::new(total, 3, Style::default());
        surface.fill(0, 0, total, ' ', palette.dim);
        surface.fill(0, 1, total, ' ', palette.dim);
        surface.fill(0, 2, total, ' ', palette.dim);
        let drew = draw_bubble(&mut surface, 4, panel_w, msg, &palette).is_some();
        let mut buf = Buffer::empty(Rect::new(0, 0, total, 3));
        surface.render(Rect::new(0, 0, total, 3), &mut buf);
        let rendered: String = buf.content.iter().map(|c| c.symbol()).collect();
        (drew, rendered)
    }

    #[test]
    fn draws_message_when_panel_wide_enough() {
        let (drew, rendered) = render(40, Some("greets the operator"));
        assert!(drew);
        assert!(rendered.contains("< greets the ope.."));
    }

    #[test]
    fn truncates_long_messages_with_ascii_marker() {
        let (drew, rendered) = render(10, Some("a very long sentence indeed"));
        assert!(drew);
        assert!(rendered.contains("< a very.."));
    }

    #[test]
    fn skips_panel_when_too_narrow() {
        let (drew, rendered) = render(4, Some("hi"));
        assert!(!drew);
        assert!(!rendered.contains("hi"));
    }

    #[test]
    fn empty_inputs_do_not_draw() {
        let (drew, _) = render(40, Some("   "));
        assert!(!drew);
    }

    #[test]
    fn preserves_accented_message_text() {
        let (drew, rendered) = render(40, Some("è pronto"));
        assert!(drew);
        assert!(rendered.contains("< è pronto"));
    }

    #[test]
    fn bubble_compacts_verbose_lifecycle_message() {
        let palette = Palette::codex();
        let mut surface = CrtSurface::new(40, 3, Style::default());
        let used = draw_bubble(
            &mut surface,
            0,
            40,
            Some("watching completed turns closely"),
            &palette,
        )
        .expect("bubble should draw");
        assert_eq!(used, 10);
        let mut buf = Buffer::empty(Rect::new(0, 0, 40, 3));
        surface.render(Rect::new(0, 0, 40, 3), &mut buf);
        let rendered: String = buf.content.iter().map(|c| c.symbol()).collect();
        assert!(rendered.contains("< watching"));
        assert!(!rendered.contains("completed turns"));
    }

    #[test]
    fn typewriter_reveals_partial_message() {
        let palette = Palette::codex();
        let mut surface = CrtSurface::new(40, 3, Style::default());
        surface.fill(0, 1, 40, ' ', palette.dim);
        let _ = draw_bubble_animated(
            &mut surface,
            0,
            40,
            Some("greets the operator"),
            &palette,
            BubbleAnim {
                reveal_chars: 5,
                glow: false,
            },
        );
        let mut buf = Buffer::empty(Rect::new(0, 0, 40, 3));
        surface.render(Rect::new(0, 0, 40, 3), &mut buf);
        let rendered: String = buf.content.iter().map(|c| c.symbol()).collect();
        // "< gre" (prefix + first 3 chars of message → 5 total)
        assert!(rendered.contains("< gre"));
        assert!(!rendered.contains("greets"));
    }

    #[test]
    fn typewriter_with_zero_reveal_skips_render() {
        let palette = Palette::codex();
        let mut surface = CrtSurface::new(40, 3, Style::default());
        let drew = draw_bubble_animated(
            &mut surface,
            0,
            40,
            Some("hello"),
            &palette,
            BubbleAnim {
                reveal_chars: 0,
                glow: false,
            },
        );
        assert!(drew.is_none());
    }

    #[test]
    fn settled_anim_matches_classic_draw_bubble() {
        let palette = Palette::codex();
        let mut a = CrtSurface::new(40, 3, Style::default());
        let mut b = CrtSurface::new(40, 3, Style::default());
        let _ = draw_bubble(&mut a, 0, 40, Some("hi"), &palette);
        let _ = draw_bubble_animated(&mut b, 0, 40, Some("hi"), &palette, BubbleAnim::settled());
        let mut buf_a = Buffer::empty(Rect::new(0, 0, 40, 3));
        let mut buf_b = Buffer::empty(Rect::new(0, 0, 40, 3));
        a.render(Rect::new(0, 0, 40, 3), &mut buf_a);
        b.render(Rect::new(0, 0, 40, 3), &mut buf_b);
        let ra: String = buf_a.content.iter().map(|c| c.symbol()).collect();
        let rb: String = buf_b.content.iter().map(|c| c.symbol()).collect();
        assert_eq!(ra, rb);
    }
}
