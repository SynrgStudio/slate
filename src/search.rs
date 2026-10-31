#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct SearchState {
    pub query: String,
    pub matches: Vec<(usize, usize)>,
    pub selected: usize,
    pub buffer_revision: u64,
}

#[allow(dead_code)]
impl SearchState {
    pub fn new(query: String, text: &str, buffer_revision: u64) -> Self {
        let matches = find_matches(text, &query);
        Self {
            query,
            matches,
            selected: 0,
            buffer_revision,
        }
    }

    pub fn selected_match(&self) -> Option<(usize, usize)> {
        self.matches.get(self.selected).copied()
    }
}

/// Inputs smaller than this are searched on the caller thread; splitting them
/// costs more than it saves.
const PARALLEL_SEARCH_MIN_BYTES: usize = 256 * 1024;

/// Case-insensitive search returning every (overlapping) match as byte ranges of
/// `text`. Large texts are split into chunks searched on all cores and merged in
/// order before returning, so callers still get a complete result in one call.
#[allow(dead_code)]
pub fn find_matches(text: &str, query: &str) -> Vec<(usize, usize)> {
    if query.is_empty() || query.len() > text.len() {
        return Vec::new();
    }
    let mut needle = Vec::with_capacity(query.len());
    fold_case_into(query, &mut needle);
    let finder = memchr::memmem::Finder::new(&needle);

    let workers = crate::parallel::worker_count();
    if workers <= 1 || text.len() < PARALLEL_SEARCH_MIN_BYTES {
        return find_in_window(text, 0, text.len(), &finder);
    }

    let chunk_len = text.len().div_ceil(workers);
    let mut bounds = Vec::with_capacity(workers);
    let mut chunk_start = 0;
    while chunk_start < text.len() {
        let chunk_end = ceil_char_boundary(text, chunk_start + chunk_len);
        bounds.push((chunk_start, chunk_end));
        chunk_start = chunk_end;
    }
    crate::parallel::par_map(&bounds, |&(start, end)| {
        find_in_window(text, start, end, &finder)
    })
    .into_iter()
    .flatten()
    .collect()
}

/// Matches whose start lies in `start..end`; the scanned window extends past `end`
/// far enough for a match that straddles the chunk border.
fn find_in_window(
    text: &str,
    start: usize,
    end: usize,
    finder: &memchr::memmem::Finder,
) -> Vec<(usize, usize)> {
    let needle_len = finder.needle().len();
    let window_end = ceil_char_boundary(text, end + needle_len.saturating_sub(1));
    let mut haystack = Vec::with_capacity(window_end - start);
    fold_case_into(&text[start..window_end], &mut haystack);

    let mut matches = Vec::new();
    let mut position = 0;
    while let Some(relative) = finder.find(&haystack[position..]) {
        let match_start = position + relative;
        if start + match_start >= end {
            break;
        }
        matches.push((start + match_start, start + match_start + needle_len));
        // Advance one character so overlapping matches ("ll" in "llll") are kept.
        position = match_start + utf8_char_len(haystack[match_start]);
    }
    matches
}

/// Lowercases while keeping every character's UTF-8 length, so byte offsets in
/// the folded copy are valid offsets in the original text. Characters whose
/// lowercase form has a different length (rare, e.g. `İ`) are left unchanged.
fn fold_case_into(text: &str, out: &mut Vec<u8>) {
    out.clear();
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte.is_ascii() {
            out.push(byte.to_ascii_lowercase());
            index += 1;
            continue;
        }
        let ch = text[index..].chars().next().expect("char at boundary");
        let len = ch.len_utf8();
        let mut lower = ch.to_lowercase();
        match (lower.next(), lower.next()) {
            (Some(folded), None) if folded.len_utf8() == len => {
                let mut buf = [0u8; 4];
                out.extend_from_slice(folded.encode_utf8(&mut buf).as_bytes());
            }
            _ => out.extend_from_slice(&bytes[index..index + len]),
        }
        index += len;
    }
}

fn ceil_char_boundary(text: &str, mut index: usize) -> usize {
    if index >= text.len() {
        return text.len();
    }
    while !text.is_char_boundary(index) {
        index += 1;
    }
    index
}

fn utf8_char_len(first_byte: u8) -> usize {
    match first_byte {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        _ => 4,
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::{SearchState, find_matches};
    use crate::editor_buffer::EditorBuffer;

    const LOREM_FIXTURE: &str = include_str!("../test-fixtures/lorem-find.md");

    #[test]
    fn finds_case_insensitive_matches() {
        assert_eq!(
            find_matches("lorem Lorem LOREM", "lorem"),
            vec![(0, 5), (6, 11), (12, 17)]
        );
    }

    #[test]
    fn finds_repeated_single_character_matches() {
        assert_eq!(
            find_matches("llll", "l"),
            vec![(0, 1), (1, 2), (2, 3), (3, 4)]
        );
    }

    #[test]
    fn empty_query_has_no_matches() {
        assert!(find_matches("hello", "").is_empty());
    }

    #[test]
    fn search_state_tracks_selected_match() {
        let state = SearchState::new("lo".to_string(), "hello lorem", 42);

        assert_eq!(state.selected_match(), Some((3, 5)));
        assert_eq!(state.buffer_revision, 42);
    }

    #[test]
    fn fixture_finds_lorem_case_insensitively() {
        let lower = find_matches(LOREM_FIXTURE, "lorem");
        let upper = find_matches(LOREM_FIXTURE, "LOREM");
        let title = find_matches(LOREM_FIXTURE, "Lorem");

        assert_eq!(lower.len(), 24);
        assert_eq!(lower, upper);
        assert_eq!(lower, title);
    }

    #[test]
    fn fixture_handles_single_letter_queries() {
        let matches = find_matches(LOREM_FIXTURE, "l");

        assert!(matches.len() > 100);
        assert_eq!(matches.first().copied(), Some((2, 3)));
    }

    #[test]
    fn fixture_returns_no_matches_for_missing_query() {
        assert!(find_matches(LOREM_FIXTURE, "definitely-not-in-this-fixture").is_empty());
    }

    #[test]
    fn fixture_offsets_map_to_line_and_column() {
        let matches = find_matches(LOREM_FIXTURE, "needle-target");
        assert_eq!(matches.len(), 1);

        let buffer = EditorBuffer::from_text(LOREM_FIXTURE.to_string());
        assert_eq!(buffer.byte_to_line_col(matches[0].0), (37, 29));
        assert_eq!(buffer.byte_to_line_col(matches[0].1), (37, 42));
    }

    #[test]
    fn fixture_finds_unicode_adjacent_matches() {
        let matches = find_matches(LOREM_FIXTURE, "lorem 😀 lorem");
        assert_eq!(matches.len(), 1);

        let buffer = EditorBuffer::from_text(LOREM_FIXTURE.to_string());
        assert_eq!(buffer.byte_to_line_col(matches[0].0), (32, 7));
    }

    #[test]
    fn folds_non_ascii_case_with_exact_offsets() {
        let text = "İstanbul ÑANDÚ ñandú";
        assert_eq!(find_matches(text, "ñandú"), vec![(10, 17), (18, 25)]);
        assert_eq!(find_matches(text, "stanbul"), vec![(2, 9)]);
    }

    #[test]
    fn parallel_search_matches_sequential_across_chunk_borders() {
        let unit = "abcab ñandú 😀 lorem\n";
        let text = unit.repeat(40_000);
        for query in ["ab", "b ñ", "lorem\nabc", "😀", "a"] {
            let expected = naive_matches(&text, query);
            assert_eq!(find_matches(&text, query), expected, "query {query:?}");
        }
    }

    fn naive_matches(text: &str, query: &str) -> Vec<(usize, usize)> {
        let mut folded_text = Vec::new();
        let mut folded_query = Vec::new();
        super::fold_case_into(text, &mut folded_text);
        super::fold_case_into(query, &mut folded_query);
        (0..folded_text.len())
            .filter(|&i| text.is_char_boundary(i) && folded_text[i..].starts_with(&folded_query))
            .map(|i| (i, i + folded_query.len()))
            .collect()
    }

    #[test]
    fn search_performance_smoke_test_for_common_queries() {
        let big_text = LOREM_FIXTURE.repeat(200);
        let start = Instant::now();
        let matches = find_matches(&big_text, "l");
        let elapsed = start.elapsed();

        assert!(!matches.is_empty());
        assert!(
            elapsed < Duration::from_millis(500),
            "single-letter search took {elapsed:?}"
        );
    }
}

#[cfg(test)]
mod benchmarks {
    /// The pre-parallel implementation, kept only to compare timings.
    fn legacy_find_matches(text: &str, query: &str) -> Vec<(usize, usize)> {
        let text_lower = text.to_lowercase();
        let query_lower = query.to_lowercase();
        let mut matches = Vec::new();
        let mut search_start = 0;
        while search_start <= text_lower.len() {
            let Some(relative) = text_lower[search_start..].find(&query_lower) else {
                break;
            };
            let start = search_start + relative;
            matches.push((start, start + query_lower.len()));
            search_start = text[start..]
                .chars()
                .next()
                .map(|ch| start + ch.len_utf8())
                .unwrap_or(start + query_lower.len());
        }
        matches
    }

    #[test]
    #[ignore = "manual release benchmark; reads D:/slate-baselines/large-document-5m.md"]
    fn benchmark_find_large_document() {
        let text = std::fs::read_to_string("D:/slate-baselines/large-document-5m.md").unwrap();
        for query in ["e", "lorem", "SLATE_BASELINE_NEEDLE_MIDDLE", "café"] {
            let best = |f: &dyn Fn() -> usize| {
                (0..5)
                    .map(|_| {
                        let started = std::time::Instant::now();
                        let count = f();
                        (started.elapsed(), count)
                    })
                    .min()
                    .unwrap()
            };
            let (legacy, legacy_count) = best(&|| legacy_find_matches(&text, query).len());
            let (current, count) = best(&|| super::find_matches(&text, query).len());
            println!(
                "find {query:?}: legacy {legacy:?} ({legacy_count}) -> parallel {current:?} ({count})"
            );
        }
    }
}
