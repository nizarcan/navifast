use super::*;
use crate::app::navidrome::{Action as Service, search::Action as Search};
use crate::model::Loadable;
use crate::navidrome::api::{Album, Collection, FavoriteKind, SearchPart, Song};
use std::borrow::Cow;

pub(super) fn shell(v: &Paint<'_>) -> El {
    let mut body = row().w_full().grow(1.).min_h(0.);
    let s = v.s;
    let side = if s.app.presentation.settings.sidebar_visible {
        s.app
            .presentation
            .settings
            .sidebar_width
            .clamp(210., (v.width - 340.).max(210.))
    } else {
        0.
    };
    if side > 0. {
        let max_width = (v.width - 340.).max(210.);
        let zoom = v.zoom;
        body = body.child(sidebar(v, side - 3.)).child(
            el().key("resize-sidebar")
                .w(3.)
                .h_full()
                .role("slider")
                .label(v.tr("Library width"))
                .range_value(f64::from(side), 210., f64::from(max_width))
                .range_step(10.)
                .on_step(move |s: &mut State, dir, _| {
                    s.app.presentation.settings.sidebar_width =
                        (s.app.presentation.settings.sidebar_width + 10. * dir as f32)
                            .clamp(210., max_width);
                    s.app.mark_dirty();
                })
                .on_drag(move |s: &mut State, d, tx| match d.phase {
                    rune::Phase::Down => d.data = s.app.presentation.settings.sidebar_width,
                    rune::Phase::Move => {
                        s.app.presentation.settings.sidebar_width =
                            (d.data + d.dx / zoom).clamp(210., max_width);
                        tx.instant();
                    }
                    rune::Phase::Up => {
                        if d.cancelled() {
                            s.app.presentation.settings.sidebar_width = d.data;
                        }
                        s.app.mark_dirty();
                    }
                }),
        );
    }
    let page = s.app.presentation.page();
    let content = match page {
        Page::Album(_) => songs(v),
        Page::Playlist(_) => playlist(v),
        Page::Search => search(v),
        Page::Artist(_) => collection(v),
        Page::LikedSongs | Page::FavoriteGroup(_) => favorites(v),
        Page::Settings => preferences::settings(v),
        _ => albums(v, v.width - side),
    };
    let mut scroller = col()
        .key("page")
        .grow(1.)
        .min_h(0.)
        .w_full()
        .px(if matches!(page, Page::Artist(_)) {
            0.
        } else {
            24.
        })
        .pt(match page {
            Page::Artist(_) | Page::LikedSongs | Page::FavoriteGroup(_) | Page::Search => 8.,
            Page::Settings => 16.,
            _ => 22.,
        })
        .pb(48.)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(content);
    let shadow = rune::Live::new(0.);
    if let Some(offset) = s.offsets.get(page).cloned() {
        let opacity = shadow.clone();
        let depth = |y: f32| (y / 24.).clamp(0., 1.);
        opacity.set(depth(offset.get()));
        scroller = scroller.on_scroll_offset(move |pos| {
            offset.set(pos.offset[1]);
            opacity.set(depth(pos.offset[1]));
        });
    }
    let mut center = col().min_w(0.).grow(1.).h_full();
    if let Some(tint) = page_tint(v, page) {
        let strength = if matches!(
            page,
            Page::Home | Page::Search | Page::Settings | Page::Queue
        ) {
            0.45
        } else {
            0.85
        };
        center = center.child(
            el().absolute(0., 0.)
                .w_full()
                .h(340.)
                .gradient(
                    color(crate::ui::blend(v.p.window, tint, strength)),
                    color(v.p.window),
                )
                .a11y_hidden(),
        );
    }
    let center = center.child(topbar(v)).child(scroller).child(
        el().absolute(0., crate::theme::TOP_BAR_HEIGHT + v.s.titlebar_inset())
            .w_full()
            .h(14.)
            .live_opacity(&shadow)
            .gradient(
                color(Color32::from_black_alpha(if v.p.dark { 110 } else { 36 })),
                Color::TRANSPARENT,
            )
            .a11y_hidden(),
    );
    let mut shell = col()
        .w_full()
        .h_full()
        .fill(color(v.p.window))
        .child(body.child(center))
        .child(transport::player(v));
    if let Some(msg) = s.app.presentation.toasts.last() {
        shell = shell.child(
            label(msg.message.clone(), 14., 500., v.p.text)
                .padding(12.)
                .radius(8.)
                .fill(color(v.p.overlay))
                .absolute(24., v.height - 145.),
        );
    }
    // Anchors resolve against their trigger anywhere in the tree. Mount menus
    // after the page so later virtual rows cannot cover their pixels or hits.
    if let Some(menu) = match s.menu {
        Some("account") => Some(account_menu(v)),
        Some("sort") => Some(sort_menu(v)),
        Some("album") => s.app.album.as_ref().map(|a| {
            let uri = FavoriteKind::Album.uri(&a.id);
            let saved = s.app.presentation.navidrome_favorites.contains(&uri) == Some(true);
            let name = v.tr(if saved {
                "Remove from Favorites"
            } else {
                "Add to Favorites"
            });
            v.menu("album-more")
                .child(v.menu_item(&name, Some(Icon::Star), move |s, tx| {
                    s.dispatch(Action::ToggleSaved(uri.clone()), tx)
                }))
        }),
        Some(key @ ("themes" | "languages")) => Some(preferences::choice_menu(v, key)),
        _ => None,
    } {
        shell = shell.child(menu);
    }
    if let Some((uri, [x, y])) = &s.favorite_menu {
        let name = if s.app.presentation.navidrome_favorites.contains(uri) == Some(true) {
            "Remove from Favorites"
        } else {
            "Add to Favorites"
        };
        let uri = uri.clone();
        shell = shell.child(
            col()
                .absolute(
                    x.clamp(0., (v.width - 200.).max(0.)),
                    y.clamp(0., (v.height - 48.).max(0.)),
                )
                .w(200.)
                .padding(5.)
                .radius(8.)
                .fill(color(v.p.overlay))
                .role("menu")
                .modal(true, |s: &mut State| s.favorite_menu = None)
                .child(
                    v.button(&v.tr(name), false, move |s, tx| {
                        s.dispatch(Action::ToggleSaved(uri.clone()), tx)
                    })
                    .role("menuitem"),
                ),
        );
    }
    if s.app.presentation.dialog.is_some() {
        shell = shell.child(preferences::shortcuts(v));
    }
    shell
}
fn page_tint(v: &Paint<'_>, page: &Page) -> Option<Color32> {
    if let Page::LikedSongs = page {
        return Some(Color32::from_rgb(0x50, 0x38, 0xc8));
    }
    let app = &v.s.app;
    if matches!(page, Page::Artist(_)) || !app.presentation.settings.accent_from_art {
        return None;
    }
    let tint = |id: &str| v.s.cover_tint(&app.cover_uri(id), v.p);
    let album = if let Page::Album(id) = page {
        let id = navidrome::decode(id)?;
        app.album
            .as_ref()
            .filter(|a| a.id == id)
            .or_else(|| app.albums.iter().find(|a| a.id == id))
    } else {
        None
    };
    album
        .and_then(|a| a.cover_art.as_deref())
        .and_then(tint)
        .or_else(|| app.playing_cover().and_then(tint))
}

fn topbar(v: &Paint<'_>) -> El {
    let mut bar = row()
        .key("topbar")
        .h(crate::theme::TOP_BAR_HEIGHT + v.s.titlebar_inset())
        .pt(0.)
        .px(24.)
        .gap(8.)
        .items_center()
        .window_drag();
    if !v.s.app.presentation.settings.sidebar_visible {
        bar = bar
            .child(
                v.icon_button(&v.tr("Show sidebar"), Icon::PanelLeft, 18., |s, tx| {
                    s.dispatch(Action::ToggleSidebar, tx)
                }),
            )
            .child(v.icon_button(&v.tr("Home"), Icon::House, 18., |s, tx| {
                s.dispatch(Action::Open(Page::Albums), tx)
            }));
    }
    bar = bar
        .child(
            v.icon_button(&v.tr("Back"), Icon::ChevronLeft, 18., |s, tx| {
                s.dispatch(Action::Back, tx)
            })
            .fill(color(v.p.panel))
            .disabled(!v.s.app.presentation.can_go_back()),
        )
        .child(
            v.icon_button(&v.tr("Forward"), Icon::ChevronRight, 18., |s, tx| {
                s.dispatch(Action::Forward, tx)
            })
            .fill(color(v.p.panel))
            .disabled(!v.s.app.presentation.can_go_forward()),
        );
    let input = text_input(&v.s.query, TextStyle::new(14., 400.))
        .single_line()
        .key("query")
        .label(v.tr("Search"))
        .grow(1.)
        .min_w(0.)
        .h(32.)
        .color(color(v.p.text))
        .placeholder(v.tr("What do you want to play?"), color(v.p.secondary))
        .on_edit(|s: &mut State| &mut s.query)
        .on_input_change(|s: &mut State, tx| {
            if s.app.presentation.page() != &Page::Search {
                s.dispatch(Action::Open(Page::Search), tx);
            }
            s.dispatch(
                Action::NavidromeSearch(Search::Edit(s.query.text.clone())),
                tx,
            );
        })
        .on_input_key(|s: &mut State, ev, tx| {
            let down = match ev.key {
                rune::Key::Down => true,
                rune::Key::Up => false,
                _ => return false,
            };
            s.dispatch(Action::NavidromeSearch(Search::Move(down)), tx);
            if let Some(hit) = s.app.presentation.navidrome_search.selected {
                tx.scroll_into_view(format!("hit-{hit:?}"));
            }
            true
        })
        .on_submit(|s: &mut State, tx| s.dispatch(Action::NavidromeSearch(Search::Submit), tx));
    bar.child(
        row()
            .w((v.content_width() - 205.).clamp(56., 440.))
            .max_w(440.)
            .min_w(56.)
            .h(34.)
            .px(12.)
            .gap(8.)
            .items_center()
            .radius(17.)
            .fill(color(v.p.surface))
            .child(v.icon(Icon::Search, 16., v.p.secondary))
            .child(input)
            .child(if v.s.query.text.is_empty() {
                el()
            } else {
                v.icon_button(&v.tr("Clear"), Icon::X, 14., |s, tx| {
                    s.query.clear();
                    s.dispatch(Action::NavidromeSearch(Search::Edit(String::new())), tx);
                })
            }),
    )
    .child(el().grow(1.))
    .child(
        v.icon_button(&v.tr("Settings"), Icon::Settings, 18., |s, tx| {
            s.dispatch(Action::Open(Page::Settings), tx)
        }),
    )
    .child(
        el().key("account")
            .size(36., 36.)
            .center()
            .radius(18.)
            .fill(color(v.p.surface))
            .hover(color(v.p.surface_hover))
            .role("button")
            .label(v.tr("Account"))
            .focusable(true)
            .child(
                el().size(28., 28.)
                    .center()
                    .radius(14.)
                    .fill(color(v.p.accent))
                    .child(label(
                        v.s.app
                            .preferences
                            .username
                            .chars()
                            .next()
                            .unwrap_or('?')
                            .to_uppercase()
                            .to_string(),
                        13.,
                        700.,
                        v.p.on_accent,
                    )),
            )
            .on_press_tx(|s: &mut State, _| s.menu = Some("account")),
    )
}
fn account_menu(v: &Paint<'_>) -> El {
    let rule = || el().w_full().h(1.).fill(color(v.p.outline)).my(4.);
    v.menu("account")
        .child(
            label(&v.s.app.preferences.username, 14., 600., v.p.text)
                .mx(10.)
                .mt(4.),
        )
        .child(label("Navidrome", 12., 400., v.p.secondary).mx(10.))
        .child(rule())
        .child(
            v.menu_item(&v.tr("Settings"), Some(Icon::Settings), |s, tx| {
                s.dispatch(Action::Open(Page::Settings), tx)
            }),
        )
        .child(
            v.menu_item(&v.tr("Keyboard shortcuts"), Some(Icon::Info), |s, tx| {
                s.dispatch(Action::ShowDialog(crate::model::Dialog::Shortcuts), tx)
            })
            .key("account-shortcuts"),
        )
        .child(rule())
        .child(v.menu_item(&v.tr("Sign out"), Some(Icon::LogOut), |s, tx| {
            s.dispatch(Action::SignOut, tx)
        }))
}

const SORTS: [(crate::settings::LibrarySort, &str); 4] = [
    (crate::settings::LibrarySort::Library, "Library order"),
    (
        crate::settings::LibrarySort::RecentlyPlayed,
        "Recently played",
    ),
    (crate::settings::LibrarySort::Name, "Name"),
    (
        crate::settings::LibrarySort::RecentlyAdded,
        "Recently added",
    ),
];
fn sort_menu(v: &Paint<'_>) -> El {
    use crate::settings::LibraryShelf;
    let selected = crate::ui::sidebar::selected_sort(&v.s.app.presentation, LibraryShelf::Albums);
    let mut menu = v
        .menu("sort")
        .anchored_fit("sort", rune::Side::Bottom, rune::Align::Start, 4.);
    for (sort, title) in SORTS {
        menu = menu.child(v.menu_item(
            &v.tr(title),
            (sort == selected).then_some(Icon::Check),
            move |s, tx| {
                s.dispatch(
                    Action::SetLibrarySort {
                        shelf: LibraryShelf::Albums,
                        sort,
                    },
                    tx,
                )
            },
        ));
    }
    menu
}
fn sidebar(v: &Paint<'_>, width: f32) -> El {
    let app = &v.s.app;
    let mut side = col()
        .key("sidebar")
        .w(width)
        .h_full()
        .fill(color(v.p.panel))
        .px(12.)
        .pt(12. + v.s.titlebar_inset());
    for (name, icon, page) in [
        ("Albums", Icon::House, Page::Albums),
        ("Favorites", Icon::Star, Page::LikedSongs),
    ] {
        let ink = if &page == app.presentation.page() {
            v.p.text
        } else {
            v.p.secondary
        };
        side = side.child(
            row()
                .h(46.)
                .px(12.)
                .gap(16.)
                .items_center()
                .role("button")
                .label(v.tr(name))
                .focusable(true)
                .hover(color(v.p.surface_hover))
                .child(v.icon(icon, 22., ink))
                .child(label(v.tr(name), 15., 650., ink))
                .on_press_tx(move |s: &mut State, tx| s.dispatch(Action::Open(page.clone()), tx)),
        );
    }
    side = side
        .child(el().w_full().h(1.).mt(12.).fill(color(v.p.outline)))
        .child(
            row()
                .h(48.)
                .items_center()
                .gap(8.)
                .child(v.icon(Icon::Library, 20., v.p.secondary))
                .child(label(v.tr("Library"), 16., 650., v.p.text).grow(1.))
                .child(v.icon_button(
                    &v.tr("Search in Your Library"),
                    Icon::Search,
                    16.,
                    |s, _| {
                        s.library_search = !s.library_search;
                        if s.library_search {
                            s.library_filter.focused = true;
                        } else {
                            s.library_filter.clear();
                        }
                    },
                ))
                .child(
                    v.icon_button(&v.tr("Grid"), Icon::LayoutGrid, 16., |s, tx| {
                        s.dispatch(
                            Action::SetLibraryGrid(!s.app.presentation.settings.sidebar_grid),
                            tx,
                        )
                    }),
                )
                .child(v.icon_button(
                    &v.tr("Show or hide the sidebar"),
                    Icon::PanelLeft,
                    16.,
                    |s, tx| s.dispatch(Action::ToggleSidebar, tx),
                )),
        )
        .child(
            row().mt(6.).child(
                row()
                    .h(30.)
                    .px(12.)
                    .center()
                    .radius(15.)
                    .fill(color(v.p.text))
                    .role("button")
                    .label(v.tr("Albums"))
                    .focusable(true)
                    .child(label(v.tr("Albums"), 13., 500., v.p.window))
                    .on_press_tx(|_: &mut State, _| {}),
            ),
        );
    let selected = crate::ui::sidebar::selected_sort(&app.presentation, LibraryShelf::Albums);
    let name = SORTS
        .iter()
        .find(|(sort, _)| *sort == selected)
        .map_or("Library order", |(_, name)| *name);
    side = side.child(
        row().mt(6.).mb(8.).child(
            row()
                .key("sort")
                .h(28.)
                .px(8.)
                .gap(4.)
                .items_center()
                .radius(12.)
                .fill(color(v.p.surface))
                .hover(color(v.p.surface_hover))
                .role("button")
                .label(v.tr(name))
                .focusable(true)
                .child(v.icon(Icon::ChevronDown, 15., v.p.text))
                .child(label(v.tr(name), 13., 500., v.p.text))
                .on_press_tx(|s: &mut State, _| s.menu = Some("sort")),
        ),
    );
    if v.s.library_search {
        side = side.child(
            text_input(&v.s.library_filter, TextStyle::new(14., 400.))
                .key("library-filter")
                .single_line()
                .label(v.tr("Search in Your Library"))
                .placeholder(v.tr("Search in Your Library"), color(v.p.secondary))
                .h(32.)
                .px(8.)
                .radius(8.)
                .fill(color(v.p.surface))
                .color(color(v.p.text))
                .on_edit(|s: &mut State| &mut s.library_filter),
        );
    }
    let filter = v.s.library_filter.text.to_lowercase();
    let mut order: Vec<_> = app
        .albums
        .iter()
        .enumerate()
        .filter(|(_, a)| {
            filter.is_empty()
                || a.name.to_lowercase().contains(&filter)
                || a.artist.to_lowercase().contains(&filter)
        })
        .map(|(i, _)| i)
        .collect();
    use crate::settings::{LibraryShelf, LibrarySort};
    match app
        .presentation
        .settings
        .library_sort
        .get(&LibraryShelf::Albums)
    {
        Some(LibrarySort::Name) => order
            .sort_by_cached_key(|&i| (app.albums[i].name.to_lowercase(), app.albums[i].id.clone())),
        Some(LibrarySort::RecentlyPlayed) => order.sort_by_cached_key(|&i| {
            let uri = format!("navidrome:album:{}", navidrome::encode(&app.albums[i].id));
            app.presentation
                .recent_contexts
                .iter()
                .position(|key| key == &uri)
                .unwrap_or(usize::MAX)
        }),
        _ => {}
    }
    let grid = app.presentation.settings.sidebar_grid;
    let compact = app.presentation.settings.sidebar_compact;
    let layout = crate::ui::sidebar::grid_layout(width - 24.);
    let columns = if grid { layout.columns } else { 1 };
    let height = if grid {
        layout.row_height
    } else if compact {
        34.
    } else {
        60.
    };
    let edge = (layout.card_width - 14.).max(1.);
    let p = v.p;
    let scale = v.scale;
    let zoom = v.zoom;
    let order = std::rc::Rc::new(order);
    let keys = order.clone();
    let list = col()
        .key("sidebar-albums")
        .grow(1.)
        .min_h(0.)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(col().growth(rune::GrowthSpec::default()).virtual_rows(
            order.len().div_ceil(columns),
            height,
            move |s: &State, i| Cow::Owned(format!("shelf-{}", s.app.albums[keys[i * columns]].id)),
            move |s: &State, r| {
                let v = Paint {
                    s,
                    p,
                    scale,
                    zoom,
                    width,
                    height: 0.,
                };
                let mut band = row().w_full().h(height);
                for &i in order.iter().skip(r * columns).take(columns) {
                    let a = &s.app.albums[i];
                    let id = navidrome::encode(&a.id);
                    let playing = s.app.playing_album.as_ref().is_some_and(|p| p.id == a.id);
                    let name = label(&a.name, 14., 550., if playing { p.accent } else { p.text })
                        .truncate();
                    let by = label(
                        format!("{} • {}", v.tr("Album"), a.artist),
                        12.5,
                        400.,
                        p.secondary,
                    )
                    .truncate();
                    let mut item = if grid {
                        col()
                            .w(layout.card_width)
                            .padding(6.)
                            .gap(3.)
                            .child(v.cover(a.cover_art.as_deref(), CoverSize::Sleeve, edge, 6.))
                            .child(name)
                            .child(by)
                    } else {
                        let mut line = row().w_full().px(8.).items_center().gap(12.);
                        if !compact {
                            line = line.child(v.cover(
                                a.cover_art.as_deref(),
                                CoverSize::Sleeve,
                                44.,
                                6.,
                            ));
                        }
                        let mut titles = col().min_w(0.).grow(1.).gap(3.).child(name);
                        if !compact {
                            titles = titles.child(by);
                        }
                        line = line.child(titles);
                        if playing {
                            line = line.child(v.icon(Icon::Volume2, 16., p.accent));
                        }
                        line
                    };
                    if s.app.presentation.page() == &Page::Album(id.clone()) {
                        item = item.fill(color(p.surface));
                    }
                    band = band.child(
                        item.h_full()
                            .radius(6.)
                            .role("button")
                            .label(&a.name)
                            .focusable(true)
                            .hover(color(p.surface_hover))
                            .on_press_tx(move |s: &mut State, tx| {
                                s.dispatch(Action::Open(Page::Album(id.clone())), tx)
                            }),
                    );
                }
                band
            },
        ));
    side.child(list)
}
fn heading(v: &Paint<'_>, text: &str) -> El {
    label(text, 28., 700., v.p.text).pb(18.)
}
pub(super) fn favorite_context(tile: El, kind: FavoriteKind, id: &str) -> El {
    let uri = kind.uri(id);
    tile.on_secondary_press(move |s: &mut State, p, _| {
        if s.app
            .presentation
            .navidrome_favorites
            .contains(&uri)
            .is_some()
        {
            let zoom = s.app.presentation.settings.zoom;
            s.favorite_menu = Some((uri.clone(), [p.x / zoom, p.y / zoom]));
        }
    })
}
pub(super) fn credits(v: &Paint<'_>, name: &str, by: &crate::navidrome::api::ArtistCredits) -> El {
    if by.artists.iter().all(|a| a.name.trim().is_empty()) {
        return credit(v, name, by.artist_id.as_deref());
    }
    let mut names = row().min_w(0.).items_center().gap(3.);
    for (i, a) in by
        .artists
        .iter()
        .filter(|a| !a.name.trim().is_empty())
        .enumerate()
    {
        if i > 0 {
            names = names.child(label(",", 12.5, 400., v.p.secondary));
        }
        names = names.child(credit(v, &a.name, a.id.as_deref()));
    }
    names
}
fn album_card(v: &Paint<'_>, a: &Album) -> El {
    let page = Page::Album(navidrome::encode(&a.id));
    let tile = col()
        .key(format!("album-{}", a.id))
        .w(172.)
        .padding(12.)
        .gap(2.)
        .radius(8.)
        .hover(color(v.p.surface_hover))
        .role("button")
        .focusable(true)
        .label(format!("{}, {}", a.name, a.artist))
        .child(v.cover(a.cover_art.as_deref(), CoverSize::Sleeve, 148., 6.))
        .child(label(&a.name, 14., 600., v.p.text).mt(8.).truncate())
        .child(label(&a.artist, 12.5, 400., v.p.secondary).wrap())
        .on_press_tx(move |s: &mut State, tx| s.dispatch(Action::Open(page.clone()), tx));
    favorite_context(tile, FavoriteKind::Album, &a.id)
}
fn albums(v: &Paint<'_>, width: f32) -> El {
    let a = &v.s.app;
    let n = crate::ui::widgets::card_columns((width - 48.).max(1.));
    let p = v.p;
    let scale = v.scale;
    let zoom = v.zoom;
    let rows = a.albums.len().div_ceil(n);
    let bounds = (v.width, v.height);
    col()
        .w_full()
        .growth(rune::GrowthSpec::default())
        .virtual_rows(
            rows + 2,
            240.,
            move |_: &State, r| {
                if r == 0 {
                    "albums-heading".into()
                } else if r == rows + 1 {
                    "albums-status".into()
                } else {
                    Cow::Owned(format!("albums-{r}"))
                }
            },
            move |s: &State, r| {
                let v = Paint {
                    s,
                    p,
                    scale,
                    zoom,
                    width: bounds.0,
                    height: bounds.1,
                };
                if r == 0 {
                    return heading(&v, &v.tr("Albums"));
                }
                if r == rows + 1 {
                    return album_status(&v);
                }
                let mut band = row().h(240.).gap(7.);
                for album in s.app.albums.iter().skip((r - 1) * n).take(n) {
                    band = band.child(album_card(&v, album));
                }
                band
            },
        )
}
fn album_status(v: &Paint<'_>) -> El {
    let a = &v.s.app;
    let mut page = col();
    if let Some(e) = &a.error {
        page = page.child(label(e, 14., 400., v.p.danger)).child(v.button(
            &v.tr("Retry"),
            false,
            |s, _| s.app.apply(Service::Retry),
        ));
    } else if a.list_loading || a.restoring_login {
        page = page.child(label(v.tr("Loading…"), 14., 400., v.p.secondary));
    } else if a.albums.is_empty() {
        page = page.child(heading(v, &v.tr("No albums"))).child(label(
            v.tr("Albums in your Navidrome library appear here."),
            14.,
            400.,
            v.p.secondary,
        ));
    } else if a.has_more {
        page = page.child(v.button(&v.tr("Load more"), false, |s, _| s.app.apply(Service::More)));
    }
    page
}
fn album(v: &Paint<'_>) -> El {
    let app = &v.s.app;
    let Some(a) = app.album.as_ref() else {
        return if let Some(e) = &app.error {
            col().child(label(e, 14., 400., v.p.danger)).child(v.button(
                &v.tr("Retry"),
                false,
                |s, tx| s.dispatch(Action::Reload(s.app.presentation.page().clone()), tx),
            ))
        } else {
            label(v.tr("Loading…"), 14., 400., v.p.secondary)
        };
    };
    let width = v.content_width();
    let edge = if width > 720. { 212. } else { 160. };
    let mut byline = row().items_center().flex_wrap().gap(4.).child(credit(
        v,
        &a.artist,
        a.credits.artist_id.as_deref(),
    ));
    if a.year > 0 {
        byline = byline.child(label(format!("• {}", a.year), 13.5, 400., v.p.secondary));
    }
    let length = a.song.iter().map(|t| (t.duration * 1000.) as u64).sum();
    let details =
        crate::ui::collection::songs_and_duration(app.presentation.locale, a.song_count, length);
    let release = col()
        .gap(6.)
        .grow(1.)
        .min_w(0.)
        .pt(edge * 0.08)
        .child(label(v.tr("Album"), 12.5, 500., v.p.text))
        .child(label(&a.name, if edge > 200. { 56. } else { 40. }, 700., v.p.text).wrap())
        .child(byline.child(label(format!("• {details}"), 13.5, 400., v.p.secondary)));
    let active = app
        .playing_album
        .as_ref()
        .is_some_and(|playing| playing.id == a.id)
        && !app.transport.paused;
    let mut page = col()
        .w_full()
        .gap(12.)
        .child(
            row()
                .w_full()
                .gap(24.)
                .child(v.cover(a.cover_art.as_deref(), CoverSize::Sleeve, edge, 6.))
                .child(release),
        )
        .child(
            row()
                .mt(14.)
                .mb(10.)
                .items_center()
                .gap(12.)
                .child(
                    el().size(56., 56.)
                        .center()
                        .radius(28.)
                        .fill(color(v.p.accent))
                        .hover(color(v.p.accent_hover))
                        .role("button")
                        .focusable(true)
                        .label(v.tr(if active { "Pause" } else { "Play" }))
                        .child(v.icon(
                            if active {
                                Icon::PauseFilled
                            } else {
                                Icon::PlayFilled
                            },
                            28.,
                            v.p.on_accent,
                        ))
                        .on_press(move |s: &mut State| {
                            s.app.apply(if active {
                                Service::Toggle
                            } else {
                                Service::Play(0)
                            })
                        }),
                )
                .child(
                    v.icon_button(&v.tr("Shuffle"), Icon::Shuffle, 24., |_, _| {})
                        .disabled(true),
                )
                .child(star(v, FavoriteKind::Album, &a.id))
                .child(
                    v.icon_button(&v.tr("More"), Icon::Ellipsis, 22., |s, _| {
                        s.menu = Some("album")
                    })
                    .key("album-more"),
                ),
        );
    if a.song.is_empty() {
        page = page.child(label(v.tr("No tracks"), 14., 400., v.p.secondary));
    }
    page
}
pub(super) fn credit(v: &Paint<'_>, name: &str, id: Option<&str>) -> El {
    let title = label(name, 14., 500., v.p.secondary).truncate();
    if let Some(id) = id {
        let page = Page::Artist(navidrome::encode(id));
        title
            .role("button")
            .focusable(true)
            .label(name)
            .on_press_tx(move |s: &mut State, tx| s.dispatch(Action::Open(page.clone()), tx))
    } else {
        title
    }
}
pub(super) fn star(v: &Paint<'_>, kind: FavoriteKind, id: &str) -> El {
    let uri = kind.uri(id);
    let saved = v.s.app.presentation.navidrome_favorites.contains(&uri);
    v.tinted_button(
        &v.tr(if saved == Some(true) {
            "Remove from Favorites"
        } else {
            "Add to Favorites"
        }),
        if saved == Some(true) {
            Icon::StarFilled
        } else {
            Icon::Star
        },
        18.,
        if saved == Some(true) {
            v.p.accent
        } else {
            v.p.secondary
        },
        move |s, tx| s.dispatch(Action::ToggleSaved(uri.clone()), tx),
    )
    .disabled(saved.is_none())
}
fn list_songs(s: &State) -> &[Song] {
    s.app.page_songs().unwrap_or_default()
}
fn open_song(s: &mut State, i: usize, tx: &mut Tx) {
    if matches!(s.app.presentation.page(), Page::Playlist(_)) {
        s.dispatch(Action::NavidromeSearch(Search::PlayEntry(i)), tx);
    } else {
        s.play_album_row(i);
    }
}
fn album_columns(v: &Paint<'_>) -> El {
    use crate::model::SortColumn;
    let sort = v.s.album_sort();
    let title = crate::i18n::pgettext(v.s.app.presentation.locale, "column heading", "TITLE");
    let button = |column, name: String, width, text: &str| {
        let active = sort
            .filter(|s| s.column == column)
            .map(|s| s.ascending)
            .or_else(|| (sort.is_none() && column == SortColumn::Index).then_some(true));
        let ink = if active.is_some() {
            v.p.accent
        } else {
            v.p.secondary
        };
        let content = if column == SortColumn::Duration {
            v.icon(Icon::Clock, 15., ink)
        } else {
            label(text, 12., 400., ink)
        };
        let mut head = row()
            .key(format!("sort-{column:?}"))
            .h(32.)
            .w(width)
            .items_center()
            .gap(2.)
            .role("button")
            .focusable(true)
            .label(name)
            .child(content)
            .on_press_tx(move |s: &mut State, tx| {
                s.sort_album(column);
                tx.instant();
            });
        if let Some(ascending) = active {
            head = head.child(v.icon(
                if ascending {
                    Icon::ChevronUp
                } else {
                    Icon::ChevronDown
                },
                10.,
                ink,
            ));
        }
        head
    };
    col()
        .w_full()
        .child(
            row()
                .w_full()
                .h(33.)
                .pl(22.)
                .pr(40.)
                .items_center()
                .child(button(
                    SortColumn::Index,
                    v.tr("Sort by playlist order"),
                    30.,
                    "#",
                ))
                .child(button(
                    SortColumn::Title,
                    v.tr("Sort by {column}").replace("{column}", &title),
                    88.,
                    &title,
                ))
                .child(el().grow(1.))
                .child(button(
                    SortColumn::Duration,
                    v.tr("Sort by duration"),
                    38.,
                    "",
                )),
        )
        .child(el().w_full().h(1.).fill(color(v.p.outline)))
}
fn songs(v: &Paint<'_>) -> El {
    let p = v.p;
    let zoom = v.zoom;
    let scale = v.scale;
    let h = if v.s.app.presentation.settings.tracklist_compact {
        36.
    } else {
        56.
    };
    let bounds = (v.width, v.height);
    let mut grid = col()
        .key("tracks")
        .role("grid")
        .label(v.tr("Tracks"))
        .w_full()
        .focusable(true)
        .growth(rune::GrowthSpec::default())
        .on_key(|s: &mut State, k, tx| track_key(s, k, tx));
    if let Some(key) = v.s.table.cursor() {
        grid = grid.active_descendant(key);
    }
    let context_playing = match v.s.app.presentation.page() {
        Page::Playlist(id) => v.s.app.playing_playlist.as_ref() == Some(id),
        Page::Album(id) => {
            v.s.app
                .playing_album
                .as_ref()
                .is_some_and(|a| navidrome::encode(&a.id) == *id)
        }
        _ => false,
    };
    let current_key = context_playing.then(|| v.s.playing_row()).flatten();
    grid.virtual_rows(
        list_songs(v.s).len() + 1,
        h,
        |s: &State, r| {
            if r == 0 {
                "tracks-heading".into()
            } else {
                Cow::Borrowed(s.table.key(r - 1))
            }
        },
        move |s: &State, r| {
            let v = Paint {
                s,
                p,
                scale,
                zoom,
                width: bounds.0,
                height: bounds.1,
            };
            if r == 0 {
                return col().w_full().child(album(&v)).child(album_columns(&v));
            }
            let i = r - 1;
            let source = s.table.source(i);
            let song = &list_songs(s)[source];
            let key = s.table.key(i).to_owned();
            let playing = current_key.as_ref() == Some(&key);
            let selected = s.table.selection.rows.contains(&i);
            let line = row()
                .key(key.clone())
                .w_full()
                .h(h)
                .items_center()
                .gap(12.)
                .px(8.)
                .radius(6.)
                .role("row")
                .focus_selected(selected)
                .label(format!("{} {} {}", source + 1, song.title, song.artist))
                .fill(if selected {
                    color(p.surface_active)
                } else {
                    Color::TRANSPARENT
                })
                .hover(color(p.surface_hover))
                .child(
                    el().w(28.)
                        .center()
                        .child(if playing && !s.app.transport.paused {
                            v.icon(Icon::AudioLines, 17., p.accent)
                        } else {
                            label(
                                (source + 1).to_string(),
                                14.,
                                400.,
                                if playing { p.accent } else { p.secondary },
                            )
                        }),
                );
            line.child(
                col()
                    .grow(1.)
                    .min_w(0.)
                    .child(
                        label(
                            &song.title,
                            14.,
                            550.,
                            if playing { p.accent } else { p.text },
                        )
                        .truncate(),
                    )
                    .child(credits(&v, &song.artist, &song.credits)),
            )
            .child(star(&v, FavoriteKind::Song, &song.id))
            .child(label(
                crate::util::format_duration_ms((song.duration * 1000.) as u32),
                13.,
                400.,
                p.secondary,
            ))
            .on_press_tx({
                let key = key.clone();
                move |s: &mut State, tx| {
                    if let Some(i) = s.table.index(&key) {
                        let pick = if tx.mods().shift {
                            crate::model::RowPick::Range
                        } else if tx.mods().cmd {
                            crate::model::RowPick::Toggle
                        } else {
                            crate::model::RowPick::Only
                        };
                        s.table.pick(i, pick);
                    }
                }
            })
            .on_double_press_tx(move |s: &mut State, tx| {
                if let Some(i) = s.table.index(&key) {
                    open_song(s, i, tx)
                }
            })
        },
    )
}
pub(super) fn track_key(s: &mut State, k: &rune::KeyEvent, tx: &mut Tx) -> bool {
    let count = list_songs(s).len();
    if count == 0 {
        return false;
    }
    if k.is_command('a') {
        s.table.selection.rows = (0..count).collect();
        s.table.selection.anchor = Some(0);
        s.table.cursor = Some(count - 1);
        return true;
    }
    if k.mods.cmd || k.mods.alt {
        return false;
    }
    let index = s.table.cursor;
    let next = match k.key {
        rune::Key::Enter => {
            if let Some(i) = index {
                open_song(s, i, tx);
            }
            return index.is_some();
        }
        rune::Key::Down => index.map_or(0, |i| (i + 1).min(count - 1)),
        rune::Key::Up => index.unwrap_or(0).saturating_sub(1),
        rune::Key::Home => 0,
        rune::Key::End => count - 1,
        _ => return false,
    };
    if !k.mods.shift {
        s.table.selection.rows.clear();
    }
    s.table.pick(
        next,
        if k.mods.shift {
            crate::model::RowPick::Range
        } else {
            crate::model::RowPick::Only
        },
    );
    tx.scroll_into_view(s.table.key(next));
    true
}
fn failed<T>(state: &Loadable<T>) -> Option<&str> {
    if let Loadable::Failed(e) = state {
        Some(e)
    } else {
        None
    }
}
fn catalogue_button(
    v: &Paint<'_>,
    name: &str,
    icon: Icon,
    artist: Option<(&str, &crate::navidrome::api::ArtistCredits)>,
    selected: bool,
) -> El {
    let mut hit_row = row()
        .grow(1.)
        .min_w(0.)
        .h(32.)
        .gap(6.)
        .px(6.)
        .items_center()
        .radius(5.)
        .role("button")
        .focusable(true)
        .label(name)
        .focus_selected(selected)
        .fill(if selected {
            color(v.p.surface_hover)
        } else {
            Color::TRANSPARENT
        })
        .hover(color(v.p.surface_hover))
        .child(v.icon(icon, 16., if selected { v.p.accent } else { v.p.secondary }))
        .child(label(name, 14., 500., v.p.text).truncate());
    if let Some((by, details)) = artist
        && !by.is_empty()
    {
        hit_row = hit_row
            .child(label("·", 13., 400., v.p.dim))
            .child(credits(v, by, details));
    }
    hit_row
}
fn playlist(v: &Paint<'_>) -> El {
    let Some(Collection::Playlist { name, songs }) =
        v.s.app.presentation.navidrome_search.collection.get()
    else {
        return collection(v);
    };
    let title = name.clone();
    let empty = songs.is_empty();
    let (p, scale, zoom, width, height) = (v.p, v.scale, v.zoom, v.width, v.height);
    col()
        .w_full()
        .key("tracks")
        .role("list")
        .label(v.tr("Songs"))
        .focusable(true)
        .on_key(|s: &mut State, k, tx| track_key(s, k, tx))
        .growth(rune::GrowthSpec::default())
        .virtual_rows(
            1 + songs.len(),
            36.,
            |s: &State, r| {
                if r == 0 {
                    "playlist-heading".into()
                } else {
                    Cow::Borrowed(s.table.key(r - 1))
                }
            },
            move |s: &State, r| {
                let v = Paint {
                    s,
                    p,
                    scale,
                    zoom,
                    width,
                    height,
                };
                if r == 0 {
                    let mut head = col()
                        .w_full()
                        .gap(6.)
                        .pb(4.)
                        .child(heading(&v, &title))
                        .child(label(v.tr("Songs"), 14., 400., p.secondary));
                    if empty {
                        head = head.child(label(v.tr("Nothing here yet."), 14., 400., p.secondary));
                    }
                    return head;
                }
                let i = r - 1;
                let song = &list_songs(s)[i];
                let key = s.table.key(i).to_owned();
                let button = catalogue_button(
                    &v,
                    &song.title,
                    Icon::Music,
                    Some((&song.artist, &song.credits)),
                    s.table.cursor == Some(i),
                )
                .key(format!("play-{key}"))
                .on_press_tx(move |s: &mut State, tx| {
                    if let Some(i) = s.table.index(&key) {
                        s.table.pick(i, crate::model::RowPick::Only);
                        open_song(s, i, tx);
                    }
                });
                row()
                    .w_full()
                    .h(36.)
                    .items_center()
                    .gap(6.)
                    .child(button)
                    .child(star(&v, FavoriteKind::Song, &song.id))
            },
        )
}
fn search(v: &Paint<'_>) -> El {
    let s = &v.s.app.presentation.navidrome_search;
    let mut list = col().w_full().gap(2.);
    if v.s.query.text.trim().is_empty() {
        if s.recent.is_empty() {
            return col()
                .w_full()
                .items_center()
                .gap(8.)
                .pt(48.)
                .child(v.icon(Icon::Search, 40., v.p.secondary))
                .child(heading(v, &v.tr("Search")))
                .child(label(
                    v.tr("What do you want to play?"),
                    14.,
                    400.,
                    v.p.secondary,
                ));
        }
        list = col()
            .w_full()
            .child(label(v.tr("Recent searches"), 18., 650., v.p.text));
        let mut terms = row().flex_wrap().gap(8.);
        for term in &s.recent {
            let query = term.clone();
            let forget = term.clone();
            terms = terms.child(
                row()
                    .items_center()
                    .child(v.button(term, false, move |s, tx| {
                        s.query.replace_text(&query);
                        s.dispatch(Action::Search(query.clone()), tx);
                    }))
                    .child(v.icon_button(&v.tr("Remove"), Icon::X, 14., move |s, tx| {
                        s.dispatch(Action::ForgetSearch(forget.clone()), tx)
                    })),
            );
        }
        return list.child(terms);
    }
    let mut previous = None;
    for (hit, name, artist) in s.rows() {
        use crate::app::navidrome::search::Hit;
        let section = match hit {
            Hit::Song(_) => "Songs",
            Hit::Album(_) => "Albums",
            Hit::Artist(_) => "Artists",
            Hit::Playlist(_) => "Playlists",
        };
        if previous != Some(section) {
            list = list.child(label(v.tr(section), 18., 650., v.p.text).mt(10.));
            previous = Some(section);
        }
        let icon = match hit {
            Hit::Song(_) => Icon::Music,
            Hit::Album(_) => Icon::Disc,
            Hit::Artist(_) => Icon::User,
            Hit::Playlist(_) => Icon::ListMusic,
        };
        let selected = s.selected == Some(hit);
        let hit_row = catalogue_button(v, name, icon, artist, selected).key(format!("hit-{hit:?}"));
        let mut line = row()
            .w_full()
            .items_center()
            .gap(6.)
            .child(hit_row.on_press_tx(move |s: &mut State, tx| {
                s.dispatch(Action::NavidromeSearch(Search::Activate(hit)), tx)
            }));
        if let Some((kind, id)) = s.catalogue.get().and_then(|c| match hit {
            Hit::Song(i) => Some((FavoriteKind::Song, &c.song[i].id)),
            Hit::Album(i) => Some((FavoriteKind::Album, &c.album[i].id)),
            Hit::Artist(i) => Some((FavoriteKind::Artist, &c.artist[i].id)),
            Hit::Playlist(_) => None,
        }) {
            line = line.child(star(v, kind, id));
        }
        list = list.child(line);
    }
    if s.catalogue.get().is_some() && s.playlists.get().is_some() && previous.is_none() {
        list = list
            .child(heading(
                v,
                &v.tr("No results for “{query}”")
                    .replace("{query}", &v.s.app.presentation.search.committed),
            ))
            .child(label(
                v.tr("Check the spelling, or try fewer words."),
                14.,
                400.,
                v.p.secondary,
            ));
    }
    if v.s.app.presentation.search.typed_at.is_some()
        || s.catalogue.is_loading()
        || s.playlists.is_loading()
    {
        list = list.child(label(v.tr("Searching…"), 14., 400., v.p.secondary));
    }
    for (part, error) in [
        (SearchPart::Catalogue, failed(&s.catalogue)),
        (SearchPart::Playlists, failed(&s.playlists)),
    ] {
        if let Some(error) = error {
            list = list
                .child(label(error, 14., 400., v.p.danger))
                .child(v.button(&v.tr("Retry"), false, move |s, tx| {
                    s.dispatch(Action::NavidromeSearch(Search::Retry(part)), tx)
                }));
        }
    }
    list
}
fn collection(v: &Paint<'_>) -> El {
    let state = &v.s.app.presentation.navidrome_search;
    match &state.collection {
        Loadable::Loaded(Collection::Playlist { name, .. }) => col().child(heading(v, name)),
        Loadable::Loaded(Collection::Artist {
            name,
            albums,
            cover_art,
        }) => artist::show(v, name, albums, cover_art.as_deref()),
        Loadable::Failed(e) => col().child(label(e, 14., 400., v.p.danger)).child(v.button(
            &v.tr("Retry"),
            false,
            |s, tx| s.dispatch(Action::Reload(s.app.presentation.page().clone()), tx),
        )),
        _ => label(v.tr("Loading…"), 14., 400., v.p.secondary),
    }
}
fn favorites(v: &Paint<'_>) -> El {
    let state = &v.s.app.presentation.navidrome_favorites;
    let group = match v.s.app.presentation.page() {
        Page::FavoriteGroup(k) => Some(*k),
        _ => None,
    };
    let mut page = favorites_header(v, group);
    if !state.ready {
        return page;
    }
    for (kind, title, empty, count) in [
        (
            FavoriteKind::Song,
            "Songs",
            "Songs you favorite appear here.",
            state.list.song.len(),
        ),
        (
            FavoriteKind::Album,
            "Albums",
            "Albums you favorite appear here.",
            state.list.album.len(),
        ),
        (
            FavoriteKind::Artist,
            "Artists",
            "Artists you favorite appear here.",
            state.list.artist.len(),
        ),
    ] {
        if group.is_some_and(|k| k != kind) {
            continue;
        }
        if group.is_none() {
            let mut head = row()
                .items_center()
                .child(label(v.tr(title), 18., 700., v.p.text).grow(1.));
            if count > 0 {
                head = head.child(
                    v.button(&v.tr("See all"), false, move |s, tx| {
                        s.dispatch(Action::Open(Page::FavoriteGroup(kind)), tx)
                    })
                    .px(5.)
                    .fill(Color::TRANSPARENT),
                );
            }
            page = page.child(head);
        }
        if count == 0 {
            page = page.child(label(v.tr(empty), 14., 400., v.p.secondary));
            continue;
        }
        let cols = crate::ui::widgets::card_columns(v.content_width());
        if group.is_some() {
            let bounds = (v.width, v.height);
            let (p, scale, zoom) = (v.p, v.scale, v.zoom);
            // Rune needs the virtual stack directly below its viewport. Include the
            // heading as its first row so it scrolls with the complete group.
            return col()
                .w_full()
                .growth(rune::GrowthSpec::default())
                .virtual_rows(
                    1 + count.div_ceil(cols),
                    240.,
                    move |_: &State, r| Cow::Owned(format!("favorite-{kind:?}-row-{r}")),
                    move |s: &State, r| {
                        let v = Paint {
                            s,
                            p,
                            scale,
                            zoom,
                            width: bounds.0,
                            height: bounds.1,
                        };
                        if r == 0 {
                            favorites_header(&v, group).pb(14.)
                        } else {
                            favorite_row(&v, kind, (r - 1) * cols, cols, count).h(240.)
                        }
                    },
                );
        }
        let limit = count.min(2 * cols);
        let grid = col().gap(10.).children(
            (0..limit)
                .step_by(cols)
                .map(|start| favorite_row(v, kind, start, cols, limit)),
        );
        page = page.child(grid);
    }
    page
}

fn favorites_header(v: &Paint<'_>, group: Option<FavoriteKind>) -> El {
    let state = &v.s.app.presentation.navidrome_favorites;
    let title = match group {
        None => "Favorites",
        Some(FavoriteKind::Song) => "Favorite Songs",
        Some(FavoriteKind::Album) => "Favorite Albums",
        Some(FavoriteKind::Artist) => "Favorite Artists",
    };
    let mut page = col().gap(14.).child(
        row()
            .items_center()
            .gap(4.)
            .child(label(v.tr(title), 28., 700., v.p.text))
            .child(
                v.icon_button(&v.tr("Refresh"), Icon::Refresh, 16., |s, tx| {
                    s.dispatch(Action::Reload(s.app.presentation.page().clone()), tx)
                })
                .disabled(state.loading),
            ),
    );
    if state.loading {
        page = page.child(label(v.tr("Loading…"), 14., 400., v.p.secondary));
    }
    if let Some(e) = &state.error {
        page = page.child(label(e, 14., 400., v.p.danger)).child(v.button(
            &v.tr("Retry"),
            false,
            |s, tx| s.dispatch(Action::Reload(Page::LikedSongs), tx),
        ));
    }
    for (uri, fail) in &state.failures {
        let uri = uri.clone();
        page = page.child(
            row()
                .child(label(&fail.message, 14., 400., v.p.danger))
                .child(v.button(&v.tr("Retry"), false, move |s, tx| {
                    s.dispatch(Action::RetryFavorite(uri.clone()), tx)
                })),
        );
    }
    page
}

fn favorite_row(v: &Paint<'_>, kind: FavoriteKind, start: usize, cols: usize, limit: usize) -> El {
    let state = &v.s.app.presentation.navidrome_favorites;
    let mut band = row().w_full().gap(crate::ui::widgets::CARD_GAP / 2.);
    for i in start..(start + cols).min(limit) {
        let (id, name, cover, by) = match kind {
            FavoriteKind::Song => {
                let a = &state.list.song[i];
                (&a.id, &a.title, &a.cover_art, Some((&a.artist, &a.credits)))
            }
            FavoriteKind::Album => {
                let a = &state.list.album[i];
                (&a.id, &a.name, &a.cover_art, Some((&a.artist, &a.credits)))
            }
            FavoriteKind::Artist => {
                let a = &state.list.artist[i];
                (&a.id, &a.name, &a.cover_art, None)
            }
        };
        let action = match kind {
            FavoriteKind::Song => Action::PlayFavorite(id.clone()),
            FavoriteKind::Album => Action::Open(Page::Album(navidrome::encode(id))),
            FavoriteKind::Artist => Action::Open(Page::Artist(navidrome::encode(id))),
        };
        let cover = v.cover(
            cover.as_deref(),
            CoverSize::Sleeve,
            148.,
            if kind == FavoriteKind::Artist {
                74.
            } else {
                6.
            },
        );
        let art = el().size(148., 148.).child(cover).child(
            star(v, kind, id)
                .size(26., 26.)
                .absolute(118., 4.)
                .radius(13.)
                .fill(color(v.p.panel)),
        );
        let subtitle = by.map_or_else(
            || label(v.tr("Artist"), 12.5, 400., v.p.secondary),
            |(name, by)| credits(v, name, by),
        );
        let tile = col()
            .w(172.)
            .px(12.)
            .pt(12.)
            .pb(8.)
            .radius(8.)
            .role("button")
            .focusable(true)
            .label(name)
            .hover(color(v.p.surface_hover))
            .child(art)
            .child(label(name, 14., 600., v.p.text).mt(10.).truncate())
            .child(subtitle)
            .on_press_tx(move |s: &mut State, tx| s.dispatch(action.clone(), tx));
        band = band.child(favorite_context(tile, kind, id));
    }
    band
}
