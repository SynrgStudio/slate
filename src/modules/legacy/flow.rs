// ---- from main.rs: fn schedule_current_flow_buffer_agenda_sync_later
    fn schedule_current_flow_buffer_agenda_sync_later(&mut self) {
        self.pending_flow_agenda_sync_at = Some(0.0);
    }


// ---- from main.rs: fn queue_flow_index_after_save
    fn queue_flow_index_after_save(&mut self, path: &Path) {
        let text = self.buffer.as_str().to_string();
        self.queue_flow_index_text(path, text);
    }


// ---- from main.rs: fn queue_flow_index_text
    fn queue_flow_index_text(&mut self, path: &Path, text: String) {
        if !flow::should_index_flow_path(path) {
            return;
        }
        let project_root = Self::flow_project_root_for_path(path);
        let Ok(sqlite_path) = Self::flow_sqlite_path_for_root(&project_root) else {
            return;
        };
        self.flow_worker
            .save_index(project_root, sqlite_path, path.to_path_buf(), text);
    }


// ---- from main.rs: fn queue_flow_agenda_sync
    fn queue_flow_agenda_sync(&mut self) {
        let Some(path) = self.path.clone() else {
            return;
        };
        if !flow::should_index_flow_path(&path) {
            return;
        }
        let project_root = Self::flow_project_root_for_path(&path);
        let Ok(sqlite_path) = Self::flow_sqlite_path_for_root(&project_root) else {
            return;
        };
        self.flow_worker.agenda_sync(
            project_root,
            sqlite_path,
            path,
            self.buffer.as_str().to_string(),
            self.agenda_open,
        );
    }


// ---- from main.rs: fn poll_flow_writer
    fn poll_flow_writer(&mut self, ctx: &egui::Context) {
        for report in self.flow_worker.take_reports() {
            match report {
                flow_worker::Report::SaveIndex { path, result } => match result {
                    Ok(summary) => {
                        let saved = format!("Saved {}", path.display());
                        if self.status == saved {
                            self.status =
                                format!("{saved} · indexed {} Flow tasks", summary.tasks.len());
                        }
                    }
                    Err(err) => {
                        perf_log::mark("flow.save_cache_update_failed", &err);
                        if flow_store::is_busy_error(&err) && self.path.as_ref() == Some(&path) {
                            self.schedule_current_flow_buffer_agenda_sync_later();
                        }
                    }
                },
                flow_worker::Report::AgendaSync { path, result } => {
                    if self.path.as_ref() != Some(&path) {
                        continue;
                    }
                    match result {
                        Ok(synced) => {
                            if let Some(err) = synced.daily_error {
                                self.status = err;
                            }
                            if let (true, Some(mut items)) = (self.agenda_open, synced.items) {
                                self.overlay_current_buffer_agenda_items(&mut items);
                                self.agenda_items = items;
                                self.clamp_agenda_selection();
                            }
                            self.show_agenda_updated_toast(ctx);
                        }
                        Err(err) if flow_store::is_busy_error(&err) => {
                            self.pending_flow_agenda_sync_at = Some(ctx.input(|i| i.time) + 0.15);
                            ctx.request_repaint_after(std::time::Duration::from_millis(160));
                            perf_log::mark("flow.ui_sync.retry", &err);
                            self.status = "Flow sync pending…".to_string();
                        }
                        Err(err) => self.status = format!("Flow agenda update failed: {err}"),
                    }
                }
            }
        }
    }


// ---- from main.rs: fn refresh_flow_index_for_path
    fn refresh_flow_index_for_path(
        &self,
        path: &Path,
        contents: &str,
    ) -> Result<Option<flow::FlowIndexSummary>, String> {
        if !flow::should_index_flow_path(path) {
            return Ok(None);
        }
        let project_root = Self::flow_project_root_for_path(path);
        let sqlite_path = Self::flow_sqlite_path_for_root(&project_root)?;
        flow::update_flow_index_for_file(&project_root, &sqlite_path, path, contents).map(Some)
    }


// ---- from main.rs: fn flow_project_root_for_path
    fn flow_project_root_for_path(path: &Path) -> PathBuf {
        let mut dir = path
            .parent()
            .map(PathBuf::from)
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."));
        loop {
            if dir.join(".git").is_dir() {
                return dir;
            }
            if !dir.pop() {
                return path
                    .parent()
                    .map(PathBuf::from)
                    .or_else(|| std::env::current_dir().ok())
                    .unwrap_or_else(|| PathBuf::from("."));
            }
        }
    }


// ---- from main.rs: fn flow_sqlite_path_for_root
    fn flow_sqlite_path_for_root(root: &Path) -> Result<PathBuf, String> {
        let Some(mut sqlite_path) = dirs_next::data_dir() else {
            return Err("Flow index failed: no data dir".to_string());
        };
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        root.display().to_string().hash(&mut hasher);
        sqlite_path.push("slate");
        sqlite_path.push("flow");
        sqlite_path.push(format!("scope-{:016x}", hasher.finish()));
        sqlite_path.push("agenda.db");
        Ok(sqlite_path)
    }


// ---- from main.rs: fn flow_sqlite_path
    fn flow_sqlite_path(&self) -> Result<PathBuf, String> {
        Self::flow_sqlite_path_for_root(&self.flow_scope_root())
    }


// ---- from main.rs: fn flow_scope_root
    fn flow_scope_root(&self) -> PathBuf {
        self.path
            .as_ref()
            .map(|path| Self::flow_project_root_for_path(path))
            .or_else(|| self.detect_project_root())
            .unwrap_or_else(|| self.project_root())
    }


// ---- from main.rs: fn collect_flow_markdown_files
    fn collect_flow_markdown_files(root: &Path, out: &mut Vec<PathBuf>, limit: usize) {
        if out.len() >= limit {
            return;
        }
        let Ok(entries) = fs::read_dir(root) else {
            return;
        };
        for entry in entries.filter_map(Result::ok) {
            if out.len() >= limit {
                return;
            }
            let path = entry.path();
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            if path.is_dir() {
                if file_dialogs::default_should_skip_dir(name) {
                    continue;
                }
                Self::collect_flow_markdown_files(&path, out, limit);
            } else if path.is_file() && flow::should_index_flow_path(&path) {
                out.push(path);
            }
        }
    }


// ---- from main.rs: fn start_flow_background_rescan
    #[cfg(test)]
    fn start_flow_background_rescan(&mut self, root: PathBuf) {
        self.start_flow_background_scan(root, false);
    }


// ---- from main.rs: fn start_flow_background_scan
    fn start_flow_background_scan(&mut self, root: PathBuf, rebuild: bool) {
        self.start_flow_background_scan_with_live(root, rebuild, None);
    }


// ---- from main.rs: fn index_live_flow_buffer
    /// Indexes one in-memory buffer off the UI thread, retrying briefly while
    /// another writer holds the SQLite lock.
    fn index_live_flow_buffer(root: &Path, sqlite_path: &Path, path: &Path, contents: &str) {
        let mut timing = perf_log::Span::new("flow.live_index", &format!("path={path:?}"));
        for attempt in 0..5 {
            match flow::update_flow_index_for_file(root, sqlite_path, path, contents) {
                Ok(_) => break,
                Err(err) if flow_store::is_busy_error(&err) && attempt < 4 => {
                    thread::sleep(std::time::Duration::from_millis(100));
                }
                Err(err) => {
                    perf_log::mark("flow.open_cache_update_failed", &err);
                    break;
                }
            }
        }
        timing.step("indexed");
    }


// ---- from main.rs: fn start_flow_background_scan_with_live
    fn start_flow_background_scan_with_live(
        &mut self,
        root: PathBuf,
        rebuild: bool,
        live: Option<(PathBuf, String)>,
    ) {
        let Ok(sqlite_path) = Self::flow_sqlite_path_for_root(&root) else {
            return;
        };
        if self.flow_background_loading && self.flow_background_root.as_ref() == Some(&root) {
            if let Some((path, contents)) = live {
                thread::spawn(move || {
                    Self::index_live_flow_buffer(&root, &sqlite_path, &path, &contents)
                });
            }
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.flow_background_rx = Some(rx);
        self.flow_background_root = Some(root.clone());
        self.flow_background_loading = true;
        let protected = self.path.iter().cloned().collect::<Vec<_>>();
        thread::spawn(move || {
            if let Some((path, contents)) = live {
                Self::index_live_flow_buffer(&root, &sqlite_path, &path, &contents);
            }
            let mut timing =
                perf_log::Span::new("flow.background_rescan", &format!("root={root:?}"));
            let mut files = Vec::new();
            Self::collect_flow_markdown_files(&root, &mut files, 20_000);
            timing.step("collect_markdown_files");
            let summary = flow_batch::index_files_with_rebuild(
                &root,
                &sqlite_path,
                &files,
                &protected,
                rebuild,
            )
            .unwrap_or_else(|err| flow_batch::ScanSummary {
                errors: 1,
                first_error: Some(err),
                ..Default::default()
            });
            timing.step("read_and_index_files");
            perf_log::mark(
                "flow.background_rescan.result",
                &format!(
                    "root={root:?} indexed_files={} indexed_tasks={} errors={} batches={} skipped_live={}",
                    summary.indexed_files,
                    summary.indexed_tasks,
                    summary.errors,
                    summary.batches,
                    summary.skipped_live
                ),
            );
            let _ = tx.send(FlowBackgroundResult {
                root,
                indexed_files: summary.indexed_files,
                indexed_tasks: summary.indexed_tasks,
                errors: summary.errors,
                first_error: summary.first_error,
            });
        });
    }


// ---- from main.rs: fn poll_flow_background_rescan
    fn poll_flow_background_rescan(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.flow_background_rx else {
            return;
        };
        match rx.try_recv() {
            Ok(result) => {
                perf_log::mark(
                    "flow.background_rescan.ui_received",
                    &format!("root={:?}", result.root),
                );
                self.flow_background_rx = None;
                self.flow_background_root = None;
                self.flow_background_loading = false;
                if self.agenda_open || self.standalone_agenda {
                    if let Ok(items) = self.load_agenda_items() {
                        self.agenda_items = items;
                        self.clamp_agenda_selection();
                        if self.standalone_agenda {
                            self.agenda_open = true;
                            self.focus_editor_once = false;
                        }
                    }
                }
                if let Some(err) = result.first_error.as_ref() {
                    perf_log::mark("flow.background_rescan.failure_reported", err);
                    if result.indexed_files == 0 {
                        flow_error::offer(ctx, err, false);
                        self.status = "Flow no disponible (ver aviso/log)".to_string();
                        return;
                    }
                }
                self.status = format!(
                    "Flow ready: indexed {} files, {} tasks in {}{}",
                    result.indexed_files,
                    result.indexed_tasks,
                    result.root.display(),
                    if result.errors > 0 {
                        format!(", {} errors", result.errors)
                    } else {
                        String::new()
                    }
                );
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.flow_background_rx = None;
                self.flow_background_root = None;
                self.flow_background_loading = false;
                self.status = "Flow background scan failed".to_string();
            }
        }
    }


// ---- from main.rs: fn flow_rescan_current_scope
    fn flow_rescan_current_scope(&mut self, rebuild: bool, _ctx: &egui::Context) {
        if self.dirty {
            self.status = "Save current buffer before Flow rescan".to_string();
            return;
        }
        if self.flow_background_loading {
            self.status = "Flow scan already in progress".to_string();
            return;
        }
        let root = self.flow_scope_root();
        if self
            .path
            .as_ref()
            .is_some_and(|p| flow::should_index_flow_path(p))
        {
            self.schedule_current_flow_buffer_agenda_sync_later();
        }
        self.start_flow_background_scan(root, rebuild);
        self.status = if rebuild {
            "Flow rebuild in background…"
        } else {
            "Flow rescan in background…"
        }
        .to_string();
    }


// ---- from main.rs: fn schedule_current_flow_buffer_agenda_sync
    fn schedule_current_flow_buffer_agenda_sync(&mut self, ctx: &egui::Context) {
        let Some(path) = self.path.clone() else {
            return;
        };
        if !flow::should_index_flow_path(&path) {
            return;
        }
        self.pending_flow_agenda_sync_at = Some(ctx.input(|input| input.time) + 0.8);
        ctx.request_repaint_after(std::time::Duration::from_millis(850));
    }


// ---- from main.rs: fn poll_pending_flow_agenda_sync
    fn poll_pending_flow_agenda_sync(&mut self, ctx: &egui::Context) {
        let Some(sync_at) = self.pending_flow_agenda_sync_at else {
            return;
        };
        let now = ctx.input(|input| input.time);
        if now < sync_at {
            let remaining_ms = ((sync_at - now) * 1000.0).max(25.0) as u64;
            ctx.request_repaint_after(std::time::Duration::from_millis(remaining_ms.min(850)));
            return;
        }
        self.pending_flow_agenda_sync_at = None;
        // Debounced typing sync: index, daily events and agenda reload run on
        // the Flow writer thread, so resuming typing never waits on SQLite.
        self.queue_flow_agenda_sync();
    }


// ---- from main.rs: fn sync_current_flow_buffer_to_agenda
    fn sync_current_flow_buffer_to_agenda(&mut self, ctx: &egui::Context) {
        let Some(path) = self.path.clone() else {
            return;
        };
        if !flow::should_index_flow_path(&path) {
            return;
        }
        self.publish_current_flow_buffer_to_agenda(ctx);
    }


// ---- from main.rs: fn publish_current_flow_buffer_to_agenda
    fn publish_current_flow_buffer_to_agenda(&mut self, ctx: &egui::Context) {
        let Some(path) = self.path.clone() else {
            return;
        };
        let old_items = self.load_agenda_items().unwrap_or_default();
        let summary = match self.refresh_flow_index_for_path(&path, self.buffer.as_str()) {
            Ok(summary) => summary,
            Err(err) => {
                if flow_store::is_busy_error(&err) {
                    self.pending_flow_agenda_sync_at = Some(ctx.input(|i| i.time) + 0.15);
                    ctx.request_repaint_after(std::time::Duration::from_millis(160));
                    perf_log::mark("flow.ui_sync.retry", &err);
                    self.status = "Flow sync pending…".to_string();
                } else {
                    self.status = format!("Flow agenda update failed: {err}");
                }
                return;
            }
        };
        if let Some(summary) = summary.as_ref() {
            self.record_daily_events_for_flow_update(&path, &old_items, summary);
        }
        if self.agenda_open {
            if let Ok(mut items) = self.load_agenda_items() {
                self.overlay_current_buffer_agenda_items(&mut items);
                self.agenda_items = items;
                self.clamp_agenda_selection();
            }
        }
        self.show_agenda_updated_toast(ctx);
    }


// ---- from main.rs: fn publish_active_flow_buffer_heartbeat
    fn publish_active_flow_buffer_heartbeat(&mut self) {
        let Some(current_path) = self.path.clone() else {
            return;
        };
        if self.standalone_agenda || !flow::should_index_flow_path(&current_path) {
            return;
        }
        if self
            .flow_heartbeat_attempt
            .as_ref()
            .is_some_and(|(path, last)| {
                path == &current_path && last.elapsed() < std::time::Duration::from_secs(3)
            })
        {
            return;
        }
        self.flow_heartbeat_attempt = Some((current_path.clone(), std::time::Instant::now()));
        let scope = Self::flow_project_root_for_path(&current_path);
        let Ok(sqlite_path) = Self::flow_sqlite_path_for_root(&scope) else {
            return;
        };
        self.flow_worker.heartbeat(sqlite_path, scope, current_path);
    }


// ---- from main.rs: fn flow_item_is_active_in_live_editor
    fn flow_item_is_active_in_live_editor(&self, item: &AgendaItem) -> bool {
        let Ok(sqlite_path) = Self::flow_sqlite_path_for_root(&self.flow_scope_root()) else {
            return false;
        };
        flow_runtime::active_buffer_contains(&sqlite_path, &item.relative_path)
    }


// ---- from main.rs: fn poll_flow_open_requests
    fn poll_flow_open_requests(&mut self) {
        let Some(current_path) = self.path.clone() else {
            return;
        };
        if !flow::should_index_flow_path(&current_path) {
            return;
        }
        let scope = Self::flow_project_root_for_path(&current_path);
        let Ok(sqlite_path) = Self::flow_sqlite_path_for_root(&scope) else {
            return;
        };
        let Some((requests_path, requests, remaining)) =
            flow_runtime::drain_open_requests_for_path(&sqlite_path, &scope, &current_path)
        else {
            return;
        };
        let opened_line = requests.last().map(|request| {
            request
                .line_index
                .min(self.buffer.line_count().saturating_sub(1))
        });
        flow_runtime::finish_request_drain(&requests_path, remaining);
        if let Some(line) = opened_line {
            self.buffer.set_cursor(self.buffer.line_end(line));
            self.editor_view.request_scroll_to_cursor(&self.buffer);
            self.focus_editor_once = true;
            self.status = format!("Agenda jumped to open buffer line {}", line + 1);
        }
    }


// ---- from main.rs: fn poll_flow_completion_requests
    fn poll_flow_completion_requests(&mut self, ctx: &egui::Context) {
        let Some(current_path) = self.path.clone() else {
            return;
        };
        if !flow::should_index_flow_path(&current_path) {
            return;
        }
        let scope = Self::flow_project_root_for_path(&current_path);
        let Ok(sqlite_path) = Self::flow_sqlite_path_for_root(&scope) else {
            return;
        };
        let Some((requests_path, requests, mut remaining)) =
            flow_runtime::drain_completion_requests_for_path(&sqlite_path, &scope, &current_path)
        else {
            return;
        };
        let mut applied = false;
        for request in requests {
            if self.apply_flow_completion_to_buffer(request.line_index, &request.title) {
                applied = true;
            } else {
                remaining.push(request.raw);
            }
        }
        flow_runtime::finish_request_drain(&requests_path, remaining);
        if applied {
            self.dirty = true;
            self.search_state = None;
            self.publish_current_flow_buffer_to_agenda(ctx);
        }
    }


// ---- from main.rs: fn poll_flow_organization_requests
    fn poll_flow_organization_requests(&mut self, ctx: &egui::Context) {
        let Some(current_path) = self.path.clone() else {
            return;
        };
        if !flow::should_index_flow_path(&current_path) {
            return;
        }
        let scope = Self::flow_project_root_for_path(&current_path);
        let Ok(sqlite_path) = Self::flow_sqlite_path_for_root(&scope) else {
            return;
        };
        let Some((requests_path, requests, mut remaining)) =
            flow_runtime::drain_organization_requests_for_path(&sqlite_path, &scope, &current_path)
        else {
            return;
        };
        let mut applied = false;
        for request in requests {
            if self.apply_flow_token_to_buffer(request.line_index, &request.value) {
                applied = true;
            } else {
                remaining.push(request.raw);
            }
        }
        flow_runtime::finish_request_drain(&requests_path, remaining);
        if applied {
            self.dirty = true;
            self.search_state = None;
            self.publish_current_flow_buffer_to_agenda(ctx);
        }
    }


// ---- from main.rs: fn poll_flow_edit_requests
    fn poll_flow_edit_requests(&mut self, ctx: &egui::Context) {
        let Some(current_path) = self.path.clone() else {
            return;
        };
        if !flow::should_index_flow_path(&current_path) {
            return;
        }
        let scope = Self::flow_project_root_for_path(&current_path);
        let Ok(sqlite_path) = Self::flow_sqlite_path_for_root(&scope) else {
            return;
        };
        let Some((requests_path, requests, mut remaining)) =
            flow_runtime::drain_edit_requests_for_path(&sqlite_path, &scope, &current_path)
        else {
            return;
        };
        let mut applied = false;
        for request in requests {
            if self.apply_flow_line_edit_to_buffer(request.line_index, &request.value) {
                applied = true;
            } else {
                remaining.push(request.raw);
            }
        }
        flow_runtime::finish_request_drain(&requests_path, remaining);
        if applied {
            self.dirty = true;
            self.search_state = None;
            self.publish_current_flow_buffer_to_agenda(ctx);
        }
    }


// ---- from main.rs: fn apply_flow_line_edit_to_buffer
    fn apply_flow_line_edit_to_buffer(&mut self, line_index: usize, replacement: &str) -> bool {
        if line_index >= self.buffer.line_count() {
            return false;
        }
        let line_start = self.buffer.line_start(line_index);
        let line_end = self.buffer.line_end(line_index);
        self.buffer
            .replace_selection_or_range(line_start, line_end, replacement);
        true
    }


// ---- from main.rs: fn apply_flow_completion_to_buffer
    fn apply_flow_completion_to_buffer(&mut self, line_index: usize, title: &str) -> bool {
        let candidate_indices = [line_index, line_index.saturating_sub(1), line_index + 1];
        for index in candidate_indices {
            let line = self.buffer.line(index);
            let Some(parsed) = parse_checkbox_line(line) else {
                continue;
            };
            if parsed.state == CheckboxState::Done {
                return true;
            }
            let Some(task) = flow::parse_flow_task_line(line, index, None) else {
                continue;
            };
            if task.title != title {
                continue;
            }
            let line_start = self.buffer.line_start(index);
            let marker_start = line_start + parsed.indent.len() + parsed.task_prefix.len();
            let marker_end = marker_start + parsed.marker.len();
            self.buffer
                .replace_selection_or_range(marker_start, marker_end, "[x]");
            return true;
        }
        false
    }


// ---- from main.rs: struct FlowBackgroundResult
struct FlowBackgroundResult {
    root: PathBuf,
    indexed_files: usize,
    indexed_tasks: usize,
    errors: usize,
    first_error: Option<String>,
}

