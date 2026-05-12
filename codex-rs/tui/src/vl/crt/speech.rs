use ratatui::style::Modifier;

use super::palette::Palette;
use super::surface::CrtSurface;

pub(crate) const PANEL_MIN_WIDTH: u16 = 6;
const BUBBLE_PREFIX: &str = "< ";
const MAX_BUBBLE_LINES: usize = 2;

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

/// Render a (possibly multi-line) speech bubble next to the face.
///
/// Layout: first line is written on row 1 (the face row); a single
/// continuation line is written on row 2 (DIM via the ambient scanline
/// effect). The bubble never spills onto row 0 — that row stays
/// reserved for the CRT ambient layer.
///
/// Returns the width consumed by the widest line (including the leading
/// `< ` marker) so callers can lay out neighbouring widgets without
/// reflow when the typewriter reveals more characters.
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
    let cleaned = last_message.and_then(prepare_bubble)?;
    let prefixed = format!("{BUBBLE_PREFIX}{cleaned}");
    let lines = word_wrap(&prefixed, panel_w as usize, MAX_BUBBLE_LINES);
    if lines.is_empty() {
        return None;
    }

    let total_chars: usize = lines.iter().map(|s| s.chars().count()).sum();
    if total_chars == 0 {
        return None;
    }
    let revealed = anim.reveal_chars.min(total_chars);
    if revealed == 0 {
        return None;
    }

    let widest = lines
        .iter()
        .map(|l| l.chars().count())
        .max()
        .unwrap_or(0)
        .min(panel_w as usize) as u16;

    let base_style = if anim.glow {
        palette.signal.add_modifier(Modifier::BOLD)
    } else {
        palette.signal
    };

    let mut remaining = revealed;
    // First line on row 1 (face row), continuation on row 2.
    // Row 0 stays untouched to keep the CRT ambient layer alive.
    let line_rows: [u16; MAX_BUBBLE_LINES] = [1, 2];
    for (idx, line) in lines.iter().take(MAX_BUBBLE_LINES).enumerate() {
        if remaining == 0 {
            break;
        }
        let count = line.chars().count();
        let take = remaining.min(count);
        let to_write: String = line.chars().take(take).collect();
        let y = line_rows[idx];
        surface.put_clipped(panel_x, y, panel_w, &to_write, base_style);
        remaining = remaining.saturating_sub(take);
    }
    Some(widest)
}

pub(crate) fn bubble_width(last_message: Option<&str>, panel_w: u16) -> u16 {
    if panel_w < PANEL_MIN_WIDTH {
        return 0;
    }
    let Some(cleaned) = last_message.and_then(prepare_bubble) else {
        return 0;
    };
    let prefixed = format!("{BUBBLE_PREFIX}{cleaned}");
    let lines = word_wrap(&prefixed, panel_w as usize, MAX_BUBBLE_LINES);
    lines
        .iter()
        .map(|l| l.chars().count())
        .max()
        .unwrap_or(0)
        .min(panel_w as usize) as u16
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
    let trimmed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn prepare_bubble(raw: &str) -> Option<String> {
    prepare(raw)
}

/// Word-wrap `text` to at most `max_lines` lines of width `width`. Lines
/// break on whitespace when possible; an overlong single token is
/// hard-broken. The final line gets an `..` ellipsis if there is still
/// content left to render. Returns the wrapped lines (each <= `width`).
fn word_wrap(text: &str, width: usize, max_lines: usize) -> Vec<String> {
    if width == 0 || max_lines == 0 {
        return Vec::new();
    }
    let mut lines: Vec<String> = Vec::with_capacity(max_lines);
    let mut current = String::new();
    let mut iter = text.split_whitespace().peekable();
    while let Some(word) = iter.next() {
        let word_len = word.chars().count();
        if current.is_empty() {
            if word_len <= width {
                current.push_str(word);
            } else {
                // Hard-break a too-long single token; consume the head,
                // push the tail back logically by truncating the input
                // is not straightforward, so just truncate.
                current.extend(word.chars().take(width.saturating_sub(2)));
                current.push_str("..");
                lines.push(std::mem::take(&mut current));
                if lines.len() >= max_lines {
                    return lines;
                }
            }
            continue;
        }
        // Try to append `word` (with a space) to the current line.
        if current.chars().count() + 1 + word_len <= width {
            current.push(' ');
            current.push_str(word);
        } else {
            lines.push(std::mem::take(&mut current));
            if lines.len() >= max_lines {
                // No room for the rest; mark ellipsis on the previous line.
                if let Some(last) = lines.last_mut() {
                    truncate_with_ellipsis(last, width);
                }
                return lines;
            }
            if word_len <= width {
                current.push_str(word);
            } else {
                current.extend(word.chars().take(width.saturating_sub(2)));
                current.push_str("..");
                lines.push(std::mem::take(&mut current));
                if lines.len() >= max_lines {
                    return lines;
                }
            }
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    // If we consumed all words but still have content, mark final ellipsis
    // only when truncation occurred (handled inside the loop). Otherwise
    // the lines are complete.
    lines
}

fn truncate_with_ellipsis(line: &mut String, width: usize) {
    if line.chars().count() <= width {
        // Append ".." if there is room and the line is not already
        // ellipsised. We treat the trailing two chars as the ellipsis
        // marker.
        if width >= 2 {
            let cut = width.saturating_sub(2);
            let head: String = line.chars().take(cut).collect();
            *line = format!("{head}..");
        }
    }
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
        assert!(rendered.contains("< greets the operator"));
    }

    #[test]
    fn wraps_long_messages_onto_a_second_line() {
        let (drew, rendered) = render(
            20,
            Some("greets the operator and waits for the next move"),
        );
        assert!(drew);
        // Row 1 ("face row") and row 2 are written by the bubble.
        // First line should end at or before the wrap point.
        assert!(rendered.contains("< greets the"));
        // The continuation line should carry the rest of the words.
        assert!(rendered.contains("operator") || rendered.contains("waits"));
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

    #[test]
    fn word_wrap_respects_max_lines_with_ellipsis() {
        let lines = word_wrap(
            "one two three four five six seven eight nine ten eleven",
            10,
            2,
        );
        assert_eq!(lines.len(), 2);
        for l in &lines {
            assert!(l.chars().count() <= 10, "line too wide: {l:?}");
        }
        // Final line truncates with "..".
        assert!(lines.last().unwrap().ends_with(".."));
    }

    #[test]
    fn word_wrap_keeps_single_short_line_intact() {
        let lines = word_wrap("hello world", 40, 2);
        assert_eq!(lines, vec!["hello world".to_string()]);
    }

    #[test]
    fn word_wrap_hard_breaks_oversize_single_token() {
        let lines = word_wrap("supercalifragilistic", 8, 2);
        assert!(lines[0].ends_with(".."));
        assert!(lines[0].chars().count() <= 8);
    }

    #[test]
    fn long_message_writes_on_row_1_and_row_2_not_row_0() {
        let palette = Palette::codex();
        let mut surface = CrtSurface::new(40, 3, Style::default());
        let _ = draw_bubble_animated(
            &mut surface,
            0,
            40,
            Some(
                "tracking a long-running pattern and watching the work flow across many turns",
            ),
            &palette,
            BubbleAnim::settled(),
        );
        let row0: String = (0..40).map(|x| surface.ch_at(x, 0)).collect();
        // Row 0 should remain untouched by the bubble (only spaces).
        assert!(
            row0.chars().all(|c| c == ' '),
            "row 0 unexpectedly written: {row0:?}"
        );
        let row1: String = (0..40).map(|x| surface.ch_at(x, 1)).collect();
        let row2: String = (0..40).map(|x| surface.ch_at(x, 2)).collect();
        let any_non_space = |s: &str| s.chars().any(|c| c != ' ');
        assert!(any_non_space(&row1));
        assert!(any_non_space(&row2));
    }
}
