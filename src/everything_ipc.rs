//! Everything public Unicode IPC v1 search / QUERY2 metadata inventory (32/64-bit servers).
//! Protocol reference: voidtools Everything SDK ipc/everything_ipc.h.
use std::path::PathBuf;
#[derive(Debug)]
pub(crate) struct Page {
    pub paths: Vec<PathBuf>,
    pub total: usize,
    pub returned: usize,
}
fn word(data: &[u8], at: usize) -> Result<u32, String> {
    let bytes = data.get(at..at + 4).ok_or("Truncated Everything reply")?;
    Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
}
fn text(data: &[u8], at: usize, minimum: usize) -> Result<String, String> {
    if at < minimum || at % 2 != 0 {
        return Err("Invalid Everything string offset".into());
    }
    let mut words = Vec::new();
    let mut offset = at;
    loop {
        let bytes = data
            .get(offset..offset + 2)
            .ok_or("Unterminated Everything string")?;
        let value = u16::from_le_bytes([bytes[0], bytes[1]]);
        if value == 0 {
            break;
        }
        words.push(value);
        if words.len() > 32768 {
            return Err("Everything path too long".into());
        }
        offset += 2;
    }
    String::from_utf16(&words).map_err(|_| "Invalid Everything Unicode".into())
}
pub(crate) fn parse(data: &[u8]) -> Result<Page, String> {
    let total = word(data, 8)? as usize;
    let count = word(data, 20)? as usize;
    if count > 256 {
        return Err("Oversized Everything page".into());
    }
    let end = 28 + count * 12;
    if end > data.len() {
        return Err("Truncated Everything item array".into());
    }
    let mut paths = Vec::new();
    for index in 0..count {
        let at = 28 + index * 12;
        if word(data, at)? & 3 != 0 {
            continue;
        }
        let name = text(data, word(data, at + 4)? as usize, end)?;
        let parent = text(data, word(data, at + 8)? as usize, end)?;
        if name.is_empty() || name == "." || name == ".." || name.contains(['\\', '/']) {
            return Err("Invalid Everything filename".into());
        }
        paths.push(PathBuf::from(format!(
            "{}\\{}",
            parent.trim_end_matches(['\\', '/']),
            name
        )));
    }
    Ok(Page {
        paths,
        total,
        returned: count,
    })
}
/// Quote plain words; never interpret user text as Everything operators/macros.
pub(crate) fn native_search(query: &str, roots: &[PathBuf]) -> Option<(String, bool)> {
    if query.trim().is_empty()
        || query.contains(['"', '!', '<', '>', '|', '*', '?', ':'])
        || query.chars().any(char::is_control)
        || roots.is_empty()
    {
        return None;
    }
    let mut scopes = Vec::new();
    for root in roots {
        let root = root.to_str()?.replace('/', "\\");
        if root.contains('"') {
            return None;
        }
        scopes.push(format!("path:\"{}\\\"", root.trim_end_matches('\\')));
    }
    let words = query
        .split_whitespace()
        .map(|word| format!("\"{word}\""))
        .collect::<Vec<_>>()
        .join(" ");
    Some((
        format!("file: <{}> {words}", scopes.join(" | ")),
        query.contains(['/', '\\']),
    ))
}
pub(crate) fn normal_query(
    search: &str,
    offset: usize,
    match_path: bool,
    cancel: &dyn Fn() -> bool,
) -> Result<Page, String> {
    #[cfg(windows)]
    {
        parse(&win::exchange(
            "EVERYTHING_TASKBAR_NOTIFICATION",
            search,
            offset,
            2,
            256,
            &[],
            5000,
            if match_path { 4 } else { 0 },
            cancel,
        )?)
    }
    #[cfg(not(windows))]
    {
        let _ = (search, offset, match_path, cancel);
        Err("Everything is available only on Windows".into())
    }
}
pub(crate) fn query_cancellable(
    expression: &str,
    offset: usize,
    cancel: &dyn Fn() -> bool,
) -> Result<Page, String> {
    #[cfg(windows)]
    {
        parse(&win::exchange(
            "EVERYTHING_TASKBAR_NOTIFICATION",
            expression,
            offset,
            2,
            256,
            &[],
            5000,
            12,
            cancel,
        )?)
    }
    #[cfg(not(windows))]
    {
        let _ = (expression, offset, cancel);
        Err("Everything is available only on Windows".into())
    }
}
pub(crate) fn filename_expression(query: &str, roots: &[PathBuf]) -> String {
    let scope = expression("", roots);
    let prefix = scope.strip_suffix(".*").unwrap_or(&scope);
    let mut result = format!("{prefix}(?:[^\\\\/]*[\\\\/])*[^\\\\/]*?");
    for ch in query
        .trim()
        .chars()
        .filter(|ch| !ch.is_whitespace() && *ch != '/' && *ch != '\\')
    {
        result.push_str(&escape(&ch.to_string()));
        result.push_str("[^\\\\/]*?");
    }
    result.push('$');
    result
}
pub(crate) fn inventory_expression(root: &std::path::Path, cache_dir: &std::path::Path) -> String {
    let expression = expression("", &[root.to_path_buf()]);
    let excluded = escape(
        &cache_dir
            .to_string_lossy()
            .replace('/', "\\")
            .trim_end_matches('\\'),
    );
    format!("^(?!{excluded}(?:[\\\\/]|$)){}", &expression[1..])
}
fn escape(value: &str) -> String {
    value
        .chars()
        .flat_map(|ch| {
            let mut out = String::new();
            if "\\.^$|?*+()[]{}".contains(ch) {
                out.push('\\');
            }
            out.push(ch);
            out.chars().collect::<Vec<_>>()
        })
        .collect()
}
/// Only regex, not Everything command syntax. Scope components are literal and boundary-safe.
pub(crate) fn expression(query: &str, roots: &[PathBuf]) -> String {
    let scopes = roots
        .iter()
        .map(|root| {
            let root = root.to_string_lossy().replace('/', "\\");
            format!("{}[\\\\/]", escape(root.trim_end_matches('\\')))
        })
        .collect::<Vec<_>>()
        .join("|");
    let mut result = if scopes.is_empty() {
        String::from("^")
    } else {
        format!("^(?:{scopes})")
    };
    for ch in query.trim().chars() {
        if ch.is_whitespace() || ch == '/' || ch == '\\' {
            continue;
        }
        result.push_str(".*");
        result.push_str(&escape(&ch.to_string()));
    }
    result.push_str(".*");
    result
}
#[cfg(all(test, not(windows)))]
pub(crate) fn query(_expression: &str, _offset: usize) -> Result<Page, String> {
    Err("Everything is available only on Windows".into())
}
#[cfg(all(test, windows))]
pub(crate) fn query(expression: &str, offset: usize) -> Result<Page, String> {
    win::query(expression, offset)
}
pub(crate) fn inventory(
    expression: &str,
    offset: usize,
) -> Result<crate::everything_inventory::Page, String> {
    #[cfg(windows)]
    {
        crate::everything_inventory::parse(&win::inventory(expression, offset)?)
    }
    #[cfg(not(windows))]
    {
        let _ = (expression, offset);
        Err("Everything is available only on Windows".into())
    }
}
#[cfg(windows)]
mod win {
    #[cfg(test)]
    use super::*;
    use std::{
        cell::RefCell,
        ffi::c_void,
        time::{Duration, Instant},
    };
    type Hwnd = *mut c_void;
    #[repr(C)]
    struct CopyData {
        tag: usize,
        len: u32,
        data: *const u8,
    }
    #[repr(C)]
    struct Class {
        style: u32,
        proc: Option<unsafe extern "system" fn(Hwnd, u32, usize, isize) -> isize>,
        class_extra: i32,
        window_extra: i32,
        instance: *mut c_void,
        icon: *mut c_void,
        cursor: *mut c_void,
        background: *mut c_void,
        menu: *const u16,
        name: *const u16,
    }
    #[repr(C)]
    struct Message {
        hwnd: Hwnd,
        message: u32,
        wparam: usize,
        lparam: isize,
        time: u32,
        point: [i32; 2],
        private: u32,
    }
    #[link(name = "user32")]
    unsafe extern "system" {
        fn FindWindowW(class: *const u16, title: *const u16) -> Hwnd;
        fn RegisterClassW(class: *const Class) -> u16;
        fn CreateWindowExW(
            ex: u32,
            class: *const u16,
            title: *const u16,
            style: u32,
            x: i32,
            y: i32,
            w: i32,
            h: i32,
            parent: Hwnd,
            menu: *mut c_void,
            instance: *mut c_void,
            param: *mut c_void,
        ) -> Hwnd;
        fn DestroyWindow(hwnd: Hwnd) -> i32;
        fn DefWindowProcW(hwnd: Hwnd, message: u32, wparam: usize, lparam: isize) -> isize;
        fn SendMessageTimeoutW(
            hwnd: Hwnd,
            message: u32,
            wparam: usize,
            lparam: isize,
            flags: u32,
            timeout: u32,
            result: *mut usize,
        ) -> isize;
        fn PeekMessageW(message: *mut Message, hwnd: Hwnd, min: u32, max: u32, remove: u32) -> i32;
        fn TranslateMessage(message: *const Message) -> i32;
        fn DispatchMessageW(message: *const Message) -> isize;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetModuleHandleW(name: *const u16) -> *mut c_void;
        fn GetLastError() -> u32;
        fn SetLastError(error: u32);
    }
    thread_local! {
        static REPLY:RefCell<Option<Vec<u8>>>=const {RefCell::new(None)};
        static WINDOW:RefCell<Option<Window>>=const {RefCell::new(None)};
        static ACTIVE_TAG:std::cell::Cell<u32>=const {std::cell::Cell::new(0x534c4154)};
    }
    unsafe extern "system" fn procedure(
        hwnd: Hwnd,
        message: u32,
        wparam: usize,
        lparam: isize,
    ) -> isize {
        if message == 0x4a && lparam != 0 {
            let data = unsafe { &*(lparam as *const CopyData) };
            if ACTIVE_TAG.with(|tag| data.tag == tag.get() as usize)
                && !data.data.is_null()
                && (20..=16_000_000).contains(&data.len)
            {
                let bytes =
                    unsafe { std::slice::from_raw_parts(data.data, data.len as usize) }.to_vec();
                REPLY.with(|reply| *reply.borrow_mut() = Some(bytes));
                return 1;
            }
        }
        unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
    }
    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(Some(0)).collect()
    }
    struct Window(Hwnd);
    impl Drop for Window {
        fn drop(&mut self) {
            unsafe {
                DestroyWindow(self.0);
            }
        }
    }
    #[cfg(test)]
    pub fn query(expression: &str, offset: usize) -> Result<Page, String> {
        query_server("EVERYTHING_TASKBAR_NOTIFICATION", expression, offset)
    }
    #[cfg(test)]
    fn query_server(server_name: &str, expression: &str, offset: usize) -> Result<Page, String> {
        parse(&exchange(
            server_name,
            expression,
            offset,
            2,
            256,
            &[],
            5000,
            12,
            &|| false,
        )?)
    }
    pub(super) fn inventory(expression: &str, offset: usize) -> Result<Vec<u8>, String> {
        exchange(
            "EVERYTHING_TASKBAR_NOTIFICATION",
            expression,
            offset,
            18,
            8192,
            &[0x54, 1],
            5000,
            12,
            &|| false,
        )
    }
    pub(super) fn exchange(
        server_name: &str,
        expression: &str,
        offset: usize,
        kind: usize,
        limit: u32,
        extra: &[u32],
        reply_timeout: u64,
        flags: u32,
        cancel: &dyn Fn() -> bool,
    ) -> Result<Vec<u8>, String> {
        if cancel() {
            return Err("cancelled".into());
        }
        let server_class = wide(server_name);
        let server = unsafe { FindWindowW(server_class.as_ptr(), std::ptr::null()) };
        if server.is_null() {
            return Err("Everything is not running (service alone is insufficient)".into());
        }
        // IS_DB_LOADED (WM_USER/401) is advisory and can be filtered independently
        // of public WM_COPYDATA. Like the official SDK query path, try the actual
        // query instead of treating that preflight as an authorization/readiness gate.
        let handle = WINDOW.with(|slot| -> Result<Hwnd, String> {
            let mut slot = slot.borrow_mut();
            if let Some(window) = slot.as_ref() {
                return Ok(window.0);
            }
            let class_name = wide("SlateEverythingReply");
            let instance = unsafe { GetModuleHandleW(std::ptr::null()) };
            let class = Class {
                style: 0,
                proc: Some(procedure),
                class_extra: 0,
                window_extra: 0,
                instance,
                icon: std::ptr::null_mut(),
                cursor: std::ptr::null_mut(),
                background: std::ptr::null_mut(),
                menu: std::ptr::null(),
                name: class_name.as_ptr(),
            };
            unsafe {
                RegisterClassW(&class);
            }
            let handle = unsafe {
                CreateWindowExW(
                    0,
                    class_name.as_ptr(),
                    std::ptr::null(),
                    0,
                    0,
                    0,
                    0,
                    0,
                    (-3_isize) as Hwnd,
                    std::ptr::null_mut(),
                    instance,
                    std::ptr::null_mut(),
                )
            };
            if handle.is_null() {
                return Err("Could not create Everything reply window".into());
            }
            *slot = Some(Window(handle));
            Ok(handle)
        })?;
        let tag = ACTIVE_TAG.with(|tag| {
            let next = tag.get().wrapping_add(1);
            tag.set(next);
            next
        });
        REPLY.with(|reply| *reply.borrow_mut() = None);
        let mut bytes = Vec::new();
        for value in [handle as usize as u32, tag, flags, offset as u32, limit]
            .iter()
            .chain(extra)
        {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for value in wide(expression) {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        let data = CopyData {
            tag: kind,
            len: bytes.len() as u32,
            data: bytes.as_ptr(),
        };
        let mut accepted = 0;
        unsafe {
            SetLastError(0);
        }
        let sent = unsafe {
            SendMessageTimeoutW(
                server,
                0x4a,
                handle as usize,
                &data as *const _ as isize,
                2,
                300,
                &mut accepted,
            )
        };
        if sent == 0 {
            let error = unsafe { GetLastError() };
            crate::perf_log::mark(
                "periscope.ipc_failure",
                &format!("stage=send win32={error}"),
            );
            return Err(match error {5=>"Everything query IPC denied by Windows (Win32 5; process integrity mismatch possible)".into(),1460=>"Everything query send timed out (Win32 1460)".into(),_=>format!("Everything query send failed (Win32 {error})")});
        }
        if accepted == 0 {
            crate::perf_log::mark("periscope.ipc_failure", "stage=query_rejected");
            return Err(if kind == 18 {
                "Everything rejected Unicode QUERY2 metadata IPC (requires 1.4.1+)".into()
            } else {
                "Everything rejected Unicode query IPC (not a readiness probe)".into()
            });
        }
        let start = Instant::now();
        loop {
            if cancel() {
                return Err("cancelled".into());
            }
            if let Some(bytes) = REPLY.with(|reply| reply.borrow_mut().take()) {
                return Ok(bytes);
            }
            if start.elapsed() > Duration::from_millis(reply_timeout) {
                crate::perf_log::mark("periscope.ipc_failure", "stage=reply_timeout");
                return Err("Everything accepted query but reply timed out".into());
            }
            let mut message: Message = unsafe { std::mem::zeroed() };
            while unsafe { PeekMessageW(&mut message, std::ptr::null_mut(), 0, 0, 1) } != 0 {
                unsafe {
                    TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    #[cfg(test)]
    mod regression {
        use super::*;
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
            mpsc,
        };
        unsafe extern "system" fn mock(
            hwnd: Hwnd,
            message: u32,
            wparam: usize,
            lparam: isize,
        ) -> isize {
            // Deliberately does not implement IS_DB_LOADED, just public query IPC.
            if message == 0x400 {
                return 0;
            }
            if message == 0x4a && lparam != 0 {
                let copy = unsafe { &*(lparam as *const CopyData) };
                if copy.tag == 2 && !copy.data.is_null() && copy.len >= 22 {
                    let request =
                        unsafe { std::slice::from_raw_parts(copy.data, copy.len as usize) };
                    if request.get(20..22) == Some(&[b'r', 0]) {
                        return 0;
                    }
                    let reply = word(request, 0).unwrap() as usize as Hwnd;
                    let tag = word(request, 4).unwrap() as usize;
                    let bytes = [0_u8; 28];
                    // A stale response must not be accepted after a new query tag.
                    let stale = CopyData {
                        tag: tag.wrapping_sub(1),
                        len: 28,
                        data: bytes.as_ptr(),
                    };
                    let mut ignored = 0;
                    unsafe {
                        SendMessageTimeoutW(
                            reply,
                            0x4a,
                            hwnd as usize,
                            &stale as *const _ as isize,
                            2,
                            500,
                            &mut ignored,
                        );
                    }
                    assert_eq!(ignored, 0);
                    let packet = CopyData {
                        tag,
                        len: 28,
                        data: bytes.as_ptr(),
                    };
                    let mut result = 0;
                    unsafe {
                        SendMessageTimeoutW(
                            reply,
                            0x4a,
                            hwnd as usize,
                            &packet as *const _ as isize,
                            2,
                            500,
                            &mut result,
                        );
                    }
                    return 1;
                }
            }
            unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
        }
        struct Server {
            stop: Arc<AtomicBool>,
            thread: Option<std::thread::JoinHandle<()>>,
            name: String,
        }
        impl Drop for Server {
            fn drop(&mut self) {
                self.stop.store(true, Ordering::Relaxed);
                if let Some(thread) = self.thread.take() {
                    thread.join().unwrap();
                }
            }
        }
        #[test]
        fn public_query_works_without_readiness_api_and_rejection_is_distinguished() {
            let name = format!(
                "SlateEverythingMock{}",
                chrono::Utc::now().timestamp_nanos_opt().unwrap()
            );
            let stop = Arc::new(AtomicBool::new(false));
            let (tx, rx) = mpsc::channel();
            let thread = {
                let name = name.clone();
                let stop = stop.clone();
                std::thread::spawn(move || {
                    let wide_name = wide(&name);
                    let instance = unsafe { GetModuleHandleW(std::ptr::null()) };
                    let class = Class {
                        style: 0,
                        proc: Some(mock),
                        class_extra: 0,
                        window_extra: 0,
                        instance,
                        icon: std::ptr::null_mut(),
                        cursor: std::ptr::null_mut(),
                        background: std::ptr::null_mut(),
                        menu: std::ptr::null(),
                        name: wide_name.as_ptr(),
                    };
                    assert_ne!(unsafe { RegisterClassW(&class) }, 0);
                    let handle = unsafe {
                        CreateWindowExW(
                            0,
                            wide_name.as_ptr(),
                            std::ptr::null(),
                            0,
                            0,
                            0,
                            0,
                            0,
                            std::ptr::null_mut(),
                            std::ptr::null_mut(),
                            instance,
                            std::ptr::null_mut(),
                        )
                    };
                    assert!(!handle.is_null());
                    let _window = Window(handle);
                    tx.send(()).unwrap();
                    while !stop.load(Ordering::Relaxed) {
                        let mut msg: Message = unsafe { std::mem::zeroed() };
                        while unsafe { PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, 1) } != 0
                        {
                            unsafe {
                                TranslateMessage(&msg);
                                DispatchMessageW(&msg);
                            }
                        }
                        std::thread::sleep(Duration::from_millis(1));
                    }
                })
            };
            let server = Server {
                name,
                stop,
                thread: Some(thread),
            };
            rx.recv_timeout(Duration::from_secs(3)).unwrap();
            let page = query_server(&server.name, "ok", 0).unwrap();
            assert_eq!(page.total, 0);
            let error = query_server(&server.name, "reject", 0).unwrap_err();
            assert!(error.contains("rejected"));
            assert!(!error.contains("readiness probe") || error.contains("not a readiness probe"));
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_words_use_scoped_plain_search_and_operators_cannot_change_scope() {
        let roots = vec![PathBuf::from("C:/"), PathBuf::from("D:/my notes")];
        let (search, path) = native_search("abrir mi", &roots).unwrap();
        assert_eq!(
            search,
            "file: <path:\"C:\\\" | path:\"D:\\my notes\\\"> \"abrir\" \"mi\""
        );
        assert!(!path);
        assert!(native_search("folder/file", &roots).unwrap().1);
        for query in ["size:>1gb", "foo | bar", "\" x", "!foo", "*foo", "a\nb"] {
            assert!(native_search(query, &roots).is_none());
        }
        assert!(native_search("x", &[PathBuf::from("D:/bad\"root")]).is_none());
    }
    #[test]
    #[ignore = "manual read-only integration against an already running Everything"]
    fn live_everything_finds_large_document_outside_home() {
        let root = PathBuf::from("D:/slate-baselines");
        let started = std::time::Instant::now();
        let page = query(&expression("large-document", &[root.clone()]), 0).unwrap();
        println!(
            "EVERYTHING_LIVE total={} returned={} wall_ms={:.3}",
            page.total,
            page.returned,
            started.elapsed().as_secs_f64() * 1000.0
        );
        assert!(
            page.paths
                .iter()
                .any(|path| path == &root.join("large-document-5m.md")),
            "{:?}",
            page.paths
        );
    }
    #[test]
    fn scopes_and_user_input_cannot_inject_regex_or_search_commands() {
        let value = expression("a.* | café", &[PathBuf::from("D:/notes")]);
        assert!(value.starts_with("^(?:D:\\\\notes[\\\\/])"));
        assert!(value.contains(".*\\..*\\*"));
        assert!(value.contains(".*\\|"));
    }
    #[test]
    fn rejects_truncated_offsets_and_unterminated_unicode() {
        for bytes in [vec![], vec![0; 27]] {
            assert!(parse(&bytes).is_err());
        }
        let mut bytes = vec![0; 40];
        bytes[20..24].copy_from_slice(&1_u32.to_le_bytes());
        bytes[32..36].copy_from_slice(&40_u32.to_le_bytes());
        assert!(parse(&bytes).is_err());
    }
    #[test]
    fn parses_unicode_files_and_ignores_folders() {
        let mut data = vec![0; 52];
        data[8..12].copy_from_slice(&2_u32.to_le_bytes());
        data[20..24].copy_from_slice(&2_u32.to_le_bytes());
        data[40..44].copy_from_slice(&1_u32.to_le_bytes());
        let name = data.len() as u32;
        for ch in "café.md".encode_utf16().chain(Some(0)) {
            data.extend_from_slice(&ch.to_le_bytes());
        }
        let parent = data.len() as u32;
        for ch in "D:\\notes".encode_utf16().chain(Some(0)) {
            data.extend_from_slice(&ch.to_le_bytes());
        }
        data[32..36].copy_from_slice(&name.to_le_bytes());
        data[36..40].copy_from_slice(&parent.to_le_bytes());
        let result = parse(&data).unwrap();
        assert_eq!(result.returned, 2);
        assert_eq!(result.paths.len(), 1);
        assert!(result.paths[0].to_string_lossy().ends_with("café.md"));
    }
}
