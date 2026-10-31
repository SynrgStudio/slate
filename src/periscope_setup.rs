//! First-use consent and a blocking (UI flow, not UI thread) metadata preparation gate.
use super::*;
#[derive(Default)]
pub(crate) struct Setup {
    pub visible: bool,
    inspection: Option<Receiver<(Vec<PathBuf>, Result<bool, String>)>>,
    roots: Vec<PathBuf>,
    preparation: Option<periscope_backend::Preparation>,
    state: State,
}
#[derive(Default)]
enum State {
    #[default]
    Inspecting,
    Consent,
    Indexing,
    Failed(String),
}
impl Setup {
    pub fn hint(&self) -> &'static str {
        match self.state {
            State::Consent => "periscope setup  Y/Enter accept indexing · N/Esc cancel",
            State::Indexing => "periscope setup  wait for cache preparation · Esc cancel",
            State::Failed(_) => "periscope setup  Enter retry indexing · Esc cancel",
            State::Inspecting => "periscope setup  checking prepared index · Esc cancel",
        }
    }
    #[cfg(test)]
    pub fn awaiting_consent(&self) -> bool {
        matches!(self.state, State::Consent)
    }
    fn close(&mut self) {
        self.visible = false;
        self.inspection = None;
        self.preparation = None;
    }
}
impl SlateApp {
    pub(super) fn begin_periscope_setup(&mut self) {
        if self.periscope_setup.visible {
            return;
        }
        self.periscope_engine.cancel();
        self.periscope_request = None;
        self.periscope_open = false;
        self.shortcut_help_open = false;
        self.palette_open = false;
        self.recent_picker_open = false;
        self.lance_open = false;
        self.file_picker_open = false;
        self.save_as_open = false;
        self.settings_open = false;
        self.command_line_focused = false;
        self.focus_command_line_once = false;
        // Existing modal-input suppression also protects the main editor during setup.
        self.periscope_prompt_open = true;
        self.focus_editor_once = false;
        let roots = self.periscope_roots.clone();
        let cache = self.periscope_engine.cache_path();
        let (tx, rx) = mpsc::channel();
        self.periscope_setup = Setup {
            visible: true,
            inspection: Some(rx),
            ..Default::default()
        };
        thread::spawn(move || {
            let roots = if roots.is_empty() {
                periscope_backend::default_roots()
            } else {
                roots
            };
            let result = periscope_store::Store::prepared(&cache, &roots);
            let _ = tx.send((roots, result));
        });
        self.status = "Checking Periscope index…".into();
    }
    pub(super) fn accept_periscope_indexing(&mut self) {
        if !self.periscope_setup.visible
            || !matches!(
                self.periscope_setup.state,
                State::Consent | State::Failed(_)
            )
        {
            return;
        }
        if self.periscope_setup.roots.is_empty() {
            self.periscope_setup.state =
                State::Failed("No search roots available. Configure periscope_root first.".into());
            return;
        }
        self.periscope_setup.preparation = Some(self.periscope_engine.prepare_using(
            self.periscope_setup.roots.clone(),
            self.periscope_everything,
        ));
        self.periscope_setup.state = State::Indexing;
        self.status = "Preparing Periscope metadata index…".into();
    }
    pub(super) fn cancel_periscope_setup(&mut self) {
        self.periscope_setup.close();
        self.periscope_prompt_open = false;
        self.periscope_open = false;
        self.focus_editor_once = true;
        self.status = "Periscope indexing cancelled; cached metadata retained".into();
    }
    pub(super) fn poll_periscope_setup(&mut self, ctx: &egui::Context) {
        if !self.periscope_setup.visible {
            return;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
        if let Some(rx) = &self.periscope_setup.inspection {
            match rx.try_recv() {
                Ok((roots, result)) => {
                    self.periscope_setup.inspection = None;
                    self.periscope_setup.roots = roots;
                    match result {
                        Ok(true) => {
                            self.periscope_setup.close();
                            self.choose_periscope_scope();
                            return;
                        }
                        Ok(false) => self.periscope_setup.state = State::Consent,
                        Err(error) => {
                            self.periscope_setup.state = State::Failed(format!(
                                "Cannot validate existing index: {error}. No cache was deleted."
                            ))
                        }
                    }
                }
                Err(TryRecvError::Disconnected) => {
                    self.periscope_setup.inspection = None;
                    self.periscope_setup.state =
                        State::Failed("Index check failed. Reopen Periscope to retry.".into());
                }
                Err(TryRecvError::Empty) => {}
            }
        }
        if let Some(preparation) = &self.periscope_setup.preparation {
            if let Some(result) = preparation.result() {
                match result {
                    Ok(completed) => {
                        self.periscope_setup.close();
                        self.choose_periscope_scope();
                        self.status = format!(
                            "Periscope index prepared · {} files cached · {} reported scan errors",
                            completed.files, completed.errors
                        );
                    }
                    Err(error) => {
                        self.periscope_setup.preparation = None;
                        self.periscope_setup.state = State::Failed(error);
                    }
                }
            }
        }
    }
    pub(super) fn handle_periscope_setup_input(&mut self, ctx: &egui::Context) {
        let (cancel, accept) = ctx.input_mut(|input| {
            (
                input.consume_key(egui::Modifiers::NONE, Key::Escape)
                    | input.consume_key(egui::Modifiers::NONE, Key::N),
                input.consume_key(egui::Modifiers::NONE, Key::Enter)
                    | input.consume_key(egui::Modifiers::NONE, Key::Y),
            )
        });
        if cancel {
            self.cancel_periscope_setup();
        } else if accept {
            self.accept_periscope_indexing();
        }
    }
    pub(super) fn periscope_setup_dialog(&mut self, ctx: &egui::Context) {
        if !self.periscope_setup.visible {
            return;
        }
        let (title, subtitle, actions) = match &self.periscope_setup.state {
            State::Inspecting => (
                "Periscope",
                "Checking previously prepared scope",
                vec![("Esc", "cancel")],
            ),
            State::Consent => (
                "Prepare Periscope",
                "First use · permission to index selected drives",
                vec![("Y/Enter", "accept and index"), ("N/Esc", "cancel")],
            ),
            State::Indexing => (
                "Preparing Periscope",
                "Wait for indexing to finish before choosing search scope",
                vec![("Esc", "cancel indexing")],
            ),
            State::Failed(_) => (
                "Periscope preparation failed",
                "Search remains closed · existing metadata retained",
                vec![("Enter", "retry indexing"), ("Esc", "cancel")],
            ),
        };
        let clicked = dialog_ui::show(
            ctx,
            "periscope_setup_dialog".into(),
            title,
            subtitle,
            &actions,
            |ui| {
                egui::ScrollArea::vertical().id_salt("preparation_body").auto_shrink([false,false]).min_scrolled_height(1.0).max_height(ui.available_height()).show(ui,|ui| {
                match &self.periscope_setup.state {
                    State::Inspecting=>{ui.label("Reading the preparation marker; no filesystem scan has started.");},
                    State::Consent=>{
                        ui.label("Index file paths, names, sizes and modification times on these roots?");ui.label("Document contents are never read or cached. This can take several minutes and uses disk space for the SQLite metadata index.");ui.add_space(8.0);
                        for root in &self.periscope_setup.roots {ui.label(RichText::new(root.display().to_string()).color(notice_ui::ACCENT));}
                        ui.add_space(8.0);ui.label("Project/global choice and searching are disabled until the scan finishes. Everything supplies its inventory and metadata when enabled and available; otherwise a slower filesystem scan prepares Slate's independent cache.");
                        ui.label("Everything's indexed scope and exclusions apply to imports. The fallback skips protected/inaccessible paths and reparse points. Interrupted imports or database failures block completion.");
                    },
                    State::Indexing=>{ui.spinner();if let Some(preparation)=&self.periscope_setup.preparation {ui.label(preparation.status());}ui.label("No accurate total is known yet, so no invented percentage is shown. Cancel preserves committed metadata and keeps search closed.");},
                    State::Failed(error)=>{ui.label(RichText::new(error).color(notice_ui::WARN));ui.label("Fix unavailable roots or disk/database errors and retry. No search will open with a failed initial preparation.");},
                }
            });
            },
        );
        if matches!(
            self.periscope_setup.state,
            State::Consent | State::Failed(_)
        ) {
            if clicked[0] {
                self.accept_periscope_indexing();
            } else if clicked[1] {
                self.cancel_periscope_setup();
            }
        } else if clicked[0] {
            self.cancel_periscope_setup();
        }
    }
    pub(super) fn choose_periscope_project(&mut self) {
        if self.periscope_setup.visible {
            return;
        }
        if let Some(root) = self.periscope_project_root.clone() {
            self.periscope_prompt_open = false;
            self.periscope_mode = PeriscopeMode::Project;
            self.periscope_open = true;
            self.refresh_periscope_results();
            self.status = format!("Periscope project: {}", root.display());
        } else {
            self.open_global_periscope();
        }
    }
}
