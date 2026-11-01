//! What Flow file updates write to the daily log.
use super::*;

fn summary(text: &str) -> flow::FlowIndexSummary {
    flow::FlowIndexSummary {
        project_root: PathBuf::from("C:/notes"),
        index_path: PathBuf::new(),
        sqlite_path: PathBuf::new(),
        known_file: flow::FlowKnownFile {
            path: "tasks.md".into(),
            last_seen_unix: 0,
            mtime_unix: 0,
            task_count: 0,
            has_tasks: true,
            has_links: false,
            has_tags: false,
        },
        tasks: flow::parse_flow_tasks(text),
    }
}

/// The agenda items the index held for `text`.
fn items(path: &Path, text: &str) -> Vec<AgendaItem> {
    flow::parse_flow_tasks(text)
        .into_iter()
        .map(|task| AgendaItem {
            kind: "task".into(),
            path: path.to_path_buf(),
            relative_path: "tasks.md".into(),
            line_number: task.line_number,
            line_index: task.line_index,
            state: if task.state == CheckboxState::Done {
                "done".into()
            } else {
                "open".into()
            },
            title: task.title,
            tags: task.tags,
            projects: task.projects,
            priority: None,
            due: None,
            scheduled: None,
            heading: None,
        })
        .collect()
}

fn logged(before: &str, after: &str) -> Vec<(String, String)> {
    let path = Path::new("C:/notes/tasks.md");
    flow_daily_events(path, &items(path, before), &summary(after))
        .into_iter()
        .map(|event| (event.kind.section().to_string(), event.title))
        .collect()
}

#[test]
fn moving_tasks_down_does_not_log_them_again() {
    let before = "- [ ] comprar pan\n- [ ] llamar\n";
    let after = "# Nuevo título\n\nnota suelta\n- [ ] comprar pan\n- [ ] llamar\n";
    assert_eq!(logged(before, after), vec![]);
}

#[test]
fn new_and_completed_tasks_are_logged_once() {
    let before = "- [ ] comprar pan\n- [ ] llamar\n";
    let after = "- [ ] regar plantas\n- [x] comprar pan\n- [ ] llamar\n";
    let events = logged(before, after);
    assert_eq!(events.len(), 2, "{events:?}");
    assert!(events.iter().any(|(_, title)| title == "regar plantas"));
    assert!(
        events
            .iter()
            .any(|(section, title)| title == "comprar pan" && *section != events[0].0)
    );
}

#[test]
fn a_task_edited_in_place_is_not_a_new_capture() {
    let before = "- [ ] com\n- [ ] llamar\n";
    let after = "- [ ] comprar pan\n- [ ] llamar\n";
    assert_eq!(logged(before, after), vec![]);
}
