//! Dropping a tab onto another Slate window joins it to that window.
//!
//! Every editor window listens on its own loopback port (same protocol and
//! token scheme as `single_instance`), recorded in
//! `%LOCALAPPDATA%/slate/windows/<pid>`. When a dragged tab is released
//! outside its window, the window under the cursor is looked up; if it
//! belongs to another Slate that listens, the tab travels there as a `TAB`
//! request (path, or a handoff file with unsaved text, and the cursor's
//! screen position so it lands where it was dropped).

use crate::single_instance::{self, OpenRequest};
use std::path::PathBuf;

/// The listening file of the window in process `process_id`.
pub(crate) fn window_file(process_id: u32) -> Option<PathBuf> {
    Some(
        single_instance::data_dir()?
            .join("windows")
            .join(process_id.to_string()),
    )
}

/// Starts listening for tabs dropped onto this window.
pub(crate) fn listen(ctx: &eframe::egui::Context) -> Option<single_instance::Server> {
    if cfg!(test) {
        return None;
    }
    let file = window_file(std::process::id())?;
    let ctx = ctx.clone();
    match single_instance::Server::start(file, move |_| ctx.request_repaint()) {
        Ok(server) => Some(server),
        Err(err) => {
            crate::perf_log::mark("tab_drop.unavailable", &err.to_string());
            None
        }
    }
}

/// Another Slate window under the cursor that accepts tabs.
pub(crate) struct Target {
    file: PathBuf,
    /// Cursor position on screen, physical pixels.
    cursor: (f32, f32),
}

/// The Slate window under the cursor, if it is another one that accepts tabs.
pub(crate) fn target_under_cursor() -> Option<Target> {
    if cfg!(test) {
        return None;
    }
    let (process_id, cursor) = win::window_under_cursor()?;
    if process_id == std::process::id() {
        return None;
    }
    let file = window_file(process_id)?;
    file.exists().then_some(Target { file, cursor })
}

/// Hands the tab to `target`; true once that window acknowledged it.
pub(crate) fn send(target: &Target, mut request: OpenRequest) -> bool {
    request.tab_at = Some(target.cursor);
    single_instance::forward(&target.file, &request)
}

#[cfg(windows)]
mod win {
    use std::ffi::c_void;

    #[repr(C)]
    struct Point {
        x: i32,
        y: i32,
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetCursorPos(point: *mut Point) -> i32;
        fn WindowFromPoint(point: Point) -> *mut c_void;
        fn GetAncestor(hwnd: *mut c_void, flags: u32) -> *mut c_void;
        fn GetWindowThreadProcessId(hwnd: *mut c_void, process_id: *mut u32) -> u32;
        fn IsWindowVisible(hwnd: *mut c_void) -> i32;
    }

    /// Process owning the top-level window under the cursor, and the cursor
    /// position in physical pixels.
    pub(super) fn window_under_cursor() -> Option<(u32, (f32, f32))> {
        const GA_ROOT: u32 = 2;
        // SAFETY: plain queries; every pointer is a local out-parameter.
        unsafe {
            let mut point = Point { x: 0, y: 0 };
            if GetCursorPos(&mut point) == 0 {
                return None;
            }
            let cursor = (point.x as f32, point.y as f32);
            let hwnd = WindowFromPoint(point);
            if hwnd.is_null() {
                return None;
            }
            let root = GetAncestor(hwnd, GA_ROOT);
            if root.is_null() || IsWindowVisible(root) == 0 {
                return None;
            }
            let mut process_id = 0;
            GetWindowThreadProcessId(root, &mut process_id);
            (process_id != 0).then_some((process_id, cursor))
        }
    }
}

#[cfg(not(windows))]
mod win {
    pub(super) fn window_under_cursor() -> Option<(u32, (f32, f32))> {
        None
    }
}
