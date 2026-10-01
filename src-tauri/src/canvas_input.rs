//! Scoped protection against the native Alt+Space window menu while panning a canvas.

#[cfg(windows)]
use std::sync::{atomic::AtomicBool, Arc};

#[derive(Default)]
pub struct CanvasInputScope {
    #[cfg(windows)]
    active: Arc<AtomicBool>,
}

/// Called from setup on the window's owning UI thread; never wait inside the subclass.
pub fn install(app: &tauri::AppHandle) -> Result<(), String> {
    use tauri::Manager;
    let scope = CanvasInputScope::default();
    #[cfg(windows)]
    {
        let window = app
            .get_webview_window(crate::MAIN_WINDOW)
            .ok_or_else(|| "无法初始化画布键盘保护：主窗口不存在".to_string())?;
        platform::install(
            window.hwnd().map_err(|e| e.to_string())?,
            scope.active.clone(),
        )?;
    }
    app.manage(scope);
    Ok(())
}

#[tauri::command]
pub fn canvas_input_set_active(
    window: tauri::WebviewWindow,
    scope: tauri::State<'_, CanvasInputScope>,
    active: bool,
) -> crate::error::AppResult<()> {
    if window.label() != crate::MAIN_WINDOW {
        return Err(crate::error::AppError::validation("画布键盘保护仅限主窗口"));
    }
    #[cfg(windows)]
    scope
        .active
        .store(active, std::sync::atomic::Ordering::Relaxed);
    #[cfg(not(windows))]
    let _ = (scope, active);
    Ok(())
}

#[cfg(windows)]
mod platform {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
    use windows::Win32::UI::WindowsAndMessaging::{
        SC_KEYMENU, WM_ACTIVATE, WM_NCDESTROY, WM_SYSCOMMAND,
    };

    const SUBCLASS_ID: usize = 0x4c_43_49;

    pub(super) fn install(hwnd: HWND, active: Arc<AtomicBool>) -> Result<(), String> {
        // The subclass owns one Arc until WM_NCDESTROY. Installation and teardown both
        // run on the HWND's UI thread, as required by SetWindowSubclass.
        let data = Arc::into_raw(active);
        if !unsafe { SetWindowSubclass(hwnd, Some(canvas_window_proc), SUBCLASS_ID, data as usize) }
            .as_bool()
        {
            unsafe {
                drop(Arc::from_raw(data));
            }
            return Err("无法初始化画布键盘保护：安装 Windows 消息处理失败".to_string());
        }
        Ok(())
    }

    unsafe extern "system" fn canvas_window_proc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        id: usize,
        data: usize,
    ) -> LRESULT {
        let active = unsafe { &*(data as *const AtomicBool) };
        // SC_KEYMENU's lParam is the character used with Alt. Preserve mouse menus,
        // bare Alt/F10, Alt+F4 and all other window commands, including outside canvas.
        if message == WM_SYSCOMMAND
            && wparam.0 & 0xfff0 == SC_KEYMENU as usize
            && lparam.0 == 32
            && active.load(Ordering::Relaxed)
        {
            return LRESULT(0);
        }
        if message == WM_ACTIVATE && wparam.0 & 0xffff == 0 {
            active.store(false, Ordering::Relaxed);
        }
        if message == WM_NCDESTROY {
            unsafe {
                let _ = RemoveWindowSubclass(hwnd, Some(canvas_window_proc), id);
                drop(Arc::from_raw(data as *const AtomicBool));
            }
        }
        unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::sync::atomic::Ordering;
        use windows::core::w;
        use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
        use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, SendMessageW, WINDOW_EX_STYLE, WM_ACTIVATE,
            WM_SYSCOMMAND, WS_OVERLAPPED,
        };

        // Observe messages that would reach the OS menu without opening a blocking menu loop.
        unsafe extern "system" fn observe(
            hwnd: HWND,
            message: u32,
            wparam: WPARAM,
            lparam: LPARAM,
            _id: usize,
            _data: usize,
        ) -> LRESULT {
            if message == WM_SYSCOMMAND {
                return LRESULT(123);
            }
            unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
        }

        #[test]
        fn canvas_alt_space_blocks_only_the_keyboard_space_system_menu() {
            unsafe {
                let hwnd = CreateWindowExW(
                    WINDOW_EX_STYLE::default(),
                    w!("STATIC"),
                    w!("canvas-input-test"),
                    WS_OVERLAPPED,
                    0,
                    0,
                    10,
                    10,
                    None,
                    None,
                    None,
                    None,
                )
                .unwrap();
                assert!(SetWindowSubclass(hwnd, Some(observe), 2, 0).as_bool());
                let active = Arc::new(AtomicBool::new(false));
                install(hwnd, active.clone()).unwrap();
                let send = |command, character| {
                    SendMessageW(
                        hwnd,
                        WM_SYSCOMMAND,
                        Some(WPARAM(command)),
                        Some(LPARAM(character)),
                    )
                    .0
                };
                assert_eq!(
                    send(0xf100, 32),
                    123,
                    "outside canvas, Alt+Space retains the menu"
                );
                active.store(true, Ordering::Relaxed);
                assert_eq!(
                    send(0xf100, 32),
                    0,
                    "canvas Alt+Space must not reach the OS menu"
                );
                assert_eq!(send(0xf10f, 32), 0, "Windows reserves the low command bits");
                for (command, character) in [
                    (0xf100, 0),
                    (0xf100, 102),
                    (0xf090, 32),
                    (0xf060, 0),
                    (0xf020, 0),
                ] {
                    assert_eq!(
                        send(command, character),
                        123,
                        "other menus and window commands still work"
                    );
                }
                active.store(false, Ordering::Relaxed);
                assert_eq!(
                    send(0xf100, 32),
                    123,
                    "leaving the canvas restores Alt+Space"
                );
                active.store(true, Ordering::Relaxed);
                SendMessageW(hwnd, WM_ACTIVATE, Some(WPARAM(0)), Some(LPARAM(0)));
                assert_eq!(
                    send(0xf100, 32),
                    123,
                    "window deactivation clears stale canvas scope"
                );
                DestroyWindow(hwnd).unwrap();
                assert_eq!(
                    Arc::strong_count(&active),
                    1,
                    "destroying the HWND releases the subclass state"
                );
            }
        }
    }
}
