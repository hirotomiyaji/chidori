//! Global hotkeys (FR-01, FR-06, FR-07, FR-30, FR-92). Off until set; a failure is reported in
//! 設定 only. Each runs on a thread of its own, so a slow start never holds up the message loop.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::str::FromStr;

use chidori_core::events::{self, Notice, NoticeLevel};
use chidori_core::host::AppHandle;
use chidori_core::recorder;
use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use parking_lot::Mutex;

type Action = fn(&AppHandle);

fn toggle(app: &AppHandle) {
    #[cfg(any(windows, target_os = "macos"))]
    recorder::toggle(app, app.state());
    #[cfg(not(any(windows, target_os = "macos")))]
    let _ = app;
}

fn screenshot(app: &AppHandle) {
    let _ = recorder::take_screenshot(app, app.state());
}

/// The button shows its own toast; a hotkey, pressed with another app in front, says what it did.
fn mark(app: &AppHandle) {
    let message = match recorder::mark_current(app.state()) {
        Ok(true) => "markAdded",
        Ok(false) => "nothingToMark",
        Err(_) => "shotOnlyWhileRecording",
    };
    events::notice(app, Notice::new(NoticeLevel::Info, "mark", message).toast());
}

fn cut(app: &AppHandle) {
    let message = if recorder::add_cut(app.state()).is_ok() { "cutAdded" } else { "shotOnlyWhileRecording" };
    events::notice(app, Notice::new(NoticeLevel::Info, "cut", message).toast());
}

thread_local! {
    static MANAGER: RefCell<Option<GlobalHotKeyManager>> = const { RefCell::new(None) };
    static REGISTERED: RefCell<Vec<HotKey>> = const { RefCell::new(Vec::new()) };
}

/// Hotkey id → what it does, read by the event handler.
static ACTIONS: Mutex<Option<HashMap<u32, Action>>> = Mutex::new(None);

/// Starts listening; on the main thread, whose message loop serves the hotkeys.
pub fn init() {
    match GlobalHotKeyManager::new() {
        Ok(m) => MANAGER.with(|cell| *cell.borrow_mut() = Some(m)),
        Err(e) => tracing::error!("global hotkeys unavailable: {e}"),
    }
    GlobalHotKeyEvent::set_event_handler(Some(|event: GlobalHotKeyEvent| {
        if event.state != HotKeyState::Pressed {
            return;
        }
        let action = ACTIONS.lock().as_ref().and_then(|a| a.get(&event.id).copied());
        if let (Some(action), Some(app)) = (action, crate::bridge::core()) {
            let app = app.clone();
            std::thread::spawn(move || action(&app));
        }
    }));
}

/// (Re)registers the hotkeys that are set (an empty one is off).
pub fn register_all(app: &AppHandle) {
    let st = app.state();
    let hotkeys = st.settings.read().hotkeys.clone();
    let mut errors = BTreeMap::new();
    let mut actions = HashMap::new();
    MANAGER.with(|cell| {
        let Some(manager) = &*cell.borrow() else { return };
        REGISTERED.with(|reg| {
            let mut reg = reg.borrow_mut();
            let _ = manager.unregister_all(&reg);
            reg.clear();
            let table: [(&str, &str, Action); 4] = [
                ("toggle", &hotkeys.toggle, toggle),
                ("screenshot", &hotkeys.screenshot, screenshot),
                ("mark", &hotkeys.mark, mark),
                ("cut", &hotkeys.cut, cut),
            ];
            for (name, accelerator, action) in table {
                if accelerator.is_empty() {
                    continue; // Off.
                }
                let hotkey = match HotKey::from_str(accelerator) {
                    Ok(h) => h,
                    Err(e) => {
                        errors.insert(name.to_string(), e.to_string());
                        continue;
                    }
                };
                if reg.iter().any(|r| r.id() == hotkey.id()) {
                    errors.insert(name.to_string(), "same as another shortcut".to_string());
                    continue;
                }
                match manager.register(hotkey) {
                    Ok(()) => {
                        actions.insert(hotkey.id(), action);
                        reg.push(hotkey);
                    }
                    Err(e) => {
                        tracing::warn!("{name} hotkey {accelerator} failed: {e}");
                        errors.insert(name.to_string(), e.to_string());
                    }
                }
            }
        });
    });
    *ACTIONS.lock() = Some(actions);
    *st.hotkey_errors.lock() = errors;
}

/// Checks that an accelerator string parses, without registering it.
pub fn validate(accelerator: &str) -> Result<(), String> {
    HotKey::from_str(accelerator).map(|_| ()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn accelerators_parse() {
        assert!(super::validate("Ctrl+Alt+R").is_ok());
        assert!(super::validate("Ctrl+Alt+S").is_ok());
        assert!(super::validate("Ctrl+Shift+F9").is_ok());
        assert!(super::validate("NotAKey+Q").is_err());
    }
}
