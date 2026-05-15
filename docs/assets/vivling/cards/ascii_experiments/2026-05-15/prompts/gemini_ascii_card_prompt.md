# Gemini Prompt - Vivling ASCII Card Art

Use this prompt with the cropped adult Vivling images in the sibling `crops/`
folder:

- `../crops/syllo_adult_subject_crop.png`
- `../crops/orchestra_adult_subject_crop.png`
- `../crops/chronosworn_adult_subject_crop.png`
- `../crops/zed_adult_subject_crop.png`

Attach one image at a time, or attach all four and ask for separate sections.

```text
You are designing hand-curated ASCII art for a terminal UI companion card.

Input: illustrated adult creature image(s).

Important:
- Do NOT rasterize the image pixel by pixel.
- Do NOT create dense grayscale ASCII.
- Do NOT use ANSI color, Unicode, emoji, box drawing, or shaded block characters.
- Use ASCII characters only: space, . , ' ` - _ / \ | ( ) [ ] { } < > ^ v o O 0 * + = : ; ! ? # @
- The result must look intentionally drawn, not automatically converted.

Target UI:
- Rust TUI card panel for codex-vl Vivling.
- Art must be readable in a small terminal card.
- Prefer iconic silhouette over detail.
- Preserve the creature identity:
  - recognizable head / face shape
  - body outline
  - one signature accessory or silhouette feature
  - no noisy background

Hard layout constraints:
- Width: exactly 28 columns per line.
- Height: 12 to 16 lines.
- Every line in one variant must have the same width.
- ASCII only.
- No trailing comments inside the art block.
- Use fenced code blocks for each variant.

Task:
For each attached Vivling image, create 5 ASCII card-art variants.

Style directions:
1. Variant A: minimal readable silhouette.
2. Variant B: more expressive face/head.
3. Variant C: stronger species accessory/symbol.
4. Variant D: compact card-safe version.
5. Variant E: bold high-contrast terminal version.

For each variant:
- Provide the ASCII block.
- Then add one short note outside the block:
  "Preserves: <3-6 visual traits>."

Species context:
- Syllo: codeweaver, insectoid/branching horns, cloak, terminal scroll/panel.
- Orchestra: conductor of agents, elegant conductor shape, baton/orchestra cues.
- Chronosworn: loop guardian, clock/time silhouette, stoic guardian presence.
- ZED: mythic narrator/presenter, prime signal, not a normal pet companion.

Evaluation criteria:
- I should be able to paste the ASCII into a terminal card and immediately tell which creature it is.
- Sparse and intentional beats detailed and noisy.
- If the image has too much detail, simplify aggressively.
- Prefer clean outlines, face marks, and one symbolic prop.

Return format:

## <Species Name>

### Variant A
```ascii
<28 columns wide, 12-16 lines>
```
Preserves: ...

### Variant B
```ascii
...
```
Preserves: ...

Continue for C, D, E.
```
