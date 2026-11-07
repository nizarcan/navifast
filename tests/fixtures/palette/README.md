# Palette compatibility fixture

Captured from Navifast commit `5426a6e` before retiring the legacy UI dependencies.
The 56 RGBA samples record saved premultiplied bytes, unpremultiplication,
fading, interpolation, linear blending and dark/light spectrum colors.
`tests/palette.rs` and `src/ui/player_bar.rs` check the native color helpers
against these fixed results without linking the retired renderer.
