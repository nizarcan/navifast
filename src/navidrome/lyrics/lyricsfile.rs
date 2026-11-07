//! LRCLIB Lyricsfile 1.0. Unknown or malformed formats keep the legacy fallback.
use super::{Line, Lyrics, MAX_LINES, MAX_TEXT, Outcome, Word, words::MAX_WORDS};
use serde::Deserialize;

#[derive(Deserialize)]
struct File {
    version: String,
    metadata: Metadata,
    #[serde(default)]
    lines: Vec<RawLine>,
    plain: Option<String>,
}
#[derive(Deserialize)]
struct Metadata {
    #[serde(default)]
    instrumental: bool,
    #[serde(default)]
    offset_ms: i64,
}
#[derive(Deserialize)]
struct RawLine {
    text: String,
    start_ms: u64,
    end_ms: Option<u64>,
    #[serde(default)]
    words: Vec<RawWord>,
}
#[derive(Deserialize)]
struct RawWord {
    text: String,
    start_ms: u64,
    end_ms: Option<u64>,
}
fn word_ranges(line: &RawLine) -> Option<Vec<Word>> {
    if line.words.len() > MAX_WORDS {
        return None;
    }
    let mut byte = 0;
    let mut previous = line.start_ms;
    let mut words = Vec::with_capacity(line.words.len());
    for word in &line.words {
        let end = byte + word.text.len();
        if word.text.is_empty()
            || line.text.get(byte..end)? != word.text
            || word.start_ms < previous
            || word.end_ms.is_some_and(|end| end < word.start_ms)
            || line
                .end_ms
                .is_some_and(|end| word.start_ms > end || word.end_ms.is_some_and(|t| t > end))
        {
            return None;
        }
        words.push(Word {
            range: byte..end,
            start_ms: word.start_ms,
            end_ms: word.end_ms,
        });
        byte = end;
        previous = word.start_ms;
    }
    if byte != line.text.len() {
        return None;
    }
    if let Some(word) = words.last_mut() {
        word.end_ms = word.end_ms.or(line.end_ms);
    }
    Some(words)
}
pub(super) fn parse(text: &str) -> Option<Outcome> {
    if text.len() > 4 * MAX_TEXT {
        return None;
    }
    let options = serde_saphyr::options! {
        reject_unsupported_tags: true,
        budget: serde_saphyr::budget! {
            max_depth: 8,
            flow_nesting_limit: 8,
            max_nodes: 100_000,
            max_events: 200_000,
            max_total_scalar_bytes: 4 * MAX_TEXT,
            max_aliases: 0,
            max_anchors: 0,
            max_merge_keys: 0,
            max_inclusion_depth: 0,
            max_documents: 1,
        },
    };
    let file: File = serde_saphyr::from_str_with_options(text, options).ok()?;
    // The draft explicitly leaves nonzero offsets undefined. Do not guess.
    if file.version != "1.0" || file.metadata.offset_ms != 0 {
        return None;
    }
    if file.metadata.instrumental {
        return Some(Outcome::Instrumental);
    }
    if file.lines.is_empty() {
        return super::plain(serde_json::json!({"lyrics":{"value":file.plain?}})).ok();
    }
    if file.lines.iter().all(|line| line.text.trim().is_empty()) {
        return Some(Outcome::Missing);
    }
    if file.lines.len() > MAX_LINES
        || file.lines.iter().map(|l| l.text.len()).sum::<usize>() > MAX_TEXT
    {
        return None;
    }
    let valid = file
        .lines
        .windows(2)
        .all(|w| w[0].start_ms <= w[1].start_ms)
        && file
            .lines
            .iter()
            .all(|l| l.end_ms.is_none_or(|end| end >= l.start_ms));
    let lines = file
        .lines
        .into_iter()
        .map(|line| {
            let words = if valid {
                word_ranges(&line).unwrap_or_default()
            } else {
                vec![]
            };
            Line {
                text: line.text,
                at_ms: valid.then_some(line.start_ms),
                words,
            }
        })
        .collect();
    Some(Outcome::Found(Lyrics {
        lines,
        timed: valid,
        invalid_timing: !valid,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    const DOCUMENT: &str = "version: '1.0'\nmetadata: {}\nlines:\n- text: 'Hello עולם'\n  start_ms: 100\n  end_ms: 900\n  words:\n  - { text: 'Hello ', start_ms: 100, end_ms: 400 }\n  - { text: 'עולם', start_ms: 400, end_ms: 900 }\n";
    #[test]
    fn real_words_keep_spaces_and_unicode_byte_ranges() {
        let Outcome::Found(lyrics) = parse(DOCUMENT).unwrap() else {
            panic!()
        };
        assert!(lyrics.timed);
        assert_eq!(lyrics.lines[0].words[1].range, 6..14);
        assert_eq!(lyrics.lines[0].active_words(450).collect::<Vec<_>>(), [1]);
        assert_eq!(lyrics.lines[0].active_words(100).collect::<Vec<_>>(), [0]);
        assert_eq!(lyrics.lines[0].active_words(950).count(), 0);
    }
    #[test]
    fn invalid_word_text_keeps_lines_and_unsafe_or_unknown_yaml_is_rejected() {
        let Outcome::Found(lyrics) =
            parse(&DOCUMENT.replace("text: 'Hello ',", "text: 'Other ',")).unwrap()
        else {
            panic!()
        };
        assert!(lyrics.lines[0].words.is_empty());
        for text in [
            DOCUMENT.replace("'1.0'", "'2.0'"),
            DOCUMENT.replace("metadata: {}", "metadata: {offset_ms: 10}"),
            DOCUMENT.replace("lines:", "lines: &alias"),
            format!("{DOCUMENT}\nversion: '1.0'"),
            format!("{DOCUMENT}\n---\n{DOCUMENT}"),
        ] {
            assert!(parse(&text).is_none());
        }
    }
}
