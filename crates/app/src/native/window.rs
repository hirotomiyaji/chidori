//! What GPUI leaves to the platform: the borderless window's shadow and corners, keeping it out
//! of screen captures, and one running copy of the app (FR-91).
//!
//! The handle is the HWND on Windows and the window's NSView on macOS.

#[cfg(any(windows, unix))]
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
    #[cfg(target_os = "macos")]
    if let Some(w) = mac::window(hwnd) {
        use objc2_app_kit::NSWindowSharingType;
        w.setSharingType(if excluded { NSWindowSharingType::None } else { NSWindowSharingType::ReadOnly });
    }
    #[cfg(not(any(windows, target_os = "macos")))]
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
    #[cfg(target_os = "macos")]
    if let Some(w) = mac::window(hwnd) {
        if visible {
            if w.isMiniaturized() {
                w.deminiaturize(None);
            }
            w.makeKeyAndOrderFront(None);
            mac::activate();
        } else {
            w.orderOut(None);
        }
    }
    #[cfg(not(any(windows, target_os = "macos")))]
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
    #[cfg(target_os = "macos")]
    {
        mac::window(hwnd).is_some_and(|w| w.isMiniaturized() || !w.isVisible())
    }
    #[cfg(not(any(windows, target_os = "macos")))]
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
    #[cfg(target_os = "macos")]
    if let Some(w) = mac::window(hwnd) {
        // NSFloatingWindowLevel and NSNormalWindowLevel.
        w.setLevel(if on { 3 } else { 0 });
    }
    #[cfg(not(any(windows, target_os = "macos")))]
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
    #[cfg(unix)]
    {
        use std::io::Read;
        use std::os::unix::net::{UnixListener, UnixStream};
        // The user's own temp folder, so another user's copy is not this one.
        let path = std::env::temp_dir().join(format!("{}.sock", chidori_core::host::IDENTIFIER));
        let listener = match UnixListener::bind(&path) {
            Ok(l) => l,
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
                if UnixStream::connect(&path).is_ok() {
                    return false;
                }
                // Left by a copy that crashed.
                let _ = std::fs::remove_file(&path);
                match UnixListener::bind(&path) {
                    Ok(l) => l,
                    Err(e) => {
                        tracing::warn!("single instance socket: {e}");
                        return true;
                    }
                }
            }
            Err(e) => {
                tracing::warn!("single instance socket: {e}");
                return true;
            }
        };
        std::thread::Builder::new()
            .name("second-launch".into())
            .spawn(move || {
                for mut conn in listener.incoming().flatten() {
                    let _ = conn.read(&mut [0u8; 1]);
                    bridge::send(UiEvent::SecondInstance);
                }
            })
            .ok();
        true
    }
    #[cfg(not(any(windows, unix)))]
    true
}

#[cfg(target_os = "macos")]
mod mac {
    use objc2::MainThreadMarker;
    use objc2::rc::Retained;
    use objc2_app_kit::{NSApplication, NSView, NSWindow};

    /// The window holding the app's view. AppKit is main-thread only, as GPUI's UI thread is.
    pub fn window(view: u64) -> Option<Retained<NSWindow>> {
        MainThreadMarker::new()?;
        let view = view as usize as *const NSView;
        // SAFETY: `view` is the app's own live NSView, from GPUI's window handle, and this runs
        // on the main thread.
        unsafe { view.as_ref() }?.window()
    }

    /// Brings the app forward, as SetForegroundWindow does.
    pub fn activate() {
        if let Some(mtm) = MainThreadMarker::new() {
            #[allow(deprecated)]
            NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
        }
    }
}
