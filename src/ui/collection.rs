use crate::{
    i18n::{Locale, ngettext},
    util,
};
pub(crate) fn songs_and_duration(locale: Locale, count: u32, duration_ms: u64) -> String {
    ngettext(
        locale,
        // Translators: {count} is a number of songs and {duration} their total
        // length, such as "2 hr 13 min".
        "{count} song, {duration}",
        "{count} songs, {duration}",
        count,
    )
    .replace("{count}", &util::format_count(count as u64))
    .replace("{duration}", &util::format_total_ms(locale, duration_ms))
}
