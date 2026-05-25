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

#[allow(dead_code)]
pub fn find_matches(text: &str, query: &str) -> Vec<(usize, usize)> {
    if query.is_empty() {
        return Vec::new();
    }

    let text_lower = text.to_lowercase();
    let query_lower = query.to_lowercase();
    let mut matches = Vec::new();
    let mut search_start = 0;

    while search_start <= text_lower.len() {
        let Some(relative) = text_lower[search_start..].find(&query_lower) else {
            break;
        };
        let start = search_start + relative;
        let end = start + query_lower.len();
        matches.push((start, end));
        search_start = text[start..]
            .chars()
            .next()
            .map(|ch| start + ch.len_utf8())
            .unwrap_or(end);
    }

    matches
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
