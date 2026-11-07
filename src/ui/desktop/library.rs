use super::*;
use crate::app::navidrome::desktop::{
    ItemMenu,
    autoscroll::{self, Pane},
};
use crate::app::navidrome::{Action as Service, search::Action as Search};
use crate::i18n::mark;
use crate::model::Loadable;
use crate::navidrome::api::{Album, Collection, FavoriteKind, SearchPart, Song};
use std::borrow::Cow;

pub(super) fn shell(v: &Paint<'_>) -> El {
    let mut body = row().w_full().grow(1.).min_h(0.);
    let s = v.s;
    let queue_width = v.queue_width();
    s.queue_compact.set(queue_width == 0.);
    let queue_page = s.queue_open && queue_width == 0.;
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
        Page::Album(_) | Page::Playlist(_) => songs(v),
        Page::Playlists => playlists(v),
        Page::Search => search(v),
        Page::Artist(_) => collection(v),
        Page::LikedSongs | Page::FavoriteGroup(_) => favorites(v),
        Page::Settings => preferences::settings(v),
        _ => albums(v, v.width - side - queue_width),
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
            Page::Artist(_) | Page::LikedSongs | Page::FavoriteGroup(_) => 8.,
            Page::Search => 10.,
            Page::Settings => 16.,
            _ => 22.,
        })
        .pb(48.)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(content);
    let scroll_enabled =
        crate::autoscroll::enabled(s.app.presentation.settings.middle_click_autoscroll);
    scroller = autoscroll::pane(scroller, Pane::Page, scroll_enabled);
    let shadow = rune::Live::new(0.);
    if let Some(offset) = s.offsets.get(page).cloned() {
        let opacity = shadow.clone();
        let depth = |y: f32| (y / 24.).clamp(0., 1.);
        opacity.set(depth(offset.get()));
        let viewport = s.autoscroll.viewport(Pane::Page);
        scroller = scroller.on_scroll_offset(move |pos| {
            viewport.set(pos);
            offset.set(pos.offset[1]);
            opacity.set(depth(pos.offset[1]));
        });
    }
    let mut center = col().min_w(0.).grow(1.).h_full();
    if let Some(tint) = page_tint(v, page) {
        let strength = if matches!(page, Page::Home | Page::Search | Page::Settings) {
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
    let mut center = center.child(topbar(v));
    if !s.app.presentation.navidrome_favorites.failures.is_empty() {
        center = center.child(favorite_write_errors(v));
    }
    let center = if queue_page {
        center.child(queue::panel(v, false).grow(1.))
    } else {
        center.child(scroller)
    };
    let center = center.child(
        el().absolute(0., crate::theme::TOP_BAR_HEIGHT + v.s.titlebar_inset())
            .w_full()
            .h(14.)
            .live_opacity(&shadow)
            .gradient(
                color(Srgba::from_black_alpha(if v.p.dark { 110 } else { 36 })),
                Color::TRANSPARENT,
            )
            .a11y_hidden(),
    );
    // The analyser changes every frame. Let Rune reuse the settled opaque
    // library surface while it draws the transport, until the page itself changes.
    let mut body = body.child(center).fill(color(v.p.window));
    if queue_width > 0. {
        body = body.child(queue::panel(v, true).w(queue_width));
    }
    let body = if !s.app.transport.paused
        && s.app.presentation.settings.player_bar_vis == crate::settings::PlayerBarVis::Spectrum
    {
        body.cache()
    } else {
        body
    };
    let mut shell = col()
        .w_full()
        .h_full()
        .fill(color(v.p.window))
        .child(body)
        .child(transport::player(v));
    if !s.app.presentation.toasts.is_empty() {
        let mut notices = col().items_end().gap(8.).pin(
            None,
            None,
            Some(20.),
            Some(crate::theme::PLAYER_BAR_HEIGHT + 16.),
        );
        for toast in &s.app.presentation.toasts {
            let (icon, ink) = match toast.kind {
                crate::model::ToastKind::Info => (Icon::CircleCheck, v.p.accent),
                crate::model::ToastKind::Error => (Icon::CircleAlert, v.p.danger),
            };
            let age = toast.created.elapsed().as_secs_f32();
            let alpha = (age / 0.15).min((3.2 - age) / 0.4).clamp(0., 1.);
            notices = notices.child(
                row()
                    .px(14.)
                    .py(10.)
                    .gap(8.)
                    .items_center()
                    .role("status")
                    .radius(f32::from(crate::theme::RADIUS))
                    .fill(color(v.p.overlay))
                    .border(1., color(v.p.outline))
                    .shadow(4., 16., color(v.p.shadow))
                    .opacity(alpha)
                    .child(v.icon(icon, 16., ink))
                    .child(label(&toast.message, 13.5, 500., v.p.text).max_w(280.)),
            );
        }
        shell = shell.child(notices);
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
                mark("Remove from Favorites")
            } else {
                mark("Add to Favorites")
            });
            collection_queue_items(v, v.menu("album-more"), None)
                .child(v.menu_item(&name, Some(Icon::Star), move |s, tx| {
                    s.dispatch(Action::ToggleSaved(uri.clone()), tx)
                }))
                .child(v.menu_item(
                    &v.tr(mark("Add to playlists")),
                    Some(Icon::ListMusic),
                    |s, tx| s.add_to_playlists(None, tx),
                ))
        }),
        Some("playlist") => {
            let mut menu =
                collection_queue_items(v, v.menu("playlist-more"), None).child(v.menu_item(
                    &v.tr(mark("Add to playlists")),
                    Some(Icon::ListMusic),
                    |s, tx| s.add_to_playlists(None, tx),
                ));
            if s.app.playlist_writable() {
                menu = menu.child(v.menu_item(&v.tr(mark("Delete playlist")), None, |s, tx| {
                    s.dispatch(
                        Action::EditPlaylists(navidrome::playlists::Action::ConfirmDelete),
                        tx,
                    )
                }));
            }
            Some(menu)
        }
        Some(key @ ("themes" | "languages")) => Some(preferences::choice_menu(v, key)),
        _ => None,
    } {
        shell = shell.child(menu);
    }
    if let Some(menu) = &s.favorite_menu {
        let uri = menu.uri.clone();
        let mut popup = col()
            .w(232.)
            .padding(6.)
            .gap(2.)
            .radius(8.)
            .fill(color(v.p.overlay))
            .border(1., color(v.p.outline))
            .role("menu")
            .modal(true, |s: &mut State| s.favorite_menu = None);
        if let Some(saved) = s.app.presentation.navidrome_favorites.contains(&uri) {
            let name = v.tr(if saved {
                mark("Remove from Favorites")
            } else {
                mark("Add to Favorites")
            });
            popup = popup.child(v.menu_item(
                &name,
                Some(if saved { Icon::StarFilled } else { Icon::Star }),
                move |s, tx| {
                    s.dispatch(Action::ToggleSaved(uri.clone()), tx);
                },
            ));
        }
        if let Some(key) = menu.row.clone() {
            popup = collection_queue_items(v, popup, Some(key.clone()));
            let add_key = key.clone();
            popup = popup.child(
                v.menu_item(
                    &v.tr(mark("Add to playlists")),
                    Some(Icon::ListMusic),
                    move |s, tx| s.add_to_playlists(Some(&add_key), tx),
                )
                .key("playlist-add-selection"),
            );
            if s.app.playlist_writable() && matches!(s.app.presentation.page(), Page::Playlist(_)) {
                let remove_key = key.clone();
                popup = popup.child(
                    v.menu_item(&v.tr(mark("Remove from playlist")), None, move |s, tx| {
                        s.remove_playlist_selection(&remove_key, tx)
                    })
                    .key("playlist-remove-selection"),
                );
            }
            popup = popup.child(
                v.menu_item(&v.tr(mark("Play")), Some(Icon::Play), move |s, tx| {
                    if let Some(i) = s.table.index(&key) {
                        open_song(s, i, tx);
                    }
                    s.favorite_menu = None;
                })
                .key("play-menu-row"),
            );
        }
        if menu.row.is_none() && s.app.queue_item(&menu.uri, false).is_some() {
            for next in [true, false] {
                let uri = menu.uri.clone();
                popup = popup.child(v.menu_item(
                    &v.tr(if next {
                        mark("Play next")
                    } else {
                        mark("Add to queue")
                    }),
                    Some(Icon::ListVideo),
                    move |s, tx| {
                        if let Some(action) = s.app.queue_item(&uri, next) {
                            s.dispatch(Action::Queue(action), tx);
                        }
                    },
                ));
            }
        }
        if let Some([x, y]) = menu.at {
            popup = popup.absolute(
                x.clamp(0., (v.width - 232.).max(0.)),
                y.clamp(0., (v.height - 84.).max(0.)),
            );
        } else if let Some(key) = &menu.row {
            let trigger = format!("more-{key}");
            popup = popup
                .anchored_fit(&trigger, rune::Side::Bottom, rune::Align::End, 2.)
                .focus_return(trigger);
        }
        shell = shell.child(popup);
    }
    if let Some(menu) = queue::menu(v) {
        shell = shell.child(menu);
    }
    if let Some(dialog) = &s.app.presentation.dialog {
        shell = shell.child(match dialog {
            crate::model::Dialog::Shortcuts => preferences::shortcuts(v),
            crate::model::Dialog::Playlists => playlists::editor(v),
            crate::model::Dialog::DeletePlaylist => playlists::confirm_delete(v),
        });
    }
    autoscroll::shell(shell, s, v.zoom)
}
fn collection_queue_items(v: &Paint<'_>, mut menu: El, key: Option<String>) -> El {
    for next in [true, false] {
        let key = key.clone();
        menu = menu.child(
            v.menu_item(
                &v.tr(if next {
                    mark("Play next")
                } else {
                    mark("Add to queue")
                }),
                Some(Icon::ListVideo),
                move |s, tx| s.queue_selection(key.as_deref(), next, tx),
            )
            .key(if next {
                "play-next-selection"
            } else {
                "queue-add-selection"
            }),
        );
    }
    menu
}

fn page_tint(v: &Paint<'_>, page: &Page) -> Option<Srgba> {
    if let Page::LikedSongs = page {
        return Some(Srgba::from_rgb(0x50, 0x38, 0xc8));
    }
    let app = &v.s.app;
    if matches!(page, Page::Artist(_)) || !app.presentation.settings.accent_from_art {
        return None;
    }
    let tint = |id: &str| v.s.cover_tint(&app.cover_uri(id), v.p);
    if matches!(page, Page::Playlist(_)) {
        return app
            .presentation
            .navidrome_search
            .collection
            .get()
            .and_then(|collection| {
                if let Collection::Playlist { cover_art, .. } = collection {
                    cover_art.as_deref().and_then(tint)
                } else {
                    None
                }
            })
            .or_else(|| app.playing_cover().and_then(tint));
    }
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
                })
                .mr(2.),
            )
            .child(
                v.icon_button(&v.tr(mark("Home")), Icon::House, 18., |s, tx| {
                    s.dispatch(Action::Open(Page::Albums), tx)
                }),
            );
    }
    let navigation = |name, icon, enabled: bool, action: Action| {
        el().size(32., 32.)
            .center()
            .radius(16.)
            .fill(color(Srgba::from_black_alpha(if v.p.dark {
                90
            } else {
                20
            })))
            .role("button")
            .label(v.tr(name))
            .focusable(true)
            .disabled(!enabled)
            .child(v.icon(icon, 20., if enabled { v.p.secondary } else { v.p.dim }))
            .on_press_tx(move |s: &mut State, tx| s.dispatch(action.clone(), tx))
    };
    bar = bar
        .child(navigation(
            mark("Back"),
            Icon::ChevronLeft,
            v.s.app.presentation.can_go_back(),
            Action::Back,
        ))
        .child(navigation(
            mark("Forward"),
            Icon::ChevronRight,
            v.s.app.presentation.can_go_forward(),
            Action::Forward,
        ));
    let hint = v.tr(mark("What do you want to play?"));
    let input = text_input(&v.s.query, TextStyle::new(14., 400.))
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
    let width = crate::ui::topbar::navidrome_search_width(
        v.content_width() + 2. * crate::ui::widgets::PAGE_PADDING,
        v.s.app.presentation.settings.sidebar_visible,
    );
    let field = v
        .search_field("query", &hint, &v.s.query, input, |s, tx| {
            s.query.clear();
            s.query.focused = true;
            s.dispatch(Action::NavidromeSearch(Search::Edit(String::new())), tx);
        })
        .key("search-field")
        .ml(8.)
        .w(width);
    bar.child(field.disabled(!v.s.app.connected))
        .child(el().grow(1.))
        .child(
            v.icon_button(&v.tr(mark("Settings")), Icon::Settings, 18., |s, tx| {
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
                .label(v.tr(mark("Account")))
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
            v.menu_item(&v.tr(mark("Settings")), Some(Icon::Settings), |s, tx| {
                s.dispatch(Action::Open(Page::Settings), tx)
            }),
        )
        .child(
            v.menu_item(
                &v.tr(mark("Keyboard shortcuts")),
                Some(Icon::Info),
                |s, tx| s.dispatch(Action::ShowDialog(crate::model::Dialog::Shortcuts), tx),
            )
            .key("account-shortcuts"),
        )
        .child(rule())
        .child(
            v.menu_item(&v.tr(mark("Sign out")), Some(Icon::LogOut), |s, tx| {
                s.dispatch(Action::SignOut, tx)
            }),
        )
}

const SORTS: [(crate::settings::LibrarySort, &str); 4] = [
    (crate::settings::LibrarySort::Library, mark("Library order")),
    (
        crate::settings::LibrarySort::RecentlyPlayed,
        mark("Recently played"),
    ),
    (crate::settings::LibrarySort::Name, mark("Name")),
    (
        crate::settings::LibrarySort::RecentlyAdded,
        mark("Recently added"),
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
        (mark("Albums"), Icon::House, Page::Albums),
        (mark("Favorites"), Icon::Star, Page::LikedSongs),
        (mark("Playlists"), Icon::ListMusic, Page::Playlists),
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
                .child(label(v.tr(mark("Library")), 16., 650., v.p.text).grow(1.))
                .child(v.icon_button(
                    &v.tr(mark("Search in Your Library")),
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
                    &v.tr(mark("Show or hide the sidebar")),
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
                    .label(v.tr(mark("Albums")))
                    .focusable(true)
                    .child(label(v.tr(mark("Albums")), 13., 500., v.p.window))
                    .on_press_tx(|_: &mut State, _| {}),
            ),
        );
    let selected = crate::ui::sidebar::selected_sort(&app.presentation, LibraryShelf::Albums);
    let name = SORTS
        .iter()
        .find(|(sort, _)| *sort == selected)
        .map_or(mark("Library order"), |(_, name)| *name);
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
        let editor = text_input(&v.s.library_filter, TextStyle::new(14., 400.))
            .on_edit(|s: &mut State| &mut s.library_filter);
        side = side.child(
            v.search_field(
                "library-filter",
                &v.tr(mark("Search in Your Library")),
                &v.s.library_filter,
                editor,
                |s, _| {
                    s.library_filter.clear();
                    s.library_filter.focused = true;
                },
            )
            .key("library-search-field")
            .w((width - 28.).max(0.)),
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
                        format!("{} • {}", v.tr(mark("Album")), a.artist),
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
    let enabled = crate::autoscroll::enabled(app.presentation.settings.middle_click_autoscroll);
    let list = if enabled {
        let viewport = v.s.autoscroll.viewport(Pane::Library);
        autoscroll::pane(
            list.on_scroll_offset(move |pos| viewport.set(pos)),
            Pane::Library,
            true,
        )
    } else {
        list
    };
    let mut side = side.child(list);
    if app.presentation.settings.art_expanded && app.playing_cover().is_some() {
        let edge = (width - 20.)
            .min((v.height - crate::theme::PLAYER_BAR_HEIGHT - v.s.titlebar_inset() - 12.) * 0.45)
            .max(80.);
        let art = transport::artwork(v, edge, true).shadow(
            0.,
            28.,
            color(Srgba::from_black_alpha(if v.p.dark { 120 } else { 40 })),
        );
        side = if grid {
            side.child(
                col()
                    .pin(Some(12.), None, None, Some(0.))
                    .pb(10.)
                    .fill(color(v.p.panel))
                    .child(art),
            )
        } else {
            side.child(art.mb(10.))
        };
    }
    side
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
            || s.app.queue_item(&uri, false).is_some()
        {
            let zoom = s.app.presentation.settings.zoom;
            s.favorite_menu = Some(ItemMenu {
                uri: uri.clone(),
                row: None,
                at: Some([p.x / zoom, p.y / zoom]),
            });
        }
    })
}
fn credits(
    v: &Paint<'_>,
    name: &str,
    by: &crate::navidrome::api::ArtistCredits,
    weight: f32,
) -> El {
    if by.artists.iter().all(|a| a.name.trim().is_empty()) {
        return credit(v, name, by.artist_id.as_deref()).font(TextStyle::new(14., weight));
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
        names = names.child(credit(v, &a.name, a.id.as_deref()).font(TextStyle::new(14., weight)));
    }
    names
}
fn card_play(v: &Paint<'_>, kind: FavoriteKind, id: &str) -> El {
    let action = if kind == FavoriteKind::Song {
        Action::PlayFavorite(id.to_owned())
    } else {
        Action::PlayContext {
            uri: kind.uri(id),
            offset_index: None,
            offset_uri: None,
        }
    };
    card_play_action(v, format!("card-play-{}", kind.uri(id)), action)
}
fn card_play_action(v: &Paint<'_>, key: String, action: Action) -> El {
    el().key(key)
        .size(44., 44.)
        .pin(None, None, Some(4.), Some(4.))
        .radius(22.)
        .center()
        .role("button")
        .focusable(true)
        .label(v.tr(mark("Play")))
        .fill(color(v.p.accent))
        .hover(color(v.p.accent_hover))
        .child(v.icon(Icon::PlayFilled, 22., v.p.on_accent))
        .group_opacity(0., 1.)
        .on_press_tx(move |s: &mut State, tx| s.dispatch(action.clone(), tx))
}
fn collection_card(
    v: &Paint<'_>,
    key: String,
    name: &str,
    cover: Option<&str>,
    byline: El,
    play: El,
    page: Page,
) -> El {
    col()
        .key(key)
        .w(172.)
        .padding(12.)
        .gap(2.)
        .radius(8.)
        .hover(color(v.p.surface_hover))
        .role("button")
        .focusable(true)
        .label(name)
        .hover_group()
        .child(
            el().size(148., 148.)
                .child(v.cover(cover, CoverSize::Sleeve, 148., 6.))
                .child(play),
        )
        .child(label(name, 14., 600., v.p.text).mt(8.).truncate())
        .child(byline)
        .on_press_tx(move |s: &mut State, tx| s.dispatch(Action::Open(page.clone()), tx))
}
fn album_card(v: &Paint<'_>, a: &Album) -> El {
    let tile = collection_card(
        v,
        format!("album-{}", a.id),
        &a.name,
        a.cover_art.as_deref(),
        label(&a.artist, 12.5, 400., v.p.secondary).wrap(),
        card_play(v, FavoriteKind::Album, &a.id),
        Page::Album(navidrome::encode(&a.id)),
    )
    .label(format!("{}, {}", a.name, a.artist));
    favorite_context(tile, FavoriteKind::Album, &a.id)
}
fn playlist_card(v: &Paint<'_>, p: &crate::navidrome::api::NamedItem) -> El {
    collection_card(
        v,
        format!("playlist-{}", p.id),
        &p.name,
        p.cover_art.as_deref(),
        label(v.tr(mark("Playlist")), 12.5, 400., v.p.secondary).wrap(),
        card_play_action(
            v,
            format!("playlist-card-play-{}", p.id),
            Action::NavidromeSearch(Search::PlayPlaylist(p.id.clone())),
        ),
        Page::Playlist(navidrome::encode(&p.id)),
    )
}
fn albums(v: &Paint<'_>, width: f32) -> El {
    let a = &v.s.app;
    let n = crate::ui::widgets::card_columns((width - 48.).max(1.));
    let p = v.p;
    let scale = v.scale;
    let zoom = v.zoom;
    let rows = a.albums.len().div_ceil(n);
    let bounds = (v.width, v.height);
    let list = col()
        .w_full()
        .key("album-library-rows")
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
                    return heading(&v, &v.tr(mark("Albums")));
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
        );
    if !a.has_more || a.list_loading || a.error.is_some() {
        return list;
    }
    list.on_visible_rows(move |s: &mut State, visible| {
        // Row zero holds the title. Count only the album rows below the viewport.
        let remaining = (rows + 1).saturating_sub(visible.end);
        if remaining as f32 * 240. < crate::ui::widgets::LOAD_AHEAD
            && s.app.has_more
            && !s.app.list_loading
            && s.app.error.is_none()
            && s.app.presentation.page() == &Page::Albums
        {
            s.app.apply(Service::More);
            true
        } else {
            false
        }
    })
}
fn album_status(v: &Paint<'_>) -> El {
    let a = &v.s.app;
    let mut page = col();
    if let Some(e) = &a.error {
        page = page.child(
            row()
                .pl(8.)
                .gap(8.)
                .items_center()
                .child(v.icon(Icon::CircleAlert, 16., v.p.danger))
                .child(retry(v, e, Action::Reload(Page::Albums)).key("album-list-error")),
        );
    } else if a.list_loading || a.restoring_login {
        page = page.child(v.loading());
    } else if a.albums.is_empty() {
        page = page.child(v.empty(
            Icon::Disc,
            &v.tr(mark("No albums")),
            &v.tr(mark("Albums in your Navidrome library appear here.")),
        ));
    }
    page
}
/// The album page defines the common layout for playable collections.
fn collection_header(
    v: &Paint<'_>,
    kind: &str,
    name: &str,
    cover: Option<&str>,
    byline: El,
    actions: El,
) -> El {
    let edge = if v.content_width() > 720. { 212. } else { 160. };
    let release = col()
        .gap(6.)
        .grow(1.)
        .min_w(0.)
        .pt(edge * 0.08)
        .child(label(kind, 12.5, 500., v.p.text))
        .child(
            label(name, if edge > 200. { 56. } else { 40. }, 700., v.p.text)
                .wrap()
                .key("collection-title"),
        )
        .child(byline);
    col()
        .w_full()
        .gap(12.)
        .child(
            row()
                .w_full()
                .gap(24.)
                .child(
                    v.cover(cover, CoverSize::Sleeve, edge, 6.)
                        .key("collection-art"),
                )
                .child(release),
        )
        .child(actions)
}

fn collection_actions(v: &Paint<'_>, key: &'static str, active: bool, empty: bool) -> El {
    row()
        .mt(14.)
        .mb(20.)
        .items_center()
        .gap(18.)
        .child(
            el().size(56., 56.)
                .key(key)
                .center()
                .radius(28.)
                .fill(color(v.p.accent))
                .hover(color(v.p.accent_hover))
                .role("button")
                .focusable(true)
                .label(v.tr(if active { mark("Pause") } else { mark("Play") }))
                .child(v.icon(
                    if active {
                        Icon::PauseFilled
                    } else {
                        Icon::PlayFilled
                    },
                    28.,
                    v.p.on_accent,
                ))
                .disabled(!active && empty)
                .on_press(move |s: &mut State| {
                    if active {
                        s.app.apply(Service::Toggle);
                    } else {
                        s.play_collection_row(0);
                    }
                }),
        )
        .child(
            transport::shuffle(v, 26.)
                .size(38., 38.)
                .key("collection-shuffle"),
        )
}

fn album(v: &Paint<'_>) -> El {
    let app = &v.s.app;
    let Some(a) = app.album.as_ref() else {
        return if let Some(e) = &app.error {
            col().child(label(e, 14., 400., v.p.danger)).child(v.button(
                &v.tr(mark("Retry")),
                false,
                |s, tx| s.dispatch(Action::Reload(s.app.presentation.page().clone()), tx),
            ))
        } else {
            v.loading()
        };
    };
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
    let active = app
        .playing_album
        .as_ref()
        .is_some_and(|playing| playing.id == a.id)
        && !app.transport.paused;
    let actions = collection_actions(v, "album-play", active, a.song.is_empty())
        .child(star(v, FavoriteKind::Album, &a.id, 26.))
        .child(
            v.icon_button(&v.tr(mark("More")), Icon::Ellipsis, 26., |s, _| {
                s.menu = Some("album")
            })
            .size(38., 38.)
            .key("album-more"),
        );
    let mut page = collection_header(
        v,
        &v.tr(mark("Album")),
        &a.name,
        a.cover_art.as_deref(),
        byline.child(label(format!("• {details}"), 13.5, 400., v.p.secondary)),
        actions,
    );
    if a.song.is_empty() {
        page = page.child(label(v.tr("No tracks"), 14., 400., v.p.secondary));
    }
    page
}

fn playlist_header(v: &Paint<'_>) -> El {
    let app = &v.s.app;
    let Some(Collection::Playlist {
        name,
        songs,
        cover_art,
        owner,
        ..
    }) = app.presentation.navidrome_search.collection.get()
    else {
        return collection(v);
    };
    let mut byline = row().items_center().flex_wrap().gap(4.);
    if let Some(owner) = owner.as_deref().filter(|s| !s.is_empty()) {
        byline = byline
            .child(label(owner, 14., 500., v.p.secondary))
            .child(label("•", 13.5, 400., v.p.secondary));
    }
    let duration = songs.iter().map(|s| (s.duration * 1000.) as u64).sum();
    byline = byline.child(label(
        crate::ui::collection::songs_and_duration(
            app.presentation.locale,
            songs.len() as u32,
            duration,
        ),
        13.5,
        400.,
        v.p.secondary,
    ));
    let active = matches!(app.presentation.page(), Page::Playlist(id) if app.playing_playlist.as_ref() == Some(id))
        && !app.transport.paused;
    let mut page = collection_header(
        v,
        &v.tr(mark("Playlist")),
        name,
        cover_art.as_deref(),
        byline,
        collection_actions(v, "playlist-play", active, songs.is_empty()).child(
            v.icon_button(&v.tr(mark("More")), Icon::Ellipsis, 26., |s, _| {
                s.menu = Some("playlist")
            })
            .size(38., 38.)
            .key("playlist-more"),
        ),
    );
    if let Page::Playlist(id) = app.presentation.page()
        && let Some(progress) = playlists::progress(v, navidrome::decode(id).as_deref())
    {
        page = page.child(progress);
    }
    if songs.is_empty() {
        page = page.child(label(
            v.tr(mark("Nothing here yet.")),
            14.,
            400.,
            v.p.secondary,
        ));
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
pub(super) fn star(v: &Paint<'_>, kind: FavoriteKind, id: &str, size: f32) -> El {
    let uri = kind.uri(id);
    let saved = v.s.app.presentation.navidrome_favorites.contains(&uri);
    v.tinted_button(
        &v.tr(if saved == Some(true) {
            mark("Remove from Favorites")
        } else {
            mark("Add to Favorites")
        }),
        if saved == Some(true) {
            Icon::StarFilled
        } else {
            Icon::Star
        },
        size,
        if saved == Some(true) {
            v.p.accent
        } else {
            v.p.secondary
        },
        move |s, tx| s.dispatch(Action::ToggleSaved(uri.clone()), tx),
    )
    .size(size + 12., size + 12.)
    .disabled(saved.is_none())
}
fn list_songs(s: &State) -> &[Song] {
    s.app.page_songs().unwrap_or_default()
}
fn open_song(s: &mut State, i: usize, tx: &mut Tx) {
    s.play_collection_row(i);
    tx.instant();
}
fn collection_columns(v: &Paint<'_>) -> El {
    use crate::model::SortColumn;
    let sort = v.s.collection_sort();
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
                s.sort_collection(column);
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
        .pb(12.)
        .child(
            row()
                .w_full()
                .h(33.)
                .pl(22.)
                .pr(40.)
                .items_center()
                .child(button(
                    SortColumn::Index,
                    v.tr(mark("Sort by playlist order")),
                    30.,
                    "#",
                ))
                .child(button(
                    SortColumn::Title,
                    v.tr(mark("Sort by {column}")).replace("{column}", &title),
                    88.,
                    &title,
                ))
                .child(el().grow(1.))
                .child(button(
                    SortColumn::Duration,
                    v.tr(mark("Sort by duration")),
                    38.,
                    "",
                )),
        )
        .child(el().mx(8.).h(1.).fill(color(v.p.outline)))
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
                let header = if matches!(s.app.presentation.page(), Page::Playlist(_)) {
                    playlist_header(&v)
                } else {
                    album(&v)
                };
                let head = col().w_full().child(header);
                return if s.app.page_songs().is_some_and(|songs| !songs.is_empty()) {
                    head.child(collection_columns(&v))
                } else {
                    head
                };
            }
            let i = r - 1;
            let source = s.table.source(i);
            let song = &list_songs(s)[source];
            let key = s.table.key(i).to_owned();
            let playing = current_key.as_ref() == Some(&key);
            let selected = s.table.selection.rows.contains(&i);
            let ink = if playing { p.accent } else { p.text };
            let saved = s
                .app
                .presentation
                .navidrome_favorites
                .contains(&FavoriteKind::Song.uri(&song.id))
                == Some(true);
            let show_more = s
                .favorite_menu
                .as_ref()
                .is_some_and(|menu| menu.row.as_ref() == Some(&key));
            let mut number = el()
                .size(44., h)
                .center()
                .key(format!("number-{key}"))
                .role("button")
                .focusable(true)
                .label(v.tr(if playing && !s.app.transport.paused {
                    mark("Pause")
                } else {
                    mark("Play")
                }))
                .on_press_tx({
                    let key = key.clone();
                    move |s: &mut State, tx| {
                        if playing {
                            s.app.apply(Service::Toggle);
                        } else if let Some(i) = s.table.index(&key) {
                            open_song(s, i, tx);
                        }
                    }
                });
            let normal = if playing && !s.app.transport.paused {
                v.icon(Icon::AudioLines, 16., p.accent)
            } else {
                label(
                    (source + 1).to_string(),
                    14.,
                    400.,
                    if playing { p.accent } else { p.secondary },
                )
            };
            number = number.child(normal.group_opacity(1., 0.)).child(
                el().pin(Some(0.), Some(0.), Some(0.), Some(0.))
                    .center()
                    .child(v.icon(
                        if playing && !s.app.transport.paused {
                            Icon::PauseFilled
                        } else {
                            Icon::PlayFilled
                        },
                        14.,
                        p.text,
                    ))
                    .group_opacity(0., 1.),
            );
            let description = if s.app.presentation.settings.tracklist_compact {
                row()
                    .min_w(0.)
                    .grow(1.)
                    .items_center()
                    .gap(5.)
                    .child(label(&song.title, 14., 550., ink).truncate())
                    .child(label("•", 12., 400., p.secondary))
                    .child(credits(&v, &song.artist, &song.credits, 500.))
            } else {
                col()
                    .min_w(0.)
                    .grow(1.)
                    .items_start()
                    .child(label(&song.title, 14., 550., ink).w_full().truncate())
                    .child(credits(&v, &song.artist, &song.credits, 500.))
            };
            let uri = FavoriteKind::Song.uri(&song.id);
            let more = v
                .icon_button(&v.tr(mark("More")), Icon::Ellipsis, 18., {
                    let key = key.clone();
                    let uri = uri.clone();
                    move |s, _| {
                        s.favorite_menu = Some(ItemMenu {
                            uri: uri.clone(),
                            row: Some(key.clone()),
                            at: None,
                        })
                    }
                })
                .key(format!("more-{key}"))
                .group_opacity(if show_more { 1. } else { 0. }, 1.);
            row()
                .key(key.clone())
                .w_full()
                .h(h)
                .items_center()
                .px(8.)
                .radius(6.)
                .role("row")
                .focus_selected(selected)
                .label(format!("{} {} {}", source + 1, song.title, song.artist))
                .hover_group()
                .fill(if selected {
                    color(p.secondary.gamma_multiply(0.20))
                } else {
                    Color::TRANSPARENT
                })
                .hover(color(if selected {
                    p.secondary.gamma_multiply(0.30)
                } else {
                    p.surface_hover
                        .gamma_multiply(if p.dark { 0.7 } else { 1. })
                }))
                .child(number)
                .child(description)
                .child(
                    el().w(36.)
                        .center()
                        .child(star(&v, FavoriteKind::Song, &song.id, 16.))
                        .group_opacity(if saved { 1. } else { 0. }, 1.),
                )
                .child(row().w(56.).pr(6.).justify_end().child(label(
                    crate::util::format_duration_ms((song.duration * 1000.) as u32),
                    13.,
                    400.,
                    p.secondary,
                )))
                .child(el().w(36.).center().child(more))
                .on_secondary_press({
                    let key = key.clone();
                    move |s: &mut State, p, _| {
                        let zoom = s.app.presentation.settings.zoom;
                        s.favorite_menu = Some(ItemMenu {
                            uri: uri.clone(),
                            row: Some(key.clone()),
                            at: Some([p.x / zoom, p.y / zoom]),
                        });
                    }
                })
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
fn retry(v: &Paint<'_>, error: &str, action: Action) -> El {
    let name = v.tr(mark("Retry"));
    row()
        .w_full()
        .flex_wrap()
        .gap(8.)
        .items_center()
        .child(label(error, 13., 400., v.p.secondary).wrap())
        .child(
            row()
                .h(28.)
                .px(10.)
                .gap(6.)
                .radius(14.)
                .items_center()
                .role("button")
                .label(&name)
                .focusable(true)
                .fill(color(v.p.surface))
                .hover(color(v.p.surface_hover))
                .child(v.icon(Icon::Refresh, 14., v.p.secondary))
                .child(label(name, 13., 600., v.p.text))
                .on_press_tx(move |s: &mut State, tx| s.dispatch(action.clone(), tx)),
        )
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
        .gap(4.)
        .pr(8.)
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
        .child(el().size(32., 32.).center().child(v.icon(
            icon,
            16.,
            if selected { v.p.accent } else { v.p.secondary },
        )))
        .child(label(name, 14., 400., v.p.text).truncate());
    if let Some((by, details)) = artist
        && !by.is_empty()
    {
        hit_row = hit_row
            .child(label("·", 14., 400., v.p.secondary).mx(1.))
            .child(credits(v, by, details, 400.));
    }
    hit_row
}
fn playlists(v: &Paint<'_>) -> El {
    let count =
        v.s.app
            .presentation
            .navidrome_playlists
            .list
            .get()
            .map_or(0, Vec::len);
    let columns = crate::ui::widgets::card_columns(v.content_width());
    let rows = count.div_ceil(columns);
    let (p, scale, zoom, width, height) = (v.p, v.scale, v.zoom, v.width, v.height);
    col()
        .w_full()
        .key("playlist-library-rows")
        .growth(rune::GrowthSpec::default())
        .virtual_rows(
            rows + 1,
            240.,
            |_: &State, i| {
                if i == 0 {
                    "playlists-heading".into()
                } else {
                    format!("playlists-{i}").into()
                }
            },
            move |s: &State, i| {
                let v = Paint {
                    s,
                    p,
                    scale,
                    zoom,
                    width,
                    height,
                };
                if i == 0 {
                    return playlists_header(&v);
                }
                let mut band = row().h(240.).gap(7.);
                for item in s
                    .app
                    .presentation
                    .navidrome_playlists
                    .list
                    .get()
                    .into_iter()
                    .flatten()
                    .skip((i - 1) * columns)
                    .take(columns)
                {
                    band = band.child(playlist_card(&v, item));
                }
                band
            },
        )
}

fn playlists_header(v: &Paint<'_>) -> El {
    let state = &v.s.app.presentation.navidrome_playlists;
    let mut head = col().w_full().gap(12.).child(
        row()
            .items_center()
            .flex_wrap()
            .gap(12.)
            .child(heading(v, &v.tr(mark("Playlists"))).grow(1.))
            .child(
                v.button(&v.tr(mark("Create playlist")), true, |s, tx| {
                    s.playlist_name.clear();
                    s.dispatch(
                        Action::EditPlaylists(navidrome::playlists::Action::Open(Vec::new())),
                        tx,
                    );
                })
                .key("create-playlist"),
            )
            .child(
                v.button(&v.tr(mark("Refresh")), false, |s, tx| {
                    s.dispatch(Action::Reload(Page::Playlists), tx)
                })
                .key("refresh-playlists"),
            ),
    );
    if let Some(progress) = playlists::progress(v, None) {
        head = head.child(progress);
    }
    if state.refreshing {
        head = head.child(v.loading());
    }
    if let Some(error) = &state.error {
        head = head.child(retry(v, error, Action::Reload(Page::Playlists)));
    }
    if state.list.get().is_some_and(Vec::is_empty) {
        head = head.child(v.empty(
            Icon::ListMusic,
            &v.tr(mark("No playlists")),
            &v.tr(mark("Playlists available on your server appear here.")),
        ));
    }
    head
}

fn search(v: &Paint<'_>) -> El {
    let s = &v.s.app.presentation.navidrome_search;
    let mut list = col().w_full().gap(6.);
    if v.s.query.text.trim().is_empty() {
        if s.recent.is_empty() {
            return v
                .empty(
                    Icon::Search,
                    &v.tr(mark("Search")),
                    &v.tr(mark("What do you want to play?")),
                )
                .mt(6.);
        }
        list =
            col()
                .w_full()
                .pt(6.)
                .child(label(v.tr(mark("Recent searches")), 17., 700., v.p.text));
        let mut terms = row().flex_wrap().gap(8.).mt(12.);
        for term in &s.recent {
            let query = term.clone();
            let forget = term.clone();
            let remove = el()
                .key(format!("forget-query-{term}"))
                .size(21., 21.)
                .center()
                .role("button")
                .label(format!("{} {term}", v.tr(mark("Remove"))))
                .focusable(true)
                .child(v.icon(Icon::Clock, 15., v.p.text).group_opacity(1., 0.))
                .child(
                    v.icon(Icon::X, 15., v.p.text)
                        .absolute(3., 3.)
                        .group_opacity(0., 1.),
                )
                .on_press_tx(move |s: &mut State, tx| {
                    s.dispatch(Action::ForgetSearch(forget.clone()), tx)
                });
            terms = terms.child(
                row()
                    .key(format!("recent-query-{term}"))
                    .h(30.)
                    .pl(9.)
                    .pr(12.)
                    .gap(3.)
                    .items_center()
                    .radius(15.)
                    .fill(color(v.p.surface))
                    .hover(color(v.p.surface_hover))
                    .hover_group()
                    .role("button")
                    .label(term)
                    .focusable(true)
                    .child(remove)
                    .child(label(term, 13., 500., v.p.text))
                    .on_press_tx(move |s: &mut State, tx| {
                        s.query.replace_text(&query);
                        s.dispatch(Action::Search(query.clone()), tx);
                    }),
            );
        }
        return list.child(terms);
    }
    if v.s.app.presentation.search.typed_at.is_some()
        || s.catalogue.is_loading()
        || s.playlists.is_loading()
    {
        list = list.child(v.loading());
    }
    if let Some(error) = failed(&s.catalogue) {
        let title = [
            v.tr(mark("Songs")),
            v.tr(mark("Albums")),
            v.tr(mark("Artists")),
        ]
        .join(" · ");
        list = list.child(label(title, 17., 700., v.p.text)).child(retry(
            v,
            error,
            Action::NavidromeSearch(Search::Retry(SearchPart::Catalogue)),
        ));
    }
    let mut previous = None;
    for (hit, name, artist) in s.rows() {
        use crate::app::navidrome::search::Hit;
        let section = match hit {
            Hit::Song(_) => mark("Songs"),
            Hit::Album(_) => mark("Albums"),
            Hit::Artist(_) => mark("Artists"),
            Hit::Playlist(_) => mark("Playlists"),
        };
        if previous != Some(section) {
            list = list.child(label(v.tr(section), 17., 700., v.p.text).mt(9.));
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
            .gap(8.)
            .child(hit_row.on_press_tx(move |s: &mut State, tx| {
                s.dispatch(Action::NavidromeSearch(Search::Activate(hit)), tx)
            }));
        if let Some((kind, id)) = s.catalogue.get().and_then(|c| match hit {
            Hit::Song(i) => Some((FavoriteKind::Song, &c.song[i].id)),
            Hit::Album(i) => Some((FavoriteKind::Album, &c.album[i].id)),
            Hit::Artist(i) => Some((FavoriteKind::Artist, &c.artist[i].id)),
            Hit::Playlist(_) => None,
        }) {
            line = line.child(star(v, kind, id, 17.));
        }
        list = list.child(line);
    }
    if let Some(error) = failed(&s.playlists) {
        list = list
            .child(label(v.tr(mark("Playlists")), 17., 700., v.p.text).mt(9.))
            .child(retry(
                v,
                error,
                Action::NavidromeSearch(Search::Retry(SearchPart::Playlists)),
            ));
    }
    if s.catalogue.get().is_some() && s.playlists.get().is_some() && previous.is_none() {
        list = list.child(
            v.empty(
                Icon::Search,
                &v.tr(mark("No results for “{query}”"))
                    .replace("{query}", &v.s.app.presentation.search.committed),
                &v.tr(mark("Check the spelling, or try fewer words.")),
            ),
        );
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
            &v.tr(mark("Retry")),
            false,
            |s, tx| s.dispatch(Action::Reload(s.app.presentation.page().clone()), tx),
        )),
        _ => v.loading(),
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
            mark("Songs"),
            mark("Songs you favorite appear here."),
            state.list.song.len(),
        ),
        (
            FavoriteKind::Album,
            mark("Albums"),
            mark("Albums you favorite appear here."),
            state.list.album.len(),
        ),
        (
            FavoriteKind::Artist,
            mark("Artists"),
            mark("Artists you favorite appear here."),
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
                    v.button(&v.tr(mark("See all")), false, move |s, tx| {
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
                .key(format!("favorites-{kind:?}-rows"))
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
        None => mark("Favorites"),
        Some(FavoriteKind::Song) => mark("Favorite Songs"),
        Some(FavoriteKind::Album) => mark("Favorite Albums"),
        Some(FavoriteKind::Artist) => mark("Favorite Artists"),
    };
    let mut page = col().gap(14.).child(
        row()
            .items_center()
            .gap(4.)
            .child(label(v.tr(title), 28., 700., v.p.text))
            .child(
                v.icon_button(&v.tr(mark("Refresh")), Icon::Refresh, 16., |s, tx| {
                    s.dispatch(Action::Reload(s.app.presentation.page().clone()), tx)
                })
                .disabled(state.loading),
            ),
    );
    if state.loading {
        page = page.child(v.loading());
    }
    if let Some(e) = &state.error {
        page = page.child(label(e, 14., 400., v.p.danger)).child(v.button(
            &v.tr(mark("Retry")),
            false,
            |s, tx| s.dispatch(Action::Reload(Page::LikedSongs), tx),
        ));
    }
    page
}

fn favorite_write_errors(v: &Paint<'_>) -> El {
    let mut errors = col()
        .w_full()
        .max_h(120.)
        .px(crate::ui::widgets::PAGE_PADDING)
        .py(4.)
        .gap(4.)
        .scroll(ScrollSpec {
            stick_bottom: false,
        });
    for (uri, failure) in &v.s.app.presentation.navidrome_favorites.failures {
        let uri = uri.clone();
        let message = v
            .tr(mark("Could not update Favorites for {name}. {error}"))
            .replace("{name}", failure.item.name())
            .replace("{error}", &failure.message);
        errors = errors.child(
            row()
                .w_full()
                .gap(8.)
                .items_center()
                .child(v.button(&v.tr(mark("Retry")), false, move |s, tx| {
                    s.dispatch(Action::RetryFavorite(uri.clone()), tx)
                }))
                .child(label(message, 13., 400., v.p.text).min_w(0.).grow(1.)),
        );
    }
    errors
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
        let mut art = el().size(148., 148.).child(cover).child(
            star(v, kind, id, 18.)
                .size(26., 26.)
                .absolute(118., 4.)
                .radius(13.)
                .fill(color(v.p.panel)),
        );
        if kind != FavoriteKind::Artist {
            art = art.child(card_play(v, kind, id));
        }
        let subtitle = by.map_or_else(
            || label(v.tr(mark("Artist")), 12.5, 400., v.p.secondary),
            |(name, by)| credits(v, name, by, 500.),
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
            .hover_group()
            .hover(color(v.p.surface_hover))
            .child(art)
            .child(label(name, 14., 600., v.p.text).mt(10.).truncate())
            .child(subtitle)
            .on_press_tx(move |s: &mut State, tx| s.dispatch(action.clone(), tx));
        band = band.child(favorite_context(tile, kind, id));
    }
    band
}
