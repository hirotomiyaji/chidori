//! What GPUI leaves to the platform: the borderless window's shadow and corners, keeping it out
//! of screen captures, and one running copy of the app (FR-91).

use crate::bridge::{self, UiEvent};

/// Gives the borderless window the system's shadow and, on Windows 11, its rounded corners.
pub fn apply_chrome(hwnd: u64) {
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::Graphics::Dwm::{
            DWM_WINDOW_CORNER_PREFERENCE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmExtendFrameIntoClientArea,
            DwmSetWindowAttribute,
        };
        use windows::Win32::UI::Controls::MARGINS;
        let hwnd = HWND(hwnd as isize as *mut _);
        let margins = MARGINS { cxLeftWidth: 1, cxRightWidth: 1, cyTopHeight: 1, cyBottomHeight: 1 };
        let corner: DWM_WINDOW_CORNER_PREFERENCE = DWMWCP_ROUND;
        // SAFETY: `hwnd` is the app's own live window; the attribute is a plain value read by size.
        unsafe {
            let _ = DwmExtendFrameIntoClientArea(hwnd, &margins);
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                (&raw const corner).cast(),
                size_of::<DWM_WINDOW_CORNER_PREFERENCE>() as u32,
            );
        }
    }
    #[cfg(not(windows))]
    let _ = hwnd;
}

/// Keeps the window out of screen sharing and screenshots (設定 › スクショ, on by default).
pub fn set_capture_excluded(hwnd: u64, excluded: bool) {
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::{SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE, WDA_NONE};
        let affinity = if excluded { WDA_EXCLUDEFROMCAPTURE } else { WDA_NONE };
        // SAFETY: `hwnd` is the app's own live window.
        if let Err(e) = unsafe { SetWindowDisplayAffinity(HWND(hwnd as isize as *mut _), affinity) } {
            tracing::warn!("capture exclusion: {e}");
        }
    }
    #[cfg(not(windows))]
    let _ = (hwnd, excluded);
}

#[cfg(windows)]
fn win(hwnd: u64) -> windows::Win32::Foundation::HWND {
    windows::Win32::Foundation::HWND(hwnd as isize as *mut _)
}

/// Shows the window (and brings it forward) or hides it to the tray. GPUI has no hidden state
/// of its own, so the window is hidden the Win32 way.
pub fn set_visible(hwnd: u64, visible: bool) {
    #[cfg(windows)]
    {
        use windows::Win32::UI::WindowsAndMessaging::{SW_HIDE, SW_RESTORE, SW_SHOW, SetForegroundWindow, ShowWindow};
        let h = win(hwnd);
        // SAFETY: `hwnd` is the app's own live window.
        unsafe {
            if visible {
                let _ = ShowWindow(h, SW_SHOW);
                if is_minimized(hwnd) {
                    let _ = ShowWindow(h, SW_RESTORE);
                }
                let _ = SetForegroundWindow(h);
            } else {
                let _ = ShowWindow(h, SW_HIDE);
            }
        }
    }
    #[cfg(not(windows))]
    let _ = (hwnd, visible);
}

/// Whether the window is minimized or hidden: nothing on screen to draw for.
pub fn is_minimized(hwnd: u64) -> bool {
    #[cfg(windows)]
    {
        use windows::Win32::UI::WindowsAndMessaging::{IsIconic, IsWindowVisible};
        let h = win(hwnd);
        // SAFETY: `hwnd` is the app's own live window.
        unsafe { IsIconic(h).as_bool() || !IsWindowVisible(h).as_bool() }
    }
    #[cfg(not(windows))]
    {
        let _ = hwnd;
        false
    }
}

/// Keeps the window above others, or not.
pub fn set_topmost(hwnd: u64, on: bool) {
    #[cfg(windows)]
    {
        use windows::Win32::UI::WindowsAndMessaging::{
            HWND_NOTOPMOST, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetWindowPos,
        };
        let after = if on { HWND_TOPMOST } else { HWND_NOTOPMOST };
        // SAFETY: `hwnd` is the app's own live window.
        if let Err(e) =
            unsafe { SetWindowPos(win(hwnd), Some(after), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE) }
        {
            tracing::warn!("always on top: {e}");
        }
    }
    #[cfg(not(windows))]
    let _ = (hwnd, on);
}

/// Makes this the one running copy. A second launch signals the first, which shows its window,
/// and returns `false` so the caller exits.
pub fn single_instance() -> bool {
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, WAIT_OBJECT_0};
        use windows::Win32::System::Threading::{CreateEventW, CreateMutexW, INFINITE, SetEvent, WaitForSingleObject};
        use windows::core::w;
        // SAFETY: plain Win32 calls on named objects this process creates or opens.
        unsafe {
            let event = match CreateEventW(None, false, false, w!("Local\\dev.yusai.chidori.show")) {
                Ok(e) => e,
                Err(e) => {
                    tracing::warn!("single instance event: {e}");
                    return true;
                }
            };
            let mutex = CreateMutexW(None, true, w!("Local\\dev.yusai.chidori"));
            if GetLastError() == ERROR_ALREADY_EXISTS {
                let _ = SetEvent(event);
                let _ = CloseHandle(event);
                return false;
            }
            // The mutex lives as long as the process.
            std::mem::forget(mutex);
            let event = event.0 as usize;
            std::thread::Builder::new()
                .name("second-launch".into())
                .spawn(move || {
                    let event = windows::Win32::Foundation::HANDLE(event as *mut _);
                    while WaitForSingleObject(event, INFINITE) == WAIT_OBJECT_0 {
                        bridge::send(UiEvent::SecondInstance);
                    }
                })
                .ok();
        }
        true
    }
    #[cfg(not(windows))]
    true
}
