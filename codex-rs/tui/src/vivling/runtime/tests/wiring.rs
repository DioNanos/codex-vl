//! codex-vl — pin del cablaggio di ticking.
//!
//! Il render del wrapper è read-only salvo `CrtAnimationLedger` (v.
//! nota di perimetro su `Vivling::crt_animation_ledger` —
//! `Renderable::render(&self)`):
//! l'expiry del testo animato e il frame pacing vivono in
//! `Vivling::tick(&mut)`, invocato da `BottomPane::vl_lifecycle_tick`
//! nel percorso pre-draw. Questo test pinnia il link; la catena a monte
//! (pre_draw -> `ChatWidget::vl_lifecycle_tick`) è pinnata dal test
//! `codex_vl_pre_draw_tick_invokes_vl_lifecycle_tick_with_canonical_args`.
//! Vive sotto `vivling::` perché il filtro CI dei gate esegue
//! `--lib vivling::`: un pin che non gira non esiste.

/// Source di `bottom_pane/vl_ext.rs` (il modulo che possiede il hook
/// `vl_lifecycle_tick`): path relativo da `vivling/runtime/tests/`.
const VL_EXT_SOURCE: &str = include_str!("../../../bottom_pane/vl_ext.rs");

/// Mirror di `extract_fn_body` (bottom_pane::vl_ext::tests): estrae il
/// body bilanciando le graffe.
fn extract_fn_body<'a>(source: &'a str, fn_name: &str) -> Option<&'a str> {
    let needle = format!("fn {fn_name}");
    let mut cursor = 0usize;
    loop {
        let hit = source[cursor..].find(&needle)?;
        let name_end = cursor + hit + needle.len();
        let after = source.as_bytes().get(name_end).copied()?;
        if !matches!(after, b'(' | b'<') {
            cursor = name_end;
            continue;
        }
        let open = source[name_end..].find('{')? + name_end;
        let bytes = source.as_bytes();
        let mut depth = 0i32;
        for (idx, &b) in bytes.iter().enumerate().skip(open) {
            match b {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(&source[open + 1..idx]);
                    }
                }
                _ => {}
            }
        }
        return None;
    }
}

#[test]
fn vl_lifecycle_tick_drives_vivling_tick() {
    let body = extract_fn_body(VL_EXT_SOURCE, "vl_lifecycle_tick")
        .expect("vl_lifecycle_tick must exist in bottom_pane/vl_ext.rs");
    assert!(
        body.contains("self.vivling.tick("),
        "vl_lifecycle_tick must drive Vivling::tick (the &mut per-frame hook: \
         animation-text expiry + frame pacing) — with the render read-only \
         (except CrtAnimationLedger, see the perimeter note on \
         Vivling::crt_animation_ledger), a missing call would freeze the \
         CRT animation in production. \
         Body was:\n{body}"
    );
}
