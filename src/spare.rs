//! Instant new windows: while a resident Slate runs, one more Slate waits
//! fully started (window, GPU device, fonts) but hidden. A new window —
//! a tab dragged out, the tray's "New window", `slate --new-window` —
//! claims it over the single-instance channel and it shows itself in a few
//! milliseconds instead of a ~300 ms cold start. The claimed window then
//! starts the next spare in the background.
//!
//! The spare is an ordinary `--new-window` process that has not been shown
//! yet: it owns no session and opens nothing until claimed. It quits by
//! itself when the resident Slate that keeps it goes away.

use crate::single_instance::{self, OpenRequest};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// Wait after a start before preparing a spare, so it never competes with
/// the window that is starting.
pub(crate) const PREPARE_DELAY: Duration = Duration::from_secs(3);

static STARTING_AS_SPARE: AtomicBool = AtomicBool::new(false);
static CLAIMED: AtomicBool = AtomicBool::new(false);

/// Set by `main` before the app is built for `--spare`.
pub(crate) fn set_starting_as_spare() {
    STARTING_AS_SPARE.store(true, Ordering::Relaxed);
}

pub(crate) fn starting_as_spare() -> bool {
    STARTING_AS_SPARE.load(Ordering::Relaxed)
}

/// Hands `request` to the ready window and returns its process. None when
/// there is none (or another claimer got it first): the caller starts a
/// window the usual way.
pub(crate) fn claim(request: &OpenRequest) -> Option<u32> {
    if cfg!(test) {
        return None;
    }
    let file = single_instance::spare_file()?;
    let process_id = single_instance::listener_process_id(&file)?;
    single_instance::forward(&file, request).then_some(process_id)
}

/// True when a spare is waiting.
pub(crate) fn is_ready() -> bool {
    single_instance::spare_file().is_some_and(|file| single_instance::is_running(&file))
}

/// How often a visible resident Slate makes sure its spare still exists.
pub(crate) const CHECK_INTERVAL: Duration = Duration::from_secs(30);

/// Last spare this process started: a spare takes a moment to start
/// listening, and asking again meanwhile would start a second one.
static LAST_PREPARED: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);

/// Starts a spare for `owner` after `delay`, unless one is already waiting.
pub(crate) fn prepare_soon(owner: u32, delay: Duration) {
    if cfg!(test) {
        return;
    }
    std::thread::spawn(move || {
        std::thread::sleep(delay);
        if is_ready() {
            return;
        }
        {
            let Ok(mut last) = LAST_PREPARED.lock() else {
                return;
            };
            if last.is_some_and(|at| at.elapsed() < Duration::from_secs(15)) {
                return;
            }
            *last = Some(std::time::Instant::now());
        }
        if let Ok(exe) = std::env::current_exe() {
            let spawned = std::process::Command::new(exe)
                .arg("--spare")
                .arg(owner.to_string())
                .spawn();
            crate::perf_log::mark(
                "spare.prepare",
                &match spawned {
                    Ok(child) => format!("pid={}", child.id()),
                    Err(err) => format!("error={err}"),
                },
            );
        }
    });
}

/// Set from the listener thread the moment a new window takes this spare.
pub(crate) fn claimed() {
    CLAIMED.store(true, Ordering::Relaxed);
}

pub(crate) fn is_claimed() -> bool {
    CLAIMED.load(Ordering::Relaxed)
}
