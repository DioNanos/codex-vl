pub(crate) fn fixed_sprite(raw: &str) -> String {
    let mut out: String = raw.chars().take(10).collect();
    while out.chars().count() < 10 {
        out.push(' ');
    }
    out
}

pub(crate) fn code(raw: &str, max: usize) -> String {
    raw.chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .take(max)
        .flat_map(|ch| ch.to_uppercase())
        .collect()
}

pub(crate) fn role_code(raw: &str) -> String {
    match raw {
        "builder" => "BLD".to_string(),
        "reviewer" => "RVW".to_string(),
        "researcher" => "RSR".to_string(),
        "operator" => "OPS".to_string(),
        _ => "VL".to_string(),
    }
}

pub(crate) fn playfield(seed: u32, elapsed_ms: u64, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let inner = width.saturating_sub(2).max(1);
    let mut ball = crate::vl::lifecycle::minigame::BouncingBall::new_with_seed(seed);
    ball.step(elapsed_ms, inner);
    let frame = ball.frame(inner);
    let mut chars: Vec<char> = frame
        .chars()
        .map(|ch| if ch == ' ' { '-' } else { ch })
        .collect();
    if width >= 2 {
        chars.insert(0, '[');
        chars.push(']');
    }
    chars.into_iter().collect()
}
