#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum FlowOrgMode {
    Tag,
    Project,
}

pub(crate) struct FlowItemEditState {
    pub(crate) item_index: usize,
    pub(crate) text: String,
    pub(crate) cursor_char_index: usize,
    pub(crate) org_mode: Option<FlowOrgMode>,
    pub(crate) org_query: String,
    pub(crate) selected_org_suggestion: usize,
    pub(crate) focus_once: bool,
}

impl FlowItemEditState {
    pub(crate) fn new(item_index: usize, text: String) -> Self {
        Self {
            item_index,
            cursor_char_index: text.chars().count(),
            text,
            org_mode: None,
            org_query: String::new(),
            selected_org_suggestion: 0,
            focus_once: true,
        }
    }

    pub(crate) fn open_org(&mut self, mode: FlowOrgMode) {
        self.org_mode = Some(mode);
        self.org_query.clear();
        self.selected_org_suggestion = 0;
    }

    pub(crate) fn close_org(&mut self) {
        self.org_mode = None;
        self.org_query.clear();
        self.selected_org_suggestion = 0;
    }
}

pub(crate) fn organization_suggestions<I>(values: I, query: &str) -> Vec<String>
where
    I: IntoIterator<Item = String>,
{
    let mut values = values.into_iter().collect::<Vec<_>>();
    values.sort_by_key(|value| value.to_ascii_lowercase());
    values.dedup_by(|a, b| a.eq_ignore_ascii_case(b));

    let query = query.trim();
    let query_lower = query.to_ascii_lowercase();
    values.retain(|value| {
        query_lower.is_empty() || value.to_ascii_lowercase().contains(&query_lower)
    });

    if values.is_empty() && !query.is_empty() {
        values.push(query.to_string());
    }

    values
}

pub(crate) fn organization_token(mode: FlowOrgMode, value: &str) -> Option<String> {
    let value = value.trim().trim_start_matches(['#', '@']);
    if value.is_empty() {
        return None;
    }
    Some(match mode {
        FlowOrgMode::Tag => format!("#{value}"),
        FlowOrgMode::Project => format!("@{value}"),
    })
}

pub(crate) fn append_token_to_text(text: &mut String, token: &str) {
    if text
        .split_whitespace()
        .any(|word| word.eq_ignore_ascii_case(token))
    {
        return;
    }
    if !text.is_empty() && !text.ends_with(char::is_whitespace) {
        text.push(' ');
    }
    text.push_str(token);
}

fn char_to_byte_index(text: &str, char_index: usize) -> usize {
    text.char_indices()
        .nth(char_index)
        .map(|(index, _)| index)
        .unwrap_or(text.len())
}

fn is_org_token_char(ch: char) -> bool {
    ch.is_alphanumeric() || matches!(ch, '-' | '_' | '/' | '.')
}

pub(crate) fn org_token_span_at_cursor(
    text: &str,
    cursor_char_index: usize,
    mode: FlowOrgMode,
) -> Option<(std::ops::Range<usize>, String)> {
    let cursor = char_to_byte_index(text, cursor_char_index);
    let prefix = match mode {
        FlowOrgMode::Tag => '#',
        FlowOrgMode::Project => '@',
    };

    let mut token_start = cursor;
    while token_start > 0 {
        let Some((previous, ch)) = text[..token_start].char_indices().next_back() else {
            break;
        };
        if is_org_token_char(ch) {
            token_start = previous;
        } else if ch == prefix {
            token_start = previous;
            break;
        } else {
            break;
        }
    }

    if text[token_start..].chars().next()? != prefix {
        return None;
    }

    if token_start > 0 {
        if let Some((_, before)) = text[..token_start].char_indices().next_back() {
            if !before.is_whitespace() && !matches!(before, '(' | '[' | '{') {
                return None;
            }
        }
    }

    let token_body_start = token_start + prefix.len_utf8();
    if cursor < token_body_start {
        return None;
    }

    let mut token_end = cursor;
    while token_end < text.len() {
        let Some(ch) = text[token_end..].chars().next() else {
            break;
        };
        if is_org_token_char(ch) {
            token_end += ch.len_utf8();
        } else {
            break;
        }
    }

    let query = text[token_body_start..cursor].to_string();
    Some((token_start..token_end, query))
}

pub(crate) fn org_query_at_cursor(
    text: &str,
    cursor_char_index: usize,
    mode: FlowOrgMode,
) -> Option<String> {
    org_token_span_at_cursor(text, cursor_char_index, mode).map(|(_, query)| query)
}

pub(crate) fn replace_org_token_at_cursor(
    text: &mut String,
    cursor_char_index: usize,
    mode: FlowOrgMode,
    token: &str,
) -> usize {
    if let Some((range, _)) = org_token_span_at_cursor(text, cursor_char_index, mode) {
        text.replace_range(range.clone(), token);
        return text[..range.start].chars().count() + token.chars().count();
    }
    let before = text.chars().count();
    append_token_to_text(text, token);
    text.chars().count().max(before)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_org_token_at_cursor_in_middle_of_text() {
        let mut text = "- [ ] tarea @pro #task".to_string();
        let cursor = text.find("@pro").unwrap() + "@pro".len();
        let cursor = text[..cursor].chars().count();
        let next =
            replace_org_token_at_cursor(&mut text, cursor, FlowOrgMode::Project, "@proyectocasa");
        assert_eq!(text, "- [ ] tarea @proyectocasa #task");
        assert_eq!(next, "- [ ] tarea @proyectocasa".chars().count());
    }

    #[test]
    fn appends_when_no_org_token_is_at_cursor() {
        let mut text = "- [ ] tarea".to_string();
        let cursor = text.chars().count();
        replace_org_token_at_cursor(&mut text, cursor, FlowOrgMode::Tag, "#task");
        assert_eq!(text, "- [ ] tarea #task");
    }
}
