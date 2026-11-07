pub const CARD_WIDTH: f32 = 172.;
pub const CARD_GAP: f32 = 14.;
pub const PAGE_PADDING: f32 = 24.;
pub(crate) const LOAD_AHEAD: f32 = 900.;
pub fn card_columns(width: f32) -> usize {
    let spacing = CARD_GAP / 2.0;
    ((width + spacing) / (CARD_WIDTH + spacing))
        .floor()
        .max(1.0) as usize
}
