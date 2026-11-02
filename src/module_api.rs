//! The contract between the core and optional modules (see
//! `docs/module-api.md`). The core calls hooks on the enabled modules; a
//! module never touches the core directly, it asks through `Cx` and the
//! core applies the requests after the hook returns.
use crate::commands::CommandSpec;
use eframe::egui;
use std::{
    ops::Range,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

/// An open document, as a module sees it.
#[derive(Clone, Copy)]
pub(crate) struct DocumentRef<'a> {
    /// Stable for the document's lifetime (not reused).
    pub(crate) id: u64,
    pub(crate) path: Option<&'a Path>,
    pub(crate) revision: u64,
    pub(crate) text: &'a str,
}

/// One row of a result list the core shows for a module.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ResultItem {
    pub(crate) path: PathBuf,
    /// 0-based line to open at.
    pub(crate) line: Option<usize>,
    pub(crate) label: String,
    pub(crate) detail: String,
}

/// What a module asks the core to do.
#[derive(Debug, PartialEq)]
pub(crate) enum Request {
    Status(String),
    /// Open a file (in a tab), at a 0-based line if given.
    #[allow(dead_code)] // API for the modules being rebuilt (Flow, Agenda)
    Open {
        path: PathBuf,
        line: Option<usize>,
    },
    /// Show a filterable result list; Enter opens the selected item.
    Results {
        title: String,
        items: Vec<ResultItem>,
    },
}

/// Wakes a module from any thread (a worker that finished).
#[derive(Clone)]
pub(crate) struct Waker {
    flag: Arc<AtomicBool>,
    ctx: egui::Context,
}

impl Waker {
    pub(crate) fn wake(&self) {
        self.flag.store(true, Ordering::Release);
        self.ctx.request_repaint();
    }
}

/// A module's door into the core during one hook call.
pub(crate) struct Cx<'a> {
    pub(crate) vaults: &'a [PathBuf],
    data_dir: PathBuf,
    waker: Waker,
    requests: Vec<Request>,
    wake_at: Option<Instant>,
}

impl Cx<'_> {
    pub(crate) fn status(&mut self, message: impl Into<String>) {
        self.requests.push(Request::Status(message.into()));
    }

    #[allow(dead_code)] // API for the modules being rebuilt (Flow, Agenda)
    pub(crate) fn open(&mut self, path: PathBuf, line: Option<usize>) {
        self.requests.push(Request::Open { path, line });
    }

    pub(crate) fn show_results(&mut self, title: impl Into<String>, items: Vec<ResultItem>) {
        self.requests.push(Request::Results {
            title: title.into(),
            items,
        });
    }

    /// Calls `wake` after `delay` (the earliest request wins).
    #[allow(dead_code)] // API for the modules being rebuilt (Flow, Agenda)
    pub(crate) fn wake_after(&mut self, delay: Duration) {
        let at = Instant::now() + delay;
        self.wake_at = Some(self.wake_at.map_or(at, |old| old.min(at)));
    }

    /// For background threads: calling it makes the core call `wake`.
    pub(crate) fn waker(&self) -> Waker {
        self.waker.clone()
    }

    /// The module's own folder for derived data (never next to the user's
    /// files). Created on demand by the module.
    pub(crate) fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    /// The vault holding `path`: the innermost declared vault, else the
    /// nearest folder with `.git`, else the file's own folder.
    pub(crate) fn vault_for(&self, path: &Path) -> PathBuf {
        crate::vaults::vault_for(self.vaults, path)
    }

    /// The declared vault holding `path` (Settings → Modules & vaults), if
    /// any: for modules that only work inside folders the user chose.
    pub(crate) fn declared_vault(&self, path: &Path) -> Option<PathBuf> {
        crate::vaults::declared(self.vaults, path)
    }
}

/// An optional feature on top of the core. Every hook has a default that
/// does nothing; a module implements what it needs. Hooks must return fast:
/// heavy work goes to the module's own thread (see `Cx::waker`).
pub(crate) trait Module {
    /// Stable id: settings keys (`module.<id>`), data folder, messages.
    fn id(&self) -> &'static str;
    /// Name and one line for Settings → Modules.
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;

    /// Commands it adds to the palette, command line and help.
    fn commands(&self) -> &'static [CommandSpec] {
        &[]
    }
    /// One of its commands ran (`args`: the rest of the command line).
    fn run(&mut self, _cx: &mut Cx, _command: &str, _args: &str, _doc: Option<DocumentRef>) {}

    fn document_opened(&mut self, _cx: &mut Cx, _doc: DocumentRef) {}
    /// `lines`: the lines that may have changed (0-based, end exclusive).
    fn document_edited(&mut self, _cx: &mut Cx, _doc: DocumentRef, _lines: Range<usize>) {}
    /// The active document was edited and then left alone for `IDLE_AFTER`:
    /// the moment to look at its whole text (once per pause, not per key).
    fn document_idle(&mut self, _cx: &mut Cx, _doc: DocumentRef) {}
    fn document_saved(&mut self, _cx: &mut Cx, _doc: DocumentRef) {}
    fn document_closed(&mut self, _cx: &mut Cx, _id: u64) {}
    /// It asked to be woken (`Cx::wake_after`, a `Waker`).
    fn wake(&mut self, _cx: &mut Cx) {}
}

struct Slot {
    module: Box<dyn Module>,
    enabled: bool,
    woken: Arc<AtomicBool>,
    wake_at: Option<Instant>,
}

/// The modules of this window, with their on/off state.
pub(crate) struct Host {
    slots: Vec<Slot>,
}

/// A pause in typing this long makes the core call `document_idle`.
pub(crate) const IDLE_AFTER: Duration = Duration::from_millis(800);

/// Hooks slower than this are written to the performance log.
const SLOW_HOOK: Duration = Duration::from_millis(4);

impl Host {
    pub(crate) fn new(modules: Vec<Box<dyn Module>>) -> Self {
        let host = Self {
            slots: modules
                .into_iter()
                .map(|module| Slot {
                    module,
                    enabled: true,
                    woken: Arc::new(AtomicBool::new(false)),
                    wake_at: None,
                })
                .collect(),
        };
        host.publish_commands();
        host
    }

    /// (id, name, description, enabled) for Settings.
    pub(crate) fn list(&self) -> Vec<(&'static str, &'static str, &'static str, bool)> {
        self.slots
            .iter()
            .map(|slot| {
                (
                    slot.module.id(),
                    slot.module.name(),
                    slot.module.description(),
                    slot.enabled,
                )
            })
            .collect()
    }

    pub(crate) fn set_enabled(&mut self, id: &str, enabled: bool) {
        if let Some(slot) = self.slots.iter_mut().find(|slot| slot.module.id() == id) {
            slot.enabled = enabled;
            slot.wake_at = None;
        }
        self.publish_commands();
    }

    #[allow(dead_code)] // API for the modules being rebuilt (Flow, Agenda)
    pub(crate) fn is_enabled(&self, id: &str) -> bool {
        self.slots
            .iter()
            .any(|slot| slot.module.id() == id && slot.enabled)
    }

    /// The command line, palette and help see the enabled modules' commands.
    fn publish_commands(&self) {
        crate::commands::set_module_commands(
            self.slots
                .iter()
                .filter(|slot| slot.enabled)
                .flat_map(|slot| slot.module.commands().iter())
                .collect(),
        );
    }

    /// The enabled module owning command `name` (its canonical name).
    pub(crate) fn command_owner(&self, name: &str) -> Option<&'static str> {
        self.slots
            .iter()
            .filter(|slot| slot.enabled)
            .find(|slot| slot.module.commands().iter().any(|spec| spec.name == name))
            .map(|slot| slot.module.id())
    }

    /// Calls `hook` on the enabled modules (only `only` when given) and
    /// returns what they asked for.
    pub(crate) fn call(
        &mut self,
        ctx: &egui::Context,
        vaults: &[PathBuf],
        only: Option<&str>,
        hook: &str,
        mut f: impl FnMut(&mut dyn Module, &mut Cx),
    ) -> Vec<Request> {
        let mut requests = Vec::new();
        for slot in self.slots.iter_mut().filter(|slot| slot.enabled) {
            if only.is_some_and(|id| id != slot.module.id()) {
                continue;
            }
            let mut cx = Cx {
                vaults,
                data_dir: module_data_dir(slot.module.id()),
                waker: Waker {
                    flag: slot.woken.clone(),
                    ctx: ctx.clone(),
                },
                requests: Vec::new(),
                wake_at: None,
            };
            let started = Instant::now();
            f(slot.module.as_mut(), &mut cx);
            let elapsed = started.elapsed();
            if elapsed >= SLOW_HOOK {
                crate::perf_log::mark(
                    "module.slow_hook",
                    &format!(
                        "module={} hook={hook} ms={:.2}",
                        slot.module.id(),
                        elapsed.as_secs_f64() * 1000.0
                    ),
                );
            }
            if let Some(at) = cx.wake_at {
                slot.wake_at = Some(slot.wake_at.map_or(at, |old| old.min(at)));
            }
            requests.append(&mut cx.requests);
        }
        if let Some(next) = self.next_wake() {
            ctx.request_repaint_after(next.saturating_duration_since(Instant::now()));
        }
        requests
    }

    /// Modules whose waker fired or whose wake time came, cleared.
    pub(crate) fn due(&mut self) -> Vec<&'static str> {
        let now = Instant::now();
        let mut due = Vec::new();
        for slot in self.slots.iter_mut().filter(|slot| slot.enabled) {
            let woken = slot.woken.swap(false, Ordering::AcqRel);
            let timed = slot.wake_at.is_some_and(|at| at <= now);
            if woken || timed {
                if timed {
                    slot.wake_at = None;
                }
                due.push(slot.module.id());
            }
        }
        due
    }

    fn next_wake(&self) -> Option<Instant> {
        self.slots
            .iter()
            .filter(|slot| slot.enabled)
            .filter_map(|slot| slot.wake_at)
            .min()
    }
}

fn module_data_dir(id: &str) -> PathBuf {
    #[cfg(test)]
    let base =
        Some(std::env::temp_dir().join(format!("slate-test-modules-{}", std::process::id())));
    #[cfg(not(test))]
    let base = dirs_next::data_dir().map(|dir| dir.join("slate").join("modules"));
    base.unwrap_or_else(std::env::temp_dir).join(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Probe {
        opened: Vec<u64>,
        woke: usize,
    }

    impl Module for Probe {
        fn id(&self) -> &'static str {
            "probe"
        }
        fn name(&self) -> &'static str {
            "Probe"
        }
        fn description(&self) -> &'static str {
            "test module"
        }
        fn document_opened(&mut self, cx: &mut Cx, doc: DocumentRef) {
            self.opened.push(doc.id);
            cx.status(format!("opened {}", doc.id));
            cx.wake_after(Duration::ZERO);
        }
        fn wake(&mut self, _cx: &mut Cx) {
            self.woke += 1;
        }
    }

    fn doc() -> DocumentRef<'static> {
        DocumentRef {
            id: 7,
            path: None,
            revision: 0,
            text: "",
        }
    }

    #[test]
    fn hooks_reach_enabled_modules_only_and_wakes_are_delivered() {
        let ctx = egui::Context::default();
        let mut host = Host::new(vec![Box::new(Probe::default())]);
        let requests = host.call(&ctx, &[], None, "opened", |module, cx| {
            module.document_opened(cx, doc())
        });
        assert_eq!(requests, vec![Request::Status("opened 7".into())]);
        assert_eq!(host.due(), vec!["probe"], "asked to be woken now");
        assert!(host.due().is_empty(), "once");

        host.set_enabled("probe", false);
        let requests = host.call(&ctx, &[], None, "opened", |module, cx| {
            module.document_opened(cx, doc())
        });
        assert!(requests.is_empty(), "a disabled module is not called");
        assert!(!host.is_enabled("probe"));
    }
}
