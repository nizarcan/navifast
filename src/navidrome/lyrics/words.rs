//! Byte ranges stay in logical UTF-8 order; Rune handles shaping and bidi layout.
use super::Line;
use serde::Deserialize;
use serde_json::Value;
use std::ops::Range;

pub(super) const MAX_WORDS: usize = 512;

#[derive(Clone, Debug, PartialEq)]
pub struct Word {
    pub range: Range<usize>,
    pub start_ms: u64,
    pub end_ms: Option<u64>,
}
impl Line {
    pub fn active_words(&self, position_ms: u64) -> impl Iterator<Item = usize> + '_ {
        self.words.iter().enumerate().filter_map(move |(i, word)| {
            let end = word
                .end_ms
                .or_else(|| self.words.get(i + 1).map(|w| w.start_ms));
            (position_ms >= word.start_ms && end.is_none_or(|end| position_ms < end)).then_some(i)
        })
    }
}

#[derive(Deserialize)]
struct CueLine {
    index: usize,
    value: String,
    end: Option<u64>,
    cue: Vec<Cue>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Cue {
    value: String,
    start: u64,
    end: Option<u64>,
    byte_start: usize,
    byte_end: usize,
}
fn shifted(time: u64, offset: i64) -> Option<u64> {
    i64::try_from(time)
        .ok()?
        .checked_sub(offset)
        .map(|t| t.max(0) as u64)
}
fn cues(line: &Line, cue: CueLine, offset: i64) -> Option<Vec<Word>> {
    // A combined multi-voice line cannot safely use a single voice's ranges.
    if cue.value != line.text || cue.cue.len() > MAX_WORDS {
        return None;
    }
    let mut words = Vec::with_capacity(cue.cue.len());
    let mut previous_byte = 0;
    let mut previous_time = line.at_ms?;
    let has_end = cue.cue.first().is_some_and(|c| c.end.is_some());
    let line_end = optional_shift(cue.end, offset)?;
    for c in cue.cue {
        let end_byte = c.byte_end.checked_add(1)?;
        let range = c.byte_start..end_byte;
        let start_ms = shifted(c.start, offset)?;
        let end_ms = optional_shift(c.end, offset)?;
        if range.start < previous_byte
            || line.text.get(range.clone())? != c.value
            || c.value.is_empty()
            || start_ms < previous_time
            || end_ms.is_some_and(|end| end < start_ms)
            || c.end.is_some() != has_end
            || line_end.is_some_and(|end| start_ms > end || end_ms.is_some_and(|t| t > end))
        {
            return None;
        }
        previous_byte = range.end;
        previous_time = start_ms;
        words.push(Word {
            range,
            start_ms,
            end_ms,
        });
    }
    if let Some(word) = words.last_mut() {
        word.end_ms = word.end_ms.or(line_end);
    }
    Some(words)
}
// Preserve the difference between an absent end and an invalid shifted end.
fn optional_shift(time: Option<u64>, offset: i64) -> Option<Option<u64>> {
    match time {
        None => Some(None),
        Some(time) => shifted(time, offset).map(Some),
    }
}
pub(super) fn attach(lines: &mut [Line], raw: Vec<Value>, offset: i64) {
    let mut seen = std::collections::HashSet::new();
    for value in raw {
        let Ok(cue) = serde_json::from_value::<CueLine>(value) else {
            continue;
        };
        let Some(line) = lines.get_mut(cue.index) else {
            continue;
        };
        if !seen.insert(cue.index) {
            // Keep the parent text readable for overlapping vocal layers.
            line.words.clear();
            continue;
        }
        line.words = cues(line, cue, offset).unwrap_or_default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn exact_utf8_ranges_and_real_times_survive_bidi_and_repeated_words() {
        let mut lines = vec![Line {
            text: "مرحبا hello hello".into(),
            at_ms: Some(0),
            words: vec![],
        }];
        attach(
            &mut lines,
            vec![json!({"index":0,"value":"مرحبا hello hello","cue":[
                {"start":0,"end":100,"value":"مرحبا","byteStart":0,"byteEnd":9},
                {"start":200,"end":400,"value":"hello","byteStart":17,"byteEnd":21}
            ]})],
            0,
        );
        assert_eq!(lines[0].words[1].range, 17..22);
        assert_eq!(lines[0].active_words(250).collect::<Vec<_>>(), [1]);
        assert_eq!(lines[0].active_words(400).count(), 0);
        assert_eq!(lines[0].active_words(150).count(), 0);
    }
    #[test]
    fn invalid_boundaries_and_partial_end_times_fall_back_to_line_timing() {
        for cue in [
            json!([{"start":0,"value":"é","byteStart":1,"byteEnd":1}]),
            json!([{"start":0,"end":100,"value":"é","byteStart":0,"byteEnd":1},{"start":200,"value":"a","byteStart":2,"byteEnd":2}]),
        ] {
            let mut lines = vec![Line {
                text: "éa".into(),
                at_ms: Some(0),
                words: vec![],
            }];
            attach(
                &mut lines,
                vec![json!({"index":0,"value":"éa","cue":cue})],
                0,
            );
            assert!(lines[0].words.is_empty());
        }
    }
}
