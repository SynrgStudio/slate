use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use chrono::{Datelike, Duration, Local, NaiveDate};
use eframe::egui::{self, Color32, FontId, Vec2};

use crate::{flow, markdown::CheckboxState};

#[derive(Clone)]
pub(crate) struct AgendaItem {
    pub(crate) kind: String,
    pub(crate) path: PathBuf,
    pub(crate) relative_path: String,
    pub(crate) line_number: usize,
    pub(crate) line_index: usize,
    pub(crate) state: String,
    pub(crate) title: String,
    pub(crate) tags: Vec<String>,
    pub(crate) projects: Vec<String>,
    pub(crate) priority: Option<String>,
    pub(crate) due: Option<NaiveDate>,
    pub(crate) scheduled: Option<NaiveDate>,
    pub(crate) heading: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgendaFocus {
    Dated,
    Inbox,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgendaTab {
    Agenda,
    Inbox,
    Tasks,
    Ideas,
    Notes,
}

impl AgendaTab {
    pub(crate) const ALL: [AgendaTab; 5] = [
        AgendaTab::Agenda,
        AgendaTab::Inbox,
        AgendaTab::Tasks,
        AgendaTab::Ideas,
        AgendaTab::Notes,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            AgendaTab::Agenda => "Agenda",
            AgendaTab::Inbox => "Inbox",
            AgendaTab::Tasks => "Tasks",
            AgendaTab::Ideas => "Ideas",
            AgendaTab::Notes => "Notes",
        }
    }

    pub(crate) fn index(self) -> usize {
        match self {
            AgendaTab::Agenda => 0,
            AgendaTab::Inbox => 1,
            AgendaTab::Tasks => 2,
            AgendaTab::Ideas => 3,
            AgendaTab::Notes => 4,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgendaDatedRow {
    Day(NaiveDate),
    Task(usize),
}

pub(crate) fn item_visible(
    item: &AgendaItem,
    tab: AgendaTab,
    filter: &str,
    show_done: bool,
) -> bool {
    item_matches_tab(item, tab)
        && item_matches_filter(item, filter)
        && (show_done || item.state != "done")
}

pub(crate) fn item_matches_tab(item: &AgendaItem, tab: AgendaTab) -> bool {
    match tab {
        AgendaTab::Agenda => true,
        AgendaTab::Inbox => {
            item.tags
                .iter()
                .any(|tag| tag.eq_ignore_ascii_case("inbox"))
                && item.scheduled.is_none()
                && item.due.is_none()
        }
        AgendaTab::Tasks => item.tags.iter().any(|tag| tag.eq_ignore_ascii_case("task")),
        AgendaTab::Ideas => item.tags.iter().any(|tag| tag.eq_ignore_ascii_case("idea")),
        AgendaTab::Notes => {
            item.tags.iter().any(|tag| tag.eq_ignore_ascii_case("note"))
                && item.scheduled.is_none()
                && item.due.is_none()
        }
    }
}

pub(crate) fn state_marker(state: &str) -> &'static str {
    match state {
        "done" => "[x]",
        "doing" => "[/]",
        _ => "[ ]",
    }
}

pub(crate) fn task_line(item: &AgendaItem) -> String {
    let state = match item.kind.as_str() {
        "note" => ":note",
        "idea" => ":idea",
        "inbox" => ":",
        _ => state_marker(&item.state),
    };
    let projects = item
        .projects
        .iter()
        .map(|project| format!("@{project}"))
        .collect::<Vec<_>>()
        .join(" ");
    let tags = item
        .tags
        .iter()
        .map(|tag| format!("#{tag}"))
        .collect::<Vec<_>>()
        .join(" ");
    let priority = item.priority.as_deref().unwrap_or("");
    let date_hint = if let Some(date) = item.scheduled {
        format!(" scheduled {date}")
    } else if let Some(date) = item.due {
        format!(" due {date}")
    } else {
        String::new()
    };
    let context = item
        .heading
        .as_ref()
        .map(|heading| format!(" · {heading}"))
        .unwrap_or_default();
    format!(
        "  {state} {} {} {} {}{}  {}:{}{}",
        item.title,
        projects,
        tags,
        priority,
        date_hint,
        item.relative_path,
        item.line_number,
        context
    )
}

pub(crate) fn day_row(
    ui: &mut egui::Ui,
    row_index: usize,
    date: NaiveDate,
    count: usize,
    expanded: bool,
    is_today: bool,
    selected: bool,
    scroll_to_selected: bool,
    selected_bg: Color32,
    today_bg: Color32,
    font: FontId,
    text: Color32,
    muted: Color32,
    accent: Color32,
) -> egui::Response {
    let row_height = 24.0;
    let (row_rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), row_height),
        egui::Sense::click(),
    );
    let painter = ui.painter_at(row_rect);
    if is_today {
        painter.rect_filled(row_rect, 0.0, today_bg);
    }
    if selected {
        painter.rect_filled(row_rect, 0.0, selected_bg);
        if scroll_to_selected {
            ui.scroll_to_rect(row_rect, Some(egui::Align::Center));
        }
    }

    let cursor = if selected { "▸" } else { " " };
    let fold = if expanded { "▾" } else { "▸" };
    let count_hint = if count > 0 {
        format!("  ● {count}")
    } else {
        String::new()
    };
    let label = format!(
        "{cursor} {fold} {:<10} {:>2} {} {}{}",
        date.format("%A"),
        date.day(),
        date.format("%B"),
        date.year(),
        count_hint,
    );
    painter.text(
        egui::pos2(row_rect.left() + 6.0, row_rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        font,
        if selected || is_today { text } else { muted },
    );
    if count > 0 {
        painter.circle_filled(
            egui::pos2(row_rect.right() - 14.0, row_rect.center().y),
            3.0,
            accent,
        );
    }
    response.on_hover_text(format!("agenda day row {}", row_index + 1))
}

pub(crate) fn task_row(
    ui: &mut egui::Ui,
    index: usize,
    selected: bool,
    scroll_to_selected: bool,
    selected_bg: Color32,
    font: FontId,
    text: Color32,
    accent: Color32,
    item: &AgendaItem,
) -> egui::Response {
    let row_height = 23.0;
    let (row_rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), row_height),
        egui::Sense::click(),
    );
    let painter = ui.painter_at(row_rect);
    if selected {
        painter.rect_filled(row_rect, 0.0, selected_bg);
        if scroll_to_selected {
            ui.scroll_to_rect(row_rect, Some(egui::Align::Center));
        }
    }
    let marker = if selected { "▸" } else { " " };
    painter.text(
        egui::pos2(row_rect.left() + 6.0, row_rect.center().y),
        egui::Align2::LEFT_CENTER,
        marker,
        font.clone(),
        accent,
    );
    painter.text(
        egui::pos2(row_rect.left() + 24.0, row_rect.center().y),
        egui::Align2::LEFT_CENTER,
        task_line(item),
        font,
        if selected {
            Color32::from_rgb(236, 239, 244)
        } else {
            text
        },
    );
    response.on_hover_text(format!("agenda item {}", index + 1))
}

pub(crate) fn from_flow_task(
    path: PathBuf,
    relative_path: String,
    task: flow::FlowTask,
) -> AgendaItem {
    AgendaItem {
        kind: task.kind.as_str().to_string(),
        path,
        relative_path,
        line_number: task.line_number,
        line_index: task.line_index,
        state: match task.kind {
            flow::FlowItemKind::Task => match task.state {
                CheckboxState::Empty => "todo",
                CheckboxState::Doing => "doing",
                CheckboxState::Done => "done",
            },
            flow::FlowItemKind::Note => "note",
            flow::FlowItemKind::Idea => "idea",
            flow::FlowItemKind::Inbox => "inbox",
        }
        .to_string(),
        title: task.title,
        tags: task.tags,
        projects: task.projects,
        priority: task.priority.map(|priority| priority.as_str().to_string()),
        due: task.metadata.due.as_deref().and_then(parse_cache_date),
        scheduled: task
            .metadata
            .scheduled
            .as_deref()
            .and_then(parse_cache_date),
        heading: task.heading,
    }
}

pub(crate) fn load_items_from_cache(
    sqlite_path: &Path,
    root: &Path,
) -> Result<Vec<AgendaItem>, String> {
    if !sqlite_path.exists() {
        return Err("No Flow agenda cache yet. Run :flow-rescan first.".to_string());
    }
    let mut items = crate::flow_store::read_tasks(sqlite_path)?
        .into_iter()
        .map(|row| AgendaItem {
            path: root.join(&row.path), relative_path: row.path,
            line_number: row.line_number, line_index: row.line_index, kind: row.kind,
            state: row.state, title: row.title, tags: split_cache_list(&row.tags),
            projects: split_cache_list(&row.projects), priority: non_empty_string(&row.priority),
            due: parse_cache_date(&row.due), scheduled: parse_cache_date(&row.scheduled),
            heading: non_empty_string(&row.heading),
        })
        .filter(|item| item.path.exists())
        .collect::<Vec<_>>();
    sort_items(&mut items);
    Ok(items)
}

fn split_cache_list(value: &str) -> Vec<String> {
    if value.is_empty() {
        Vec::new()
    } else {
        value.split(',').map(|part| part.to_string()).collect()
    }
}

fn non_empty_string(value: &str) -> Option<String> {
    (!value.is_empty()).then(|| value.to_string())
}

fn parse_cache_date(value: &str) -> Option<NaiveDate> {
    if value.is_empty() {
        None
    } else {
        NaiveDate::parse_from_str(value, "%Y-%m-%d").ok()
    }
}

pub(crate) fn overlay_live_buffer_items(
    items: &mut Vec<AgendaItem>,
    path: &Path,
    root: &Path,
    document: &str,
) {
    let relative_path = path
        .strip_prefix(root)
        .ok()
        .and_then(|path| path.to_str())
        .map(|path| path.replace('\\', "/"))
        .unwrap_or_else(|| path.display().to_string());

    items.retain(|item| item.path != path);
    items.extend(
        flow::parse_flow_tasks(document)
            .into_iter()
            .map(|task| from_flow_task(path.to_path_buf(), relative_path.clone(), task)),
    );
    sort_items(items);
}

pub(crate) fn sort_items(items: &mut [AgendaItem]) {
    items.sort_by_key(|item| {
        (
            item.scheduled.or(item.due).unwrap_or(NaiveDate::MAX),
            item.relative_path.clone(),
            item.line_number,
        )
    });
}

pub(crate) fn visible_item_indices(
    items: &[AgendaItem],
    tab: AgendaTab,
    filter: &str,
    show_done: bool,
) -> Vec<usize> {
    items
        .iter()
        .enumerate()
        .filter(|(_, item)| item_visible(item, tab, filter, show_done))
        .map(|(index, _)| index)
        .collect()
}

pub(crate) fn week_start(offset_days: i64) -> NaiveDate {
    let today = Local::now().date_naive();
    let monday = today - Duration::days(today.weekday().num_days_from_monday() as i64);
    monday + Duration::days(offset_days)
}

pub(crate) fn week_dates(offset_days: i64) -> [NaiveDate; 7] {
    let start = week_start(offset_days);
    std::array::from_fn(|index| start + Duration::days(index as i64))
}

pub(crate) fn item_indices_for_date(
    items: &[AgendaItem],
    tab: AgendaTab,
    filter: &str,
    show_done: bool,
    date: NaiveDate,
) -> Vec<usize> {
    items
        .iter()
        .enumerate()
        .filter(|(_, item)| {
            item_visible(item, tab, filter, show_done) && item.scheduled.or(item.due) == Some(date)
        })
        .map(|(index, _)| index)
        .collect()
}

pub(crate) fn inbox_item_indices(
    items: &[AgendaItem],
    tab: AgendaTab,
    filter: &str,
    show_done: bool,
) -> Vec<usize> {
    items
        .iter()
        .enumerate()
        .filter(|(_, item)| {
            item_visible(item, tab, filter, show_done)
                && item.scheduled.is_none()
                && item.due.is_none()
        })
        .map(|(index, _)| index)
        .collect()
}

pub(crate) fn dated_rows(
    items: &[AgendaItem],
    tab: AgendaTab,
    filter: &str,
    show_done: bool,
    offset_days: i64,
    expanded_dates: &HashSet<NaiveDate>,
) -> Vec<AgendaDatedRow> {
    let mut rows = Vec::new();
    for date in week_dates(offset_days) {
        rows.push(AgendaDatedRow::Day(date));
        if expanded_dates.contains(&date) {
            rows.extend(
                item_indices_for_date(items, tab, filter, show_done, date)
                    .into_iter()
                    .map(AgendaDatedRow::Task),
            );
        }
    }
    rows
}

pub(crate) fn clamp_selection(
    items: &[AgendaItem],
    tab: AgendaTab,
    filter: &str,
    show_done: bool,
    offset_days: i64,
    expanded_dates: &HashSet<NaiveDate>,
    selected_dated_row: &mut usize,
    selected_inbox_item: &mut usize,
    selected_tab_items: &mut [usize; 5],
) {
    let dated_len = dated_rows(items, tab, filter, show_done, offset_days, expanded_dates)
        .len()
        .max(1);
    *selected_dated_row = (*selected_dated_row).min(dated_len - 1);
    let inbox_len = inbox_item_indices(items, tab, filter, show_done).len();
    *selected_inbox_item = (*selected_inbox_item).min(inbox_len.saturating_sub(1));
    let flat_len = visible_item_indices(items, tab, filter, show_done).len();
    let tab_index = tab.index();
    selected_tab_items[tab_index] = selected_tab_items[tab_index].min(flat_len.saturating_sub(1));
}

pub(crate) fn move_selection(
    items: &[AgendaItem],
    tab: AgendaTab,
    focus: AgendaFocus,
    filter: &str,
    show_done: bool,
    offset_days: i64,
    expanded_dates: &HashSet<NaiveDate>,
    selected_dated_row: &mut usize,
    selected_inbox_item: &mut usize,
    selected_tab_items: &mut [usize; 5],
    delta: isize,
) -> bool {
    if tab != AgendaTab::Agenda {
        let indices = visible_item_indices(items, tab, filter, show_done);
        if indices.is_empty() {
            selected_tab_items[tab.index()] = 0;
            return false;
        }
        let tab_index = tab.index();
        selected_tab_items[tab_index] = selected_tab_items[tab_index]
            .saturating_add_signed(delta)
            .min(indices.len().saturating_sub(1));
        return true;
    }

    match focus {
        AgendaFocus::Dated => {
            let rows = dated_rows(items, tab, filter, show_done, offset_days, expanded_dates);
            if rows.is_empty() {
                *selected_dated_row = 0;
                return false;
            }
            *selected_dated_row = (*selected_dated_row)
                .saturating_add_signed(delta)
                .min(rows.len().saturating_sub(1));
        }
        AgendaFocus::Inbox => {
            let indices = inbox_item_indices(items, tab, filter, show_done);
            if indices.is_empty() {
                *selected_inbox_item = 0;
                return false;
            }
            *selected_inbox_item = (*selected_inbox_item)
                .saturating_add_signed(delta)
                .min(indices.len().saturating_sub(1));
        }
    }
    true
}

pub(crate) fn selected_item_index(
    items: &[AgendaItem],
    tab: AgendaTab,
    focus: AgendaFocus,
    filter: &str,
    show_done: bool,
    offset_days: i64,
    expanded_dates: &HashSet<NaiveDate>,
    selected_dated_row: usize,
    selected_inbox_item: usize,
    selected_tab_items: &[usize; 5],
) -> Option<usize> {
    if tab != AgendaTab::Agenda {
        return visible_item_indices(items, tab, filter, show_done)
            .get(selected_tab_items[tab.index()])
            .copied();
    }

    match focus {
        AgendaFocus::Dated => {
            dated_rows(items, tab, filter, show_done, offset_days, expanded_dates)
                .get(selected_dated_row)
                .and_then(|row| match row {
                    AgendaDatedRow::Task(index) => Some(*index),
                    AgendaDatedRow::Day(_) => None,
                })
        }
        AgendaFocus::Inbox => inbox_item_indices(items, tab, filter, show_done)
            .get(selected_inbox_item)
            .copied(),
    }
}

pub(crate) fn item_matches_filter(item: &AgendaItem, filter: &str) -> bool {
    let filter = filter.trim();
    if filter.is_empty() {
        return true;
    }
    let today = Local::now().date_naive();
    filter.split_whitespace().all(|token| {
        let lower = token.to_ascii_lowercase();
        if let Some(tag) = lower.strip_prefix('#') {
            return item
                .tags
                .iter()
                .any(|item_tag| item_tag.eq_ignore_ascii_case(tag));
        }
        if let Some(project) = lower.strip_prefix('@') {
            return item
                .projects
                .iter()
                .any(|item_project| item_project.eq_ignore_ascii_case(project));
        }
        match lower.as_str() {
            "alta" | "media" | "baja" => item
                .priority
                .as_deref()
                .is_some_and(|priority| priority.eq_ignore_ascii_case(&lower)),
            "todo" | "open" => item.state == "todo",
            "doing" => item.state == "doing",
            "done" => item.state == "done",
            "today" => item.scheduled.or(item.due) == Some(today),
            "overdue" => item.scheduled.or(item.due).is_some_and(|date| date < today),
            "due" => item.due.is_some(),
            "scheduled" => item.scheduled.is_some(),
            "inbox" | "nodate" | "no-date" => item.due.is_none() && item.scheduled.is_none(),
            other => {
                item.title.to_ascii_lowercase().contains(other)
                    || item.relative_path.to_ascii_lowercase().contains(other)
                    || item
                        .heading
                        .as_ref()
                        .is_some_and(|heading| heading.to_ascii_lowercase().contains(other))
            }
        }
    })
}
