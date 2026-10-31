//! Keeps window drags smooth with flip-model presentation (wgpu/DX12).
//!
//! Windows moves a window inside a modal loop that waits for every
//! `WM_WINDOWPOSCHANGED` handler. winit turns each one into `WindowEvent::Moved`
//! and egui-winit repaints on `Moved`, so each mouse step of a drag rendered and
//! presented a frame. Worse, inside the move loop each `WM_PAINT` becomes a
//! `RedrawRequested`, egui-winit answers it with another redraw request, and the
//! loop renders back to back (~330 fps with glow). With a flip-model swapchain
//! every present waits on the compositor, which tied the drag to ~30 Hz. A pure
//! move does not change what Slate draws, so while the move loop runs those
//! notifications skip winit and one repaint after the drag refreshes egui's idea
//! of the window position.

#[cfg(windows)]
pub fn install(cc: &eframe::CreationContext<'_>) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let Ok(handle) = cc.window_handle() else {
        return;
    };
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return;
    };
    let _ = win::CONTEXT.set(cc.egui_ctx.clone());
    // SAFETY: the HWND belongs to this thread's live main window; the subclass
    // procedure only forwards to the default procedure chain.
    unsafe {
        win::SetWindowSubclass(
            handle.hwnd.get() as win::Hwnd,
            Some(win::procedure),
            win::SUBCLASS_ID,
            0,
        );
    }
}

#[cfg(not(windows))]
pub fn install(_cc: &eframe::CreationContext<'_>) {}

/// Counts frames rendered during a move loop, for the `window.drag` log line.
pub fn note_frame() {
    #[cfg(windows)]
    win::note_frame();
}

#[cfg(windows)]
mod win {
    use std::ffi::c_void;
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::time::Instant;

    pub(super) type Hwnd = *mut c_void;
    type SubclassProc = unsafe extern "system" fn(Hwnd, u32, usize, isize, usize, usize) -> isize;

    pub(super) const SUBCLASS_ID: usize = 0x534c_4154;
    const WM_PAINT: u32 = 0x000F;
    const WM_WINDOWPOSCHANGED: u32 = 0x0047;
    const WM_ENTERSIZEMOVE: u32 = 0x0231;
    const WM_EXITSIZEMOVE: u32 = 0x0232;
    const SWP_NOSIZE: u32 = 0x0001;

    pub(super) static CONTEXT: OnceLock<eframe::egui::Context> = OnceLock::new();
    static IN_MOVE_LOOP: AtomicBool = AtomicBool::new(false);
    static MOVES: AtomicU32 = AtomicU32::new(0);
    static SKIPPED: AtomicU32 = AtomicU32::new(0);
    static FRAMES: AtomicU32 = AtomicU32::new(0);
    static PAINTS: AtomicU32 = AtomicU32::new(0);
    static STARTED: std::sync::Mutex<Option<Instant>> = std::sync::Mutex::new(None);

    pub(super) fn note_frame() {
        if IN_MOVE_LOOP.load(Ordering::Relaxed) {
            FRAMES.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[repr(C)]
    struct WindowPos {
        hwnd: Hwnd,
        insert_after: Hwnd,
        x: i32,
        y: i32,
        cx: i32,
        cy: i32,
        flags: u32,
    }

    #[link(name = "comctl32")]
    unsafe extern "system" {
        pub(super) fn SetWindowSubclass(
            hwnd: Hwnd,
            proc: Option<SubclassProc>,
            id: usize,
            data: usize,
        ) -> i32;
        fn DefSubclassProc(hwnd: Hwnd, message: u32, wparam: usize, lparam: isize) -> isize;
    }
    #[link(name = "user32")]
    unsafe extern "system" {
        fn DefWindowProcW(hwnd: Hwnd, message: u32, wparam: usize, lparam: isize) -> isize;
    }

    pub(super) unsafe extern "system" fn procedure(
        hwnd: Hwnd,
        message: u32,
        wparam: usize,
        lparam: isize,
        _id: usize,
        _data: usize,
    ) -> isize {
        match message {
            WM_ENTERSIZEMOVE => {
                for counter in [&MOVES, &SKIPPED, &FRAMES, &PAINTS] {
                    counter.store(0, Ordering::Relaxed);
                }
                *STARTED.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
                IN_MOVE_LOOP.store(true, Ordering::Relaxed);
            }
            WM_EXITSIZEMOVE => {
                IN_MOVE_LOOP.store(false, Ordering::Relaxed);
                let started = STARTED.lock().unwrap_or_else(|e| e.into_inner()).take();
                crate::perf_log::mark(
                    "window.drag",
                    &format!(
                        "duration_ms={:.0} moves={} skipped={} paints={} frames={}",
                        started.map_or(0.0, |s| s.elapsed().as_secs_f64() * 1000.0),
                        MOVES.load(Ordering::Relaxed),
                        SKIPPED.load(Ordering::Relaxed),
                        PAINTS.load(Ordering::Relaxed),
                        FRAMES.load(Ordering::Relaxed),
                    ),
                );
                if let Some(ctx) = CONTEXT.get() {
                    ctx.request_repaint();
                }
            }
            WM_PAINT if IN_MOVE_LOOP.load(Ordering::Relaxed) => {
                // The swapchain keeps the last frame; validating the window here
                // (BeginPaint/EndPaint in the default procedure) breaks the
                // paint -> redraw -> paint cycle without rendering.
                PAINTS.fetch_add(1, Ordering::Relaxed);
                return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
            }
            WM_WINDOWPOSCHANGED if IN_MOVE_LOOP.load(Ordering::Relaxed) && lparam != 0 => {
                // SAFETY: WM_WINDOWPOSCHANGED carries a valid WINDOWPOS pointer.
                let flags = unsafe { (*(lparam as *const WindowPos)).flags };
                MOVES.fetch_add(1, Ordering::Relaxed);
                if flags & SWP_NOSIZE != 0 {
                    SKIPPED.fetch_add(1, Ordering::Relaxed);
                    // Pure move: skip winit's `Moved` (and egui's repaint). The
                    // default procedure still sends WM_MOVE as Windows expects.
                    return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
                }
            }
            _ => {}
        }
        unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
    }
}
