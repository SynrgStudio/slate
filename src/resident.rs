//! Resident mode: the primary Slate window hides instead of exiting, so the
//! next `slate` launch only shows an already initialized window (~10–20 ms)
//! instead of paying ~300 ms for window, GPU device and driver start-up.
//!
//! A tray icon keeps the hidden process visible to the user: left click shows
//! the window, the menu offers a new window or a real quit. `--background`
//! (used by the optional "start with Windows" entry) starts hidden.

use std::sync::atomic::{AtomicBool, Ordering};

static SHOW_REQUESTED: AtomicBool = AtomicBool::new(false);
static QUIT_REQUESTED: AtomicBool = AtomicBool::new(false);

/// The tray asked to show the window (left click or "Open Slate").
pub(crate) fn take_show_request() -> bool {
    SHOW_REQUESTED.swap(false, Ordering::Relaxed)
}

/// The tray asked to quit for real ("Quit Slate").
pub(crate) fn take_quit_request() -> bool {
    QUIT_REQUESTED.swap(false, Ordering::Relaxed)
}

#[cfg(windows)]
pub(crate) use win::{
    allow_foreground_handoff, autostart_enabled, install, remove_tray, set_autostart, show_now,
};

#[cfg(not(windows))]
pub(crate) fn install(_cc: &eframe::CreationContext<'_>) {}
#[cfg(not(windows))]
pub(crate) fn remove_tray() {}
#[cfg(not(windows))]
pub(crate) fn show_now() {}
#[cfg(not(windows))]
pub(crate) fn allow_foreground_handoff() {}
#[cfg(not(windows))]
pub(crate) fn autostart_enabled() -> bool {
    false
}
#[cfg(not(windows))]
pub(crate) fn set_autostart(_enabled: bool) -> Result<(), String> {
    Err("Start with the system is only available on Windows".into())
}

#[cfg(windows)]
mod win {
    use super::{QUIT_REQUESTED, SHOW_REQUESTED};
    use std::ffi::c_void;
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering};

    type Hwnd = *mut c_void;
    type Handle = *mut c_void;
    type SubclassProc = unsafe extern "system" fn(Hwnd, u32, usize, isize, usize, usize) -> isize;

    const SUBCLASS_ID: usize = 0x534c_5452;
    const TRAY_ID: u32 = 1;
    const WM_TRAY: u32 = 0x8000 + 0x51; // WM_APP + 0x51
    const WM_NULL: u32 = 0x0000;
    const WM_GETICON: u32 = 0x007F;
    const WM_LBUTTONUP: u32 = 0x0202;
    const WM_RBUTTONUP: u32 = 0x0205;
    const WM_CONTEXTMENU: u32 = 0x007B;
    const NIM_ADD: u32 = 0;
    const NIM_DELETE: u32 = 2;
    const NIF_MESSAGE: u32 = 1;
    const NIF_ICON: u32 = 2;
    const NIF_TIP: u32 = 4;
    const MF_STRING: u32 = 0;
    const MF_SEPARATOR: u32 = 0x800;
    const TPM_RETURNCMD: u32 = 0x100;
    const TPM_NONOTIFY: u32 = 0x80;
    const TPM_RIGHTBUTTON: u32 = 0x2;
    const GCLP_HICONSM: i32 = -34;
    const IDI_APPLICATION: usize = 32512;
    const ASFW_ANY: u32 = u32::MAX;
    const MENU_OPEN: usize = 1;
    const MENU_NEW_WINDOW: usize = 2;
    const MENU_QUIT: usize = 3;

    const HKEY_CURRENT_USER: isize = 0x8000_0001_u32 as i32 as isize;
    const REG_SZ: u32 = 1;
    const RRF_RT_REG_SZ: u32 = 0x2;
    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const RUN_VALUE: &str = "Slate";

    static HWND: AtomicIsize = AtomicIsize::new(0);
    static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);
    static CONTEXT: OnceLock<eframe::egui::Context> = OnceLock::new();

    #[repr(C)]
    struct NotifyIconData {
        size: u32,
        hwnd: Hwnd,
        id: u32,
        flags: u32,
        callback_message: u32,
        icon: Handle,
        tip: [u16; 128],
        state: u32,
        state_mask: u32,
        info: [u16; 256],
        version: u32,
        info_title: [u16; 64],
        info_flags: u32,
        guid: [u8; 16],
        balloon_icon: Handle,
    }

    #[repr(C)]
    struct Point {
        x: i32,
        y: i32,
    }

    #[link(name = "comctl32")]
    unsafe extern "system" {
        fn SetWindowSubclass(hwnd: Hwnd, proc: Option<SubclassProc>, id: usize, data: usize)
        -> i32;
        fn DefSubclassProc(hwnd: Hwnd, message: u32, wparam: usize, lparam: isize) -> isize;
    }
    #[link(name = "shell32")]
    unsafe extern "system" {
        fn Shell_NotifyIconW(message: u32, data: *mut NotifyIconData) -> i32;
    }
    #[link(name = "user32")]
    unsafe extern "system" {
        fn RegisterWindowMessageW(name: *const u16) -> u32;
        fn SendMessageW(hwnd: Hwnd, message: u32, wparam: usize, lparam: isize) -> isize;
        fn PostMessageW(hwnd: Hwnd, message: u32, wparam: usize, lparam: isize) -> i32;
        fn GetClassLongPtrW(hwnd: Hwnd, index: i32) -> usize;
        fn LoadIconW(instance: Handle, name: *const u16) -> Handle;
        fn CreatePopupMenu() -> Handle;
        fn AppendMenuW(menu: Handle, flags: u32, id: usize, text: *const u16) -> i32;
        fn TrackPopupMenu(
            menu: Handle,
            flags: u32,
            x: i32,
            y: i32,
            reserved: i32,
            hwnd: Hwnd,
            rect: *const c_void,
        ) -> i32;
        fn DestroyMenu(menu: Handle) -> i32;
        fn GetCursorPos(point: *mut Point) -> i32;
        fn SetForegroundWindow(hwnd: Hwnd) -> i32;
        fn IsWindowVisible(hwnd: Hwnd) -> i32;
        fn ShowWindowAsync(hwnd: Hwnd, command: i32) -> i32;
        fn AllowSetForegroundWindow(process_id: u32) -> i32;
    }
    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn RegSetKeyValueW(
            key: isize,
            subkey: *const u16,
            value: *const u16,
            kind: u32,
            data: *const c_void,
            bytes: u32,
        ) -> i32;
        fn RegDeleteKeyValueW(key: isize, subkey: *const u16, value: *const u16) -> i32;
        fn RegGetValueW(
            key: isize,
            subkey: *const u16,
            value: *const u16,
            flags: u32,
            kind: *mut u32,
            data: *mut c_void,
            bytes: *mut u32,
        ) -> i32;
    }

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn hwnd() -> Hwnd {
        HWND.load(Ordering::Relaxed) as Hwnd
    }

    /// Adds the tray icon and listens for its clicks on the main window.
    pub(crate) fn install(cc: &eframe::CreationContext<'_>) {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        let Ok(handle) = cc.window_handle() else {
            return;
        };
        let RawWindowHandle::Win32(handle) = handle.as_raw() else {
            return;
        };
        let hwnd = handle.hwnd.get() as Hwnd;
        HWND.store(hwnd as isize, Ordering::Relaxed);
        let _ = CONTEXT.set(cc.egui_ctx.clone());
        // SAFETY: plain Win32 calls on this thread's live main window.
        unsafe {
            TASKBAR_CREATED.store(
                RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()),
                Ordering::Relaxed,
            );
            SetWindowSubclass(hwnd, Some(procedure), SUBCLASS_ID, 0);
        }
        add_tray();
    }

    fn icon_data() -> NotifyIconData {
        // SAFETY: zeroed is a valid NOTIFYICONDATAW (all integers/pointers).
        let mut data: NotifyIconData = unsafe { std::mem::zeroed() };
        data.size = std::mem::size_of::<NotifyIconData>() as u32;
        data.hwnd = hwnd();
        data.id = TRAY_ID;
        data
    }

    fn add_tray() {
        if hwnd().is_null() {
            return;
        }
        let mut data = icon_data();
        data.flags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        data.callback_message = WM_TRAY;
        data.icon = window_icon();
        for (slot, unit) in data.tip.iter_mut().zip("Slate".encode_utf16()) {
            *slot = unit;
        }
        // SAFETY: `data` is a fully initialized NOTIFYICONDATAW.
        unsafe { Shell_NotifyIconW(NIM_ADD, &mut data) };
    }

    pub(crate) fn remove_tray() {
        if hwnd().is_null() {
            return;
        }
        let mut data = icon_data();
        // SAFETY: same identity (hwnd, id) as the icon that was added.
        unsafe { Shell_NotifyIconW(NIM_DELETE, &mut data) };
    }

    fn window_icon() -> Handle {
        // SAFETY: read-only queries of our own window and a stock icon.
        unsafe {
            for icon in [
                SendMessageW(hwnd(), WM_GETICON, 2, 0) as Handle, // ICON_SMALL2
                SendMessageW(hwnd(), WM_GETICON, 0, 0) as Handle, // ICON_SMALL
                GetClassLongPtrW(hwnd(), GCLP_HICONSM) as Handle,
            ] {
                if !icon.is_null() {
                    return icon;
                }
            }
            LoadIconW(std::ptr::null_mut(), IDI_APPLICATION as *const u16)
        }
    }

    /// Shows the hidden main window immediately, from any thread.
    ///
    /// eframe throttles repaints of invisible windows to one per 100 ms, so
    /// waiting for the next egui frame to send `Visible(true)` made reopening
    /// take ~100 ms. The show message is queued ahead of the repaint request,
    /// so that frame already runs unthrottled and syncs winit's visible flag.
    pub(crate) fn show_now() {
        let hwnd = hwnd();
        // SAFETY: plain Win32 calls; a null/unknown HWND is rejected first.
        unsafe {
            if !hwnd.is_null() && IsWindowVisible(hwnd) == 0 {
                ShowWindowAsync(hwnd, 5); // SW_SHOW
            }
        }
    }

    fn wake() {
        if let Some(ctx) = CONTEXT.get() {
            ctx.request_repaint();
        }
    }

    fn show_menu(hwnd: Hwnd) {
        // SAFETY: standard tray-menu sequence on our own window; the menu is
        // destroyed before returning.
        let choice = unsafe {
            let menu = CreatePopupMenu();
            if menu.is_null() {
                return;
            }
            AppendMenuW(menu, MF_STRING, MENU_OPEN, wide("Open Slate").as_ptr());
            AppendMenuW(
                menu,
                MF_STRING,
                MENU_NEW_WINDOW,
                wide("New window").as_ptr(),
            );
            AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
            AppendMenuW(menu, MF_STRING, MENU_QUIT, wide("Quit Slate").as_ptr());
            let mut point = Point { x: 0, y: 0 };
            GetCursorPos(&mut point);
            // Required so the menu closes when the user clicks elsewhere.
            SetForegroundWindow(hwnd);
            let choice = TrackPopupMenu(
                menu,
                TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON,
                point.x,
                point.y,
                0,
                hwnd,
                std::ptr::null(),
            );
            DestroyMenu(menu);
            PostMessageW(hwnd, WM_NULL, 0, 0);
            choice as usize
        };
        match choice {
            MENU_OPEN => {
                SHOW_REQUESTED.store(true, Ordering::Relaxed);
                show_now();
            }
            MENU_NEW_WINDOW => {
                if let Ok(exe) = std::env::current_exe() {
                    let _ = std::process::Command::new(exe).arg("--new-window").spawn();
                }
            }
            MENU_QUIT => QUIT_REQUESTED.store(true, Ordering::Relaxed),
            _ => return,
        }
        wake();
    }

    unsafe extern "system" fn procedure(
        hwnd: Hwnd,
        message: u32,
        wparam: usize,
        lparam: isize,
        _id: usize,
        _data: usize,
    ) -> isize {
        if message == WM_TRAY {
            match lparam as u32 & 0xFFFF {
                WM_LBUTTONUP => {
                    SHOW_REQUESTED.store(true, Ordering::Relaxed);
                    show_now();
                    wake();
                }
                WM_RBUTTONUP | WM_CONTEXTMENU => show_menu(hwnd),
                _ => {}
            }
            return 0;
        }
        let taskbar_created = TASKBAR_CREATED.load(Ordering::Relaxed);
        if taskbar_created != 0 && message == taskbar_created {
            // Explorer restarted: tray icons must be added again.
            add_tray();
        }
        unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
    }

    /// Called by a launching process before it forwards to the running one,
    /// so the running window may take the foreground when it shows itself.
    pub(crate) fn allow_foreground_handoff() {
        // SAFETY: no pointers; only widens the foreground lock for this launch.
        unsafe { AllowSetForegroundWindow(ASFW_ANY) };
    }

    pub(crate) fn autostart_enabled() -> bool {
        let mut bytes = 0u32;
        // SAFETY: size query only (null data pointer).
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                wide(RUN_KEY).as_ptr(),
                wide(RUN_VALUE).as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut bytes,
            )
        };
        status == 0
    }

    pub(crate) fn set_autostart(enabled: bool) -> Result<(), String> {
        let status = if enabled {
            let exe = std::env::current_exe().map_err(|err| err.to_string())?;
            let command = wide(&format!("\"{}\" --background", exe.display()));
            // SAFETY: `command` is a NUL-terminated UTF-16 buffer of the given size.
            unsafe {
                RegSetKeyValueW(
                    HKEY_CURRENT_USER,
                    wide(RUN_KEY).as_ptr(),
                    wide(RUN_VALUE).as_ptr(),
                    REG_SZ,
                    command.as_ptr() as *const c_void,
                    (command.len() * 2) as u32,
                )
            }
        } else {
            // SAFETY: NUL-terminated key and value names.
            let status = unsafe {
                RegDeleteKeyValueW(
                    HKEY_CURRENT_USER,
                    wide(RUN_KEY).as_ptr(),
                    wide(RUN_VALUE).as_ptr(),
                )
            };
            if status == 2 { 0 } else { status } // ERROR_FILE_NOT_FOUND: already off
        };
        if status == 0 {
            Ok(())
        } else {
            Err(format!("Windows registry error {status}"))
        }
    }
}
