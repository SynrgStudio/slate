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
    use super::{SearchState, find_matches};

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
}
