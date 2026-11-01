//! Dragging tabs between Slate windows.
//!
//! Every editor window listens on its own loopback port (same protocol and
//! token scheme as `single_instance`), recorded in
//! `%LOCALAPPDATA%/slate/windows/<pid>`. While a tab is dragged over another
//! Slate window that window is told where (`HOVER`, then `LEAVE`) so it can
//! show where the tab would land; released there, the tab travels as a
//! `TAB` request (path, or a handoff file with unsaved text, and the
//! cursor's screen position).
//!
//! Dragged out of its window (with a spare ready) a tab is torn off at once
//! into a new window that follows the cursor until the button is released;
//! released over a Slate window, the torn-off window hands the tab on to it
//! (`SENDTO`) and closes.

use crate::single_instance::{self, OpenRequest};
use std::path::{Path, PathBuf};

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

/// A Slate window under the cursor that accepts tabs.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Target {
    pub(crate) process_id: u32,
    pub(crate) file: PathBuf,
    /// Cursor position on screen, physical pixels.
    pub(crate) cursor: (f32, f32),
}

/// The Slate window under the cursor that accepts tabs, looking through the
/// windows of the processes in `skip` (this one, a window being dragged).
pub(crate) fn target_under_cursor(skip: &[u32]) -> Option<Target> {
    if cfg!(test) {
        return None;
    }
    let cursor = win::cursor()?;
    let process_id = win::window_at(cursor, skip)?;
    let file = window_file(process_id)?;
    file.exists().then_some(Target {
        process_id,
        file,
        cursor,
    })
}

/// Hands the tab to `target`; true once that window acknowledged it.
pub(crate) fn send(target: &Target, request: OpenRequest) -> bool {
    single_instance::forward(
        &target.file,
        &OpenRequest {
            tab_at: Some(target.cursor),
            ..request
        },
    )
}

/// Tells `target` a tab called `label` is over it.
pub(crate) fn hover(target: &Target, label: &str) {
    let _ = single_instance::forward(
        &target.file,
        &OpenRequest {
            tab_at: Some(target.cursor),
            hover: Some(label.to_string()),
            ..Default::default()
        },
    );
}

/// Tells the window listening at `file` the dragged tab left it.
pub(crate) fn leave(file: &Path) {
    let _ = single_instance::forward(
        file,
        &OpenRequest {
            hover: Some(String::new()),
            ..Default::default()
        },
    );
}

/// Asks the (torn-off) window in `process_id` to hand its tab to `target`.
pub(crate) fn send_on(process_id: u32, target: &Target) -> bool {
    window_file(process_id).is_some_and(|file| {
        single_instance::forward(
            &file,
            &OpenRequest {
                tab_at: Some(target.cursor),
                send_to: Some(target.file.clone()),
                ..Default::default()
            },
        )
    })
}

/// Cursor position on screen, physical pixels.
pub(crate) fn cursor() -> Option<(f32, f32)> {
    win::cursor()
}

/// Moves the top-level window of `process_id` so its top-left is `cursor`
/// minus `grab` (physical pixels; `grab` is in points of that window).
/// Returns false while that window cannot be found yet.
pub(crate) fn move_window(process_id: u32, cursor: (f32, f32), grab: (f32, f32)) -> bool {
    win::move_window(process_id, cursor, grab)
}

/// Brings the top-level window of `process_id` to the front.
pub(crate) fn focus_window(process_id: u32) {
    win::focus_window(process_id);
}

#[cfg(windows)]
mod win {
    use std::ffi::c_void;

    type Hwnd = *mut c_void;

    #[repr(C)]
    #[derive(Default)]
    struct Point {
        x: i32,
        y: i32,
    }

    #[repr(C)]
    #[derive(Default)]
    struct Rect {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetCursorPos(point: *mut Point) -> i32;
        fn GetTopWindow(hwnd: Hwnd) -> Hwnd;
        fn GetWindow(hwnd: Hwnd, command: u32) -> Hwnd;
        fn GetWindowRect(hwnd: Hwnd, rect: *mut Rect) -> i32;
        fn GetWindowThreadProcessId(hwnd: Hwnd, process_id: *mut u32) -> u32;
        fn GetWindowLongPtrW(hwnd: Hwnd, index: i32) -> isize;
        fn GetWindowTextLengthW(hwnd: Hwnd) -> i32;
        fn IsWindowVisible(hwnd: Hwnd) -> i32;
        fn IsIconic(hwnd: Hwnd) -> i32;
        fn GetDpiForWindow(hwnd: Hwnd) -> u32;
        fn SetWindowPos(
            hwnd: Hwnd,
            after: Hwnd,
            x: i32,
            y: i32,
            width: i32,
            height: i32,
            flags: u32,
        ) -> i32;
        fn SetForegroundWindow(hwnd: Hwnd) -> i32;
    }
    #[link(name = "dwmapi")]
    unsafe extern "system" {
        fn DwmGetWindowAttribute(hwnd: Hwnd, attribute: u32, value: *mut c_void, size: u32) -> i32;
    }

    const GW_HWNDNEXT: u32 = 2;
    const GWL_EXSTYLE: i32 = -20;
    const WS_EX_TRANSPARENT: isize = 0x20;
    const DWMWA_CLOAKED: u32 = 14;

    pub(super) fn cursor() -> Option<(f32, f32)> {
        let mut point = Point::default();
        // SAFETY: `point` is a local out-parameter.
        (unsafe { GetCursorPos(&mut point) } != 0).then_some((point.x as f32, point.y as f32))
    }

    fn process_of(hwnd: Hwnd) -> u32 {
        let mut process_id = 0;
        // SAFETY: plain query; `process_id` is a local out-parameter.
        unsafe { GetWindowThreadProcessId(hwnd, &mut process_id) };
        process_id
    }

    /// A window someone can see: visible, not minimized, not cloaked and
    /// not click-through.
    fn shown(hwnd: Hwnd) -> bool {
        // SAFETY: plain queries on a window handle from the system's list.
        unsafe {
            if IsWindowVisible(hwnd) == 0 || IsIconic(hwnd) != 0 {
                return false;
            }
            if GetWindowLongPtrW(hwnd, GWL_EXSTYLE) & WS_EX_TRANSPARENT != 0 {
                return false;
            }
            let mut cloaked = 0u32;
            DwmGetWindowAttribute(
                hwnd,
                DWMWA_CLOAKED,
                (&raw mut cloaked).cast(),
                std::mem::size_of::<u32>() as u32,
            );
            cloaked == 0
        }
    }

    /// Top-level windows from the front to the back.
    fn windows() -> impl Iterator<Item = Hwnd> {
        // SAFETY: walking the system's top-level window list.
        let first = unsafe { GetTopWindow(std::ptr::null_mut()) };
        std::iter::successors((!first.is_null()).then_some(first), |hwnd| {
            let next = unsafe { GetWindow(*hwnd, GW_HWNDNEXT) };
            (!next.is_null()).then_some(next)
        })
    }

    /// Process of the front-most window at `point`, looking through the
    /// windows of the processes in `skip`.
    pub(super) fn window_at(point: (f32, f32), skip: &[u32]) -> Option<u32> {
        let (x, y) = (point.0 as i32, point.1 as i32);
        windows().find_map(|hwnd| {
            let mut rect = Rect::default();
            // SAFETY: `rect` is a local out-parameter.
            if !shown(hwnd) || unsafe { GetWindowRect(hwnd, &mut rect) } == 0 {
                return None;
            }
            if x < rect.left || x >= rect.right || y < rect.top || y >= rect.bottom {
                return None;
            }
            // The front-most window here decides, unless it is skipped.
            let process_id = process_of(hwnd);
            (!skip.contains(&process_id)).then_some(process_id)
        })
    }

    /// The titled top-level window of `process_id` (shown or not).
    fn window_of(process_id: u32) -> Option<Hwnd> {
        // SAFETY: plain query.
        windows().find(|hwnd| {
            process_of(*hwnd) == process_id && unsafe { GetWindowTextLengthW(*hwnd) } > 0
        })
    }

    pub(super) fn move_window(process_id: u32, cursor: (f32, f32), grab: (f32, f32)) -> bool {
        const SWP_NOSIZE: u32 = 0x1;
        const SWP_NOZORDER: u32 = 0x4;
        const SWP_NOACTIVATE: u32 = 0x10;
        const SWP_ASYNCWINDOWPOS: u32 = 0x4000;
        let Some(hwnd) = window_of(process_id) else {
            return false;
        };
        // SAFETY: plain Win32 calls; the move is posted to the window's own
        // thread instead of waiting for it.
        unsafe {
            let scale = GetDpiForWindow(hwnd).max(96) as f32 / 96.0;
            SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                (cursor.0 - grab.0 * scale).round() as i32,
                (cursor.1 - grab.1 * scale).round() as i32,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_ASYNCWINDOWPOS,
            );
        }
        true
    }

    pub(super) fn focus_window(process_id: u32) {
        if let Some(hwnd) = window_of(process_id) {
            // SAFETY: plain Win32 call; this process is in the foreground.
            unsafe { SetForegroundWindow(hwnd) };
        }
    }
}

#[cfg(not(windows))]
mod win {
    pub(super) fn cursor() -> Option<(f32, f32)> {
        None
    }
    pub(super) fn window_at(_point: (f32, f32), _skip: &[u32]) -> Option<u32> {
        None
    }
    pub(super) fn move_window(_process_id: u32, _cursor: (f32, f32), _grab: (f32, f32)) -> bool {
        false
    }
    pub(super) fn focus_window(_process_id: u32) {}
}
