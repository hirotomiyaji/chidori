//! Everything that reaches the UI from outside its own update loop, in one channel: the core's
//! events, the tray, menu picks, hotkeys and a second launch. Also the core's `Shell`.

use std::sync::OnceLock;

use chidori_core::events::{Event, EventSink};
use chidori_core::host::{AppHandle, Shell};
use parking_lot::Mutex;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

#[derive(Debug, Clone)]
pub enum UiEvent {
    Core(Event),
    /// The tray icon was left-clicked.
    TrayClick,
    /// An item of a tray or popup menu was picked, by its id.
    Menu(String),
    /// Another launch of the app asked this one to show itself (FR-91).
    SecondInstance,
    /// The core wants the app to quit (a recording finished saving after 終了).
    Exit,
}

static TX: OnceLock<UnboundedSender<UiEvent>> = OnceLock::new();
static RX: Mutex<Option<UnboundedReceiver<UiEvent>>> = Mutex::new(None);

/// Opens the channel; call once, before anything sends.
pub fn init() {
    let (tx, rx) = unbounded_channel();
    if TX.set(tx).is_ok() {
        *RX.lock() = Some(rx);
    }
}

pub fn send(event: UiEvent) {
    if let Some(tx) = TX.get() {
        let _ = tx.send(event);
    }
}

/// The receiving end, taken once by the window, which hands each event to the app.
pub fn take_receiver() -> Option<UnboundedReceiver<UiEvent>> {
    RX.lock().take()
}

/// The core's events go straight into the channel.
pub struct Events;

impl EventSink for Events {
    fn emit(&self, event: Event) {
        send(UiEvent::Core(event));
    }
}

/// What the core asks of the shell.
pub struct AppShell;

impl Shell for AppShell {
    fn window_hidden(&self) -> bool {
        chidori_core::host::ui_hidden()
    }

    fn notify(&self, text: &str) {
        crate::native::notify::show(text);
    }

    fn write_clipboard(&self, text: &str) -> anyhow::Result<()> {
        arboard::Clipboard::new()?.set_text(text)?;
        Ok(())
    }

    fn exit(&self) {
        send(UiEvent::Exit);
    }
}

static CORE: OnceLock<AppHandle> = OnceLock::new();

/// The core's handle, for callbacks that run outside the UI (hotkeys, tray items).
pub fn set_core(app: AppHandle) {
    let _ = CORE.set(app);
}

pub fn core() -> Option<&'static AppHandle> {
    CORE.get()
}
