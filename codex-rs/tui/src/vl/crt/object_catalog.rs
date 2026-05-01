use super::palette::Palette;
use super::surface::CrtSurface;

fn frame_index(elapsed_ms: u64, frame_count: usize, frame_ms: u64) -> usize {
    if frame_count == 0 {
        return 0;
    }
    ((elapsed_ms / frame_ms.max(1)) as usize) % frame_count
}

fn draw_frame(
    surface: &mut CrtSurface,
    x: u16,
    width: u16,
    rows: [&str; 3],
    palette: &Palette,
) -> bool {
    for (y, row) in rows.into_iter().enumerate() {
        if row.is_empty() {
            continue;
        }
        let style = if y == 1 { palette.signal } else { palette.dim };
        surface.put_clipped(x, y as u16, width, row, style);
    }
    true
}

pub(super) fn draw_cube(
    surface: &mut CrtSurface,
    x: u16,
    width: u16,
    elapsed_ms: u64,
    palette: &Palette,
) -> bool {
    const FRAMES: [[&str; 3]; 4] = [
        ["    ", " [] ", "----"],
        ["    ", "[/] ", "----"],
        ["    ", "[*] ", "----"],
        ["    ", "[\\] ", "----"],
    ];
    draw_frame(
        surface,
        x,
        width,
        FRAMES[frame_index(elapsed_ms, FRAMES.len(), 520)],
        palette,
    )
}

pub(super) fn draw_crt_orb(
    surface: &mut CrtSurface,
    x: u16,
    width: u16,
    elapsed_ms: u64,
    palette: &Palette,
) -> bool {
    const FRAMES: [[&str; 3]; 4] = [
        ["     ", " (o) ", " --- "],
        ["     ", "((o))", " --- "],
        ["     ", " (@) ", " --- "],
        ["     ", "((o))", " --- "],
    ];
    draw_frame(
        surface,
        x,
        width,
        FRAMES[frame_index(elapsed_ms, FRAMES.len(), 580)],
        palette,
    )
}

pub(super) fn draw_nest(
    surface: &mut CrtSurface,
    x: u16,
    width: u16,
    elapsed_ms: u64,
    palette: &Palette,
) -> bool {
    const FRAMES: [[&str; 3]; 4] = [
        ["     ", "\\___/", " \\_/ "],
        ["     ", "\\_*_/", " \\_/ "],
        ["     ", "\\_o_/", " \\_/ "],
        ["     ", "\\_*_/", " \\_/ "],
    ];
    draw_frame(
        surface,
        x,
        width,
        FRAMES[frame_index(elapsed_ms, FRAMES.len(), 900)],
        palette,
    )
}

pub(super) fn draw_pillow(
    surface: &mut CrtSurface,
    x: u16,
    width: u16,
    elapsed_ms: u64,
    palette: &Palette,
) -> bool {
    const FRAMES: [[&str; 3]; 3] = [
        [" z  ", " ___", "(___)"],
        ["  z ", " _~_", "(___)"],
        ["   z", " ___", "(___)"],
    ];
    draw_frame(
        surface,
        x,
        width,
        FRAMES[frame_index(elapsed_ms, FRAMES.len(), 980)],
        palette,
    )
}

pub(super) fn draw_logbook(
    surface: &mut CrtSurface,
    x: u16,
    width: u16,
    elapsed_ms: u64,
    palette: &Palette,
) -> bool {
    const FRAMES: [[&str; 3]; 4] = [
        ["    ", " [] ", "/__\\"],
        ["    ", "[ ] ", "/___\\"],
        ["    ", "[*] ", "/___\\"],
        ["    ", "[ ] ", "/___\\"],
    ];
    draw_frame(
        surface,
        x,
        width,
        FRAMES[frame_index(elapsed_ms, FRAMES.len(), 620)],
        palette,
    )
}

pub(super) fn draw_test_chip(
    surface: &mut CrtSurface,
    x: u16,
    width: u16,
    elapsed_ms: u64,
    palette: &Palette,
) -> bool {
    const FRAMES: [[&str; 3]; 4] = [
        ["     ", " [#] ", "-===-"],
        ["     ", "((#))", "-===-"],
        ["     ", " [v] ", "-===-"],
        ["     ", "((#))", "-===-"],
    ];
    draw_frame(
        surface,
        x,
        width,
        FRAMES[frame_index(elapsed_ms, FRAMES.len(), 560)],
        palette,
    )
}

pub(super) fn draw_memory_shard(
    surface: &mut CrtSurface,
    x: u16,
    width: u16,
    elapsed_ms: u64,
    palette: &Palette,
) -> bool {
    const FRAMES: [[&str; 3]; 4] = [
        ["    ", " <> ", " || "],
        ["    ", "<*> ", " || "],
        ["    ", "<O> ", " || "],
        ["    ", "<*> ", " || "],
    ];
    draw_frame(
        surface,
        x,
        width,
        FRAMES[frame_index(elapsed_ms, FRAMES.len(), 700)],
        palette,
    )
}

pub(super) fn draw_scan_lens(
    surface: &mut CrtSurface,
    x: u16,
    width: u16,
    elapsed_ms: u64,
    palette: &Palette,
) -> bool {
    const FRAMES: [[&str; 3]; 4] = [
        ["     ", " <o> ", "-----"],
        ["     ", "(o)>>", "-----"],
        ["     ", " <O> ", "-----"],
        ["     ", "<<(o)", "-----"],
    ];
    draw_frame(
        surface,
        x,
        width,
        FRAMES[frame_index(elapsed_ms, FRAMES.len(), 540)],
        palette,
    )
}

pub(super) fn draw_signal_key(
    surface: &mut CrtSurface,
    x: u16,
    width: u16,
    elapsed_ms: u64,
    palette: &Palette,
) -> bool {
    const FRAMES: [[&str; 3]; 4] = [
        ["   ", "o- ", " | "],
        ["   ", "(o-", " | "],
        ["   ", "o-)", " | "],
        ["   ", "o- ", " | "],
    ];
    draw_frame(
        surface,
        x,
        width,
        FRAMES[frame_index(elapsed_ms, FRAMES.len(), 640)],
        palette,
    )
}

pub(super) fn draw_log_lantern(
    surface: &mut CrtSurface,
    x: u16,
    width: u16,
    elapsed_ms: u64,
    palette: &Palette,
) -> bool {
    const FRAMES: [[&str; 3]; 4] = [
        [" [] ", "[.*]", " || "],
        [" [] ", "[**]", " || "],
        [" [] ", "[##]", " || "],
        [" [] ", "[**]", " || "],
    ];
    draw_frame(
        surface,
        x,
        width,
        FRAMES[frame_index(elapsed_ms, FRAMES.len(), 480)],
        palette,
    )
}
