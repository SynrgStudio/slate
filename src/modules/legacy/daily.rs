// ---- from main.rs: fn open_daily_with_target
    fn open_daily_with_target(&mut self, raw_target: &str) {
        if self.dirty {
            self.pending_daily_target = Some(raw_target.to_string());
            self.confirm(PendingAction::OpenDaily);
            return;
        }
        let target = match daily::parse_open_target(raw_target) {
            Ok(target) => target,
            Err(err) => {
                self.status = err;
                return;
            }
        };
        let date = daily::resolve_open_date(&target);
        let path = match daily::ensure_monthly_document(date) {
            Ok(path) => path,
            Err(err) => {
                self.status = err;
                return;
            }
        };
        let refreshing = matches!(target, daily::DailyOpenTarget::Refresh);
        self.request_file_open(path, OpenDestination::Daily { date, refreshing });
    }


// ---- from main.rs: fn jump_to_daily_day
    fn jump_to_daily_day(&mut self, date: chrono::NaiveDate) {
        let Some(byte) = daily::day_heading_byte(self.buffer.as_str(), date) else {
            return;
        };
        self.buffer.set_cursor(byte);
        self.editor_view.request_scroll_to_cursor(&self.buffer);
    }


// ---- from main.rs: fn record_daily_events_for_flow_update
    fn record_daily_events_for_flow_update(
        &mut self,
        path: &Path,
        old_items: &[AgendaItem],
        summary: &flow::FlowIndexSummary,
    ) {
        if let Err(err) = record_flow_daily_events(path, old_items, summary) {
            self.status = err;
        }
    }


// ---- from main.rs: fn record_flow_daily_events
/// Appends Captured/Completed daily events for tasks that are new or newly
/// done in `summary` compared with the cached `old_items`.
pub(crate) fn record_flow_daily_events(
    path: &Path,
    old_items: &[AgendaItem],
    summary: &flow::FlowIndexSummary,
) -> Result<(), String> {
    for event in flow_daily_events(path, old_items, summary) {
        daily::append_event(&event)?;
    }
    Ok(())
}


// ---- from main.rs: fn flow_daily_events
/// What a Flow file update means for the daily log: tasks captured (new)
/// and completed.
pub(crate) fn flow_daily_events(
    path: &Path,
    old_items: &[AgendaItem],
    summary: &flow::FlowIndexSummary,
) -> Vec<DailyEvent> {
    let mut events = Vec::new();
    let relative_path = summary.known_file.path.clone();
    // Items are matched by title, not line: inserting a line above a task
    // must not log it again. A changed title on a line that lost its old
    // item is an edit of that item, not a new capture.
    let mut old_by_title: std::collections::HashMap<&str, Vec<&AgendaItem>> =
        std::collections::HashMap::new();
    for item in old_items.iter().filter(|item| item.path == path) {
        old_by_title
            .entry(item.title.as_str())
            .or_default()
            .push(item);
    }
    let mut unmatched = Vec::new();
    let mut matched = Vec::new();
    for task in &summary.tasks {
        match old_by_title
            .get_mut(task.title.as_str())
            .and_then(|items| items.pop())
        {
            Some(old) => matched.push((task, Some(old))),
            None => unmatched.push(task),
        }
    }
    let leftover: std::collections::HashMap<usize, &AgendaItem> = old_by_title
        .into_values()
        .flatten()
        .map(|item| (item.line_index, item))
        .collect();
    for task in unmatched {
        matched.push((task, leftover.get(&task.line_index).copied()));
    }
    matched.sort_by_key(|(task, _)| task.line_index);

    for (task, old) in matched {
        let source = format!("{}:{}", relative_path, task.line_number);
        let source_link = Some(format!("{}#L{}", path.display(), task.line_number));
        let event_kind = if old.is_none() {
            Some(DailyEventKind::Captured)
        } else if task.kind == flow::FlowItemKind::Task
            && old.map(|item| item.state.as_str()) != Some("done")
            && task.state == CheckboxState::Done
        {
            Some(DailyEventKind::Completed)
        } else {
            None
        };

        let Some(kind) = event_kind else {
            continue;
        };
        let event = DailyEvent::now(
            kind,
            task.title.clone(),
            task.tags.clone(),
            task.projects.clone(),
            source,
            source_link,
        );
        events.push(event);
    }
    events
}

