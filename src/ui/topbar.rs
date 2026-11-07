const ITEM_SPACING: f32 = 8.;
const SEARCH_IDEAL: f32 = 200.;
const SEARCH_MAX: f32 = 440.;
const SEARCH_FLOOR: f32 = 130.;
const SEARCH_MIN: f32 = 80.;
const AVATAR_SIZE: f32 = 36.;
const ICON_BUTTON_SIZE: f32 = 31.;
const RIGHT_CONTROLS_WIDTH: f32 =
    super::widgets::PAGE_PADDING + AVATAR_SIZE + 4.0 + 3.0 * ICON_BUTTON_SIZE + 3.0 * ITEM_SPACING;
#[derive(Clone, Copy, Debug, PartialEq)]
struct TopbarFit {
    /// How wide the search field may be.
    search: f32,
    /// Whether the badges have the room to spell themselves out.
    labels: bool,
}

fn topbar_fit(room: f32, controls: f32, labelled: f32, icons: f32) -> TopbarFit {
    // SEARCH_IDEAL is above SEARCH_FLOOR, so the clamp below is well ordered.
    let ideal = (room * 0.5).clamp(SEARCH_IDEAL, SEARCH_MAX);
    let labels = room - controls - labelled >= SEARCH_FLOOR;
    let badges = if labels { labelled } else { icons };
    TopbarFit {
        search: (room - controls - badges).clamp(SEARCH_MIN, ideal),
        labels,
    }
}
pub(super) fn navidrome_search_width(bar_width: f32, sidebar_visible: bool) -> f32 {
    let button_and_gap = 32. + ITEM_SPACING;
    let lead = super::widgets::PAGE_PADDING + 2. * button_and_gap + 8.;
    let sidebar_controls = if sidebar_visible {
        0.
    } else {
        2. * button_and_gap + 2.
    };
    topbar_fit(
        bar_width - lead - sidebar_controls,
        RIGHT_CONTROLS_WIDTH,
        0.,
        0.,
    )
    .search
}
