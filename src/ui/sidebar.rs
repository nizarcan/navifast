use crate::{
    app::App,
    settings::{LibraryShelf as Filter, LibrarySort},
};
pub(crate) fn selected_sort(app: &App, shelf: Filter) -> LibrarySort {
    app.settings
        .library_sort
        .get(&shelf)
        .copied()
        .filter(|sort| sort.supports(shelf))
        .unwrap_or(LibrarySort::Library)
}
const GRID_MIN_CARD_WIDTH: f32 = 108.0;
const GRID_MAX_COLUMNS: usize = 4;
const GRID_TEXT_HEIGHT: f32 = 44.0;

#[derive(Clone, Copy)]
pub(super) struct GridLayout {
    pub(super) columns: usize,
    pub(super) card_width: f32,
    pub(super) row_height: f32,
}

pub(super) fn grid_layout(width: f32) -> GridLayout {
    let columns = ((width / GRID_MIN_CARD_WIDTH).floor() as usize).clamp(2, GRID_MAX_COLUMNS);
    let card_width = (width / columns as f32).max(1.0);
    let card_height = card_width + GRID_TEXT_HEIGHT;
    GridLayout {
        columns,
        card_width,
        row_height: card_height,
    }
}
