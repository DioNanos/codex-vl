# Vivling ASCII Card Experiments - 2026-05-15

Source image: `../../../sheets/First4Vivlings.png`.

This folder stores adult-form crop experiments for future `/vivling card`
ASCII work. The first raw crops included the lower terminal-sprite strip from
the source image, so the `*_subject_crop.png` files trim that strip away and are
the preferred inputs for ASCII conversion.

Directory layout:

- `crops/`: image crops. Use `*_adult_subject_crop.png` with Gemini.
- `prompts/`: reusable model prompts.
- `raw/`: first-pass converter output. Kept for traceability, not final art.
- `generated/`: later automatic ASCII attempts. Reference only.

Useful files:

- `prompts/gemini_ascii_card_prompt.md`: prompt to paste into Gemini together
  with the cropped images.
- `crops/*_adult_subject_crop.png`: preferred image inputs for Gemini.

Generated families:

- `crops/*_adult_crop.png`: first adult crop, includes bottom source strip.
- `crops/*_adult_subject_crop.png`: adult crop with the bottom strip removed.
- `raw/*_adult_ppmtoascii_*.txt`: raw Netpbm conversion, too noisy for final
  use.
- `raw/*_adult_ascii_gray_48.txt`: grayscale conversion from the raw crop,
  noisy.
- `raw/*_adult_ascii_threshold_48.txt`: threshold conversion from the raw crop,
  still polluted by the bottom strip.
- `generated/*_adult_subject_ascii_shaded_36x28.txt`: compact shaded
  candidate.
- `generated/*_adult_subject_ascii_silhouette_36x28.txt`: compact silhouette
  candidate.
- `generated/*_adult_subject_ascii_shaded_44x34.txt`: larger shaded candidate.
- `generated/*_adult_subject_ascii_silhouette_44x34.txt`: larger silhouette
  candidate.
- `generated/*_adult_subject_ascii_silhouette_28x20.txt`: tiny preview
  candidate for chat review.

Current recommendation: use the `36x28` subject candidates only as reference,
not as final card art. The conversion proves the silhouettes can be extracted,
but hand-curated ASCII will still be needed for polished card panels.
