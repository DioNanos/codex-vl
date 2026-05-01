use super::director::CrtMode;
use super::layers::compose_expression;
use super::palette::Palette;
use super::scene::CrtScene;
use super::surface::CrtSurface;

pub(crate) fn render_scene(
    surface: &mut CrtSurface,
    scene: &CrtScene<'_>,
    mode: CrtMode,
    palette: &Palette,
) {
    let name_phase = scene
        .name
        .bytes()
        .fold(0u64, |acc, byte| acc.wrapping_add(byte as u64))
        % 4;
    compose_expression(
        surface,
        mode,
        scene.elapsed_ms + name_phase * 45,
        palette,
        scene.last_message,
        scene.activity,
        scene.seed,
        scene.tier,
        scene.species_id,
        scene.stage,
    );
}
