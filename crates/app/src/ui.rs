//! The glue between GPUI and the app's message loop. The app keeps the shape it had under Iced: one
//! `App` state, `Message`s that change it in `App::update`, and `Task`s that update hands back.
//! Everything that happens in the window (a click, a key, text typed into a box, a core call
//! coming back) becomes a message here, queued and handled in order once GPUI is done with the
//! event, so an update never runs inside another.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use gpui_kit::component::input::{Input, InputEvent, InputState, RopeExt as _, Textarea, TextareaState};
use gpui_kit::{
    AnyElement, AnyWindowHandle, AppContext as _, AsyncApp, ClickEvent, Entity, InteractiveElement, IntoElement,
    ParentElement, SharedString, Stateful, StatefulInteractiveElement, Styled, Subscription, WeakEntity, Window, div,
};

use crate::app::{AppView, Message};
use crate::task::{Cmd, Task};

pub type El = AnyElement;
/// GPUI's app context (the app's own state is `crate::app::App`).
pub type Cx = gpui_kit::App;

enum Item {
    Msg(Message),
    Cmd(Cmd),
}

#[derive(Default)]
struct Queue {
    items: Vec<Item>,
    scheduled: bool,
    target: Option<(AnyWindowHandle, WeakEntity<AppView>)>,
}

thread_local! {
    static QUEUE: RefCell<Queue> = RefCell::new(Queue::default());
    static IDS: RefCell<HashMap<u64, u32>> = RefCell::new(HashMap::new());
    static INPUTS: RefCell<HashMap<SharedString, Slot>> = RefCell::new(HashMap::new());
    static PENDING_FOCUS: RefCell<Option<SharedString>> = const { RefCell::new(None) };
    static RENDERED: Cell<u64> = const { Cell::new(0) };
    /// The text boxes built since the last frame began.
    static SEEN: RefCell<HashSet<SharedString>> = RefCell::new(HashSet::new());
}

/// Where messages go: the window and the view that holds the app.
pub fn install(window: AnyWindowHandle, view: WeakEntity<AppView>) {
    QUEUE.with(|q| q.borrow_mut().target = Some((window, view)));
}

/// Hands a message to the app, after whatever GPUI is doing now.
pub fn send(msg: Message, cx: &mut Cx) {
    push(Item::Msg(msg), cx);
}

fn push(item: Item, cx: &mut Cx) {
    let schedule = QUEUE.with(|q| {
        let mut q = q.borrow_mut();
        q.items.push(item);
        !std::mem::replace(&mut q.scheduled, true)
    });
    if schedule {
        cx.defer(flush);
    }
}

fn flush(cx: &mut Cx) {
    let (items, target) = QUEUE.with(|q| {
        let mut q = q.borrow_mut();
        q.scheduled = false;
        (std::mem::take(&mut q.items), q.target.clone())
    });
    let Some((window, view)) = target else { return };
    let Some(view) = view.upgrade() else { return };
    let r = window.update(cx, |_, window, cx| {
        view.update(cx, |v, cx| {
            for item in items {
                match item {
                    Item::Msg(m) => {
                        let task = v.app.update(m);
                        run(task, cx);
                    }
                    Item::Cmd(c) => v.apply(c, window, cx),
                }
            }
            v.after_update(cx);
            cx.notify();
        })
    });
    if let Err(e) = r {
        tracing::debug!("no window for queued messages: {e}");
    }
}

/// Carries out a task: messages are queued, core calls run and answer later, window commands
/// are applied in order with the messages.
pub fn run(task: Task, cx: &mut Cx) {
    match task {
        Task::None => {}
        Task::Done(m) => push(Item::Msg(m), cx),
        Task::Cmd(c) => push(Item::Cmd(c), cx),
        Task::Batch(v) => {
            for t in v {
                run(t, cx);
            }
        }
        Task::Future(f) => cx
            .spawn(async move |cx: &mut AsyncApp| {
                let m = f.await;
                cx.update(|cx| send(m, cx));
            })
            .detach(),
        Task::Chain(a, b) if a.is_instant() => {
            run(*a, cx);
            run(*b, cx);
        }
        chain => cx.spawn(async move |cx: &mut AsyncApp| exec(chain, cx.clone()).await).detach(),
    }
}

/// A task run to its end, every message it produced queued before it returns.
fn exec(task: Task, cx: AsyncApp) -> futures::future::LocalBoxFuture<'static, ()> {
    Box::pin(async move {
        match task {
            Task::Future(f) => {
                let m = f.await;
                cx.update(|cx| send(m, cx));
            }
            Task::Batch(v) => {
                futures::future::join_all(v.into_iter().map(|t| exec(t, cx.clone()))).await;
            }
            Task::Chain(a, b) => {
                exec(*a, cx.clone()).await;
                exec(*b, cx).await;
            }
            other => cx.update(|cx| run(other, cx)),
        }
    })
}

/// A click handler that sends a message; the click goes no further (a button in a row that is a
/// button itself takes the click for itself, as Iced's did).
pub fn on(msg: Message) -> impl Fn(&ClickEvent, &mut Window, &mut Cx) + 'static {
    move |_, _, cx| {
        cx.stop_propagation();
        send(msg.clone(), cx);
    }
}

/// Starts a frame's element IDs afresh, and lets go of the text boxes the last frame didn't build
/// (lists build their rows after the view, so this waits for the next frame), so a box opened
/// again starts afresh.
pub fn begin_frame() {
    IDS.with(|ids| ids.borrow_mut().clear());
    let seen = SEEN.with(|s| std::mem::take(&mut *s.borrow_mut()));
    INPUTS.with(|i| i.borrow_mut().retain(|k, _| seen.contains(k)));
    RENDERED.with(|r| r.set(r.get() + 1));
}

/// An element ID from something that names it (its message, its label), stable from frame to
/// frame: the n-th element named the same way in a frame gets the same ID each time.
pub fn uid(seed: impl Hash) -> gpui_kit::ElementId {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    seed.hash(&mut h);
    let base = h.finish();
    let n = IDS.with(|ids| {
        let mut ids = ids.borrow_mut();
        let n = ids.entry(base).or_insert(0);
        *n += 1;
        *n
    });
    gpui_kit::ElementId::Integer(base.wrapping_add(n as u64))
}

/// The ID of an element that sends a message.
pub fn msg_id(msg: &Message) -> gpui_kit::ElementId {
    uid(format!("{msg:?}"))
}

/// A box that sends `msg` when clicked, with the pointer cursor; inert without one.
pub fn clickable(msg: Option<Message>, seed: impl Hash) -> Stateful<gpui_kit::Div> {
    let id = match &msg {
        Some(m) => msg_id(m),
        None => uid(seed),
    };
    let d = div().id(id);
    match msg {
        Some(m) => d.cursor_pointer().on_click(on(m)),
        None => d,
    }
}

/// What a view needs from GPUI while it is built: the window and the app context, for the text
/// boxes, which are GPUI entities that live between frames.
pub struct Ui<'a> {
    pub window: &'a mut Window,
    pub cx: &'a mut Cx,
}

type OnChange = Rc<dyn Fn(String) -> Message>;

/// A one-line box or a multi-line one.
#[derive(Clone)]
enum Boxed {
    Line(Entity<InputState>),
    Area(Entity<TextareaState>),
}

/// A text box between frames: its GPUI state, and what its typing and Enter send.
struct Slot {
    state: Boxed,
    on_change: OnChange,
    on_submit: Option<Message>,
    /// The value the app last gave it, so a change on the app's side is copied in.
    value: String,
    /// A search box: a magnifier at its start and a clear button once it has text.
    search: bool,
    _sub: Subscription,
}

/// Options for a text box.
pub struct Field<'a> {
    pub key: SharedString,
    pub value: &'a str,
    pub placeholder: String,
    pub multi_line: bool,
    pub search: bool,
    pub on_change: OnChange,
    pub on_submit: Option<Message>,
}

impl<'a> Field<'a> {
    pub fn new(key: impl Into<SharedString>, value: &'a str, on_change: impl Fn(String) -> Message + 'static) -> Self {
        Self {
            key: key.into(),
            value,
            placeholder: String::new(),
            multi_line: false,
            search: false,
            on_change: Rc::new(on_change),
            on_submit: None,
        }
    }

    pub fn placeholder(mut self, p: impl Into<String>) -> Self {
        self.placeholder = p.into();
        self
    }

    pub fn on_submit(mut self, m: Message) -> Self {
        self.on_submit = Some(m);
        self
    }

    pub fn multi_line(mut self) -> Self {
        self.multi_line = true;
        self
    }

    pub fn search(mut self) -> Self {
        self.search = true;
        self
    }
}

/// What a box's events send.
fn on_event(key: &SharedString, value: String, ev: &InputEvent, multi: bool, cx: &mut Cx) {
    let slot =
        INPUTS.with(|i| i.borrow().get(key).map(|s| (s.on_change.clone(), s.on_submit.clone(), s.value.clone())));
    let Some((on_change, on_submit, known)) = slot else { return };
    match ev {
        InputEvent::Change => {
            if value != known {
                INPUTS.with(|i| {
                    if let Some(s) = i.borrow_mut().get_mut(key) {
                        s.value = value.clone();
                    }
                });
                send(on_change(value), cx);
            }
        }
        InputEvent::PressEnter { secondary, .. } => {
            if (!multi || *secondary)
                && let Some(m) = on_submit
            {
                send(m, cx);
            }
        }
        _ => {}
    }
}

impl Ui<'_> {
    /// A text box kept between frames by its key. Its value follows the app's (`value`); what is
    /// typed comes back as `on_change`'s message, Enter (Ctrl+Enter in a multi-line box) as
    /// `on_submit`'s.
    pub fn input(&mut self, f: Field<'_>) -> AnyElement {
        SEEN.with(|s| s.borrow_mut().insert(f.key.clone()));
        let exists = INPUTS.with(|i| i.borrow().contains_key(&f.key));
        if !exists {
            let placeholder = f.placeholder.clone();
            let value = f.value.to_string();
            let key = f.key.clone();
            let (state, sub) = if f.multi_line {
                let state = self.cx.new(|cx| {
                    let mut s = TextareaState::new(self.window, cx).placeholder(placeholder).auto_grow(3, 14);
                    s.set_value(value, self.window, cx);
                    s
                });
                let sub = self.cx.subscribe(&state, move |state, ev: &InputEvent, cx| {
                    let v = state.read(cx).value().to_string();
                    on_event(&key, v, ev, true, cx);
                });
                (Boxed::Area(state), sub)
            } else {
                let state = self.cx.new(|cx| {
                    let mut s = InputState::new(self.window, cx).placeholder(placeholder);
                    s.set_value(value, self.window, cx);
                    s
                });
                let sub = self.cx.subscribe(&state, move |state, ev: &InputEvent, cx| {
                    let v = state.read(cx).value().to_string();
                    on_event(&key, v, ev, false, cx);
                });
                (Boxed::Line(state), sub)
            };
            INPUTS.with(|i| {
                i.borrow_mut().insert(
                    f.key.clone(),
                    Slot {
                        state,
                        on_change: f.on_change.clone(),
                        on_submit: f.on_submit.clone(),
                        value: f.value.to_string(),
                        search: f.search,
                        _sub: sub,
                    },
                )
            });
        }
        let (state, changed) = INPUTS.with(|i| {
            let mut i = i.borrow_mut();
            let s = i.get_mut(&f.key).expect("slot just made");
            s.on_change = f.on_change;
            s.on_submit = f.on_submit;
            let changed = s.value != f.value;
            if changed {
                s.value = f.value.to_string();
            }
            (s.state.clone(), changed)
        });
        let v = f.value.to_string();
        let window = &mut *self.window;
        match &state {
            Boxed::Line(st) if changed => st.update(self.cx, |s, cx| {
                if s.value().as_ref() != v {
                    s.set_value(v, window, cx);
                }
            }),
            Boxed::Area(st) if changed => st.update(self.cx, |s, cx| {
                if s.value().as_ref() != v {
                    s.set_value(v, window, cx);
                }
            }),
            _ => {}
        }
        let pending = PENDING_FOCUS.with(|p| p.borrow().as_ref() == Some(&f.key));
        if pending {
            PENDING_FOCUS.with(|p| p.borrow_mut().take());
            focus_state(&state, self.window, self.cx);
        }
        element(&state, f.search)
    }
}

fn element(state: &Boxed, search: bool) -> AnyElement {
    match state {
        Boxed::Line(st) if search => Input::new(st)
            .prefix(crate::icons::icon(crate::icons::Icon::Search, 15.0, crate::theme::p().ink_muted))
            .cleanable(true)
            .into_any_element(),
        Boxed::Line(st) => Input::new(st).into_any_element(),
        Boxed::Area(st) => Textarea::new(st).into_any_element(),
    }
}

fn focus_state(state: &Boxed, window: &mut Window, cx: &mut Cx) {
    match state {
        Boxed::Line(st) => st.update(cx, |s, cx| {
            s.focus(window, cx);
            let end = s.text().len();
            let pos = s.text().offset_to_position(end);
            s.set_cursor_position(pos, window, cx);
        }),
        Boxed::Area(st) => st.update(cx, |s, cx| {
            s.focus(window, cx);
            let end = s.text().len();
            let pos = s.text().offset_to_position(end);
            s.set_cursor_position(pos, window, cx);
        }),
    }
}

/// The element of a text box already built this frame by `Ui::input`, for a part of the view
/// built later without the window (a list's rows).
pub fn built_input(key: &str) -> Option<AnyElement> {
    INPUTS.with(|i| i.borrow().get(key).map(|s| element(&s.state, s.search)))
}

/// Reads the app's state where only GPUI's context is at hand (a list building its rows).
pub fn read_app<R>(cx: &Cx, f: impl FnOnce(&crate::app::App) -> R) -> Option<R> {
    let view = QUEUE.with(|q| q.borrow().target.as_ref().map(|t| t.1.clone()))?;
    let view = view.upgrade()?;
    Some(f(&view.read(cx).app))
}

/// Focuses a text box, now or as soon as it is built.
pub fn focus(key: &SharedString, window: &mut Window, cx: &mut Cx) {
    let state = INPUTS.with(|i| i.borrow().get(key).map(|s| s.state.clone()));
    match state {
        Some(s) => focus_state(&s, window, cx),
        None => PENDING_FOCUS.with(|p| *p.borrow_mut() = Some(key.clone())),
    }
}

/// Focuses a text box when it is next built (a dialog's box, as the dialog opens).
pub fn focus_soon(key: impl Into<SharedString>) {
    PENDING_FOCUS.with(|p| *p.borrow_mut() = Some(key.into()));
}

/// A key pressed, named as the hotkey settings name it (`KeyA`, `Digit1`, `F5`, `Space`, `Escape`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyPress {
    pub code: String,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub logo: bool,
}

impl From<&gpui_kit::Keystroke> for KeyPress {
    fn from(k: &gpui_kit::Keystroke) -> Self {
        let m = &k.modifiers;
        Self { code: code_of(&k.key), ctrl: m.control, alt: m.alt, shift: m.shift, logo: m.platform }
    }
}

/// GPUI's name for a key (`a`, `1`, `f5`, `pageup`) in the hotkey settings' terms.
fn code_of(key: &str) -> String {
    let mut chars = key.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) if c.is_ascii_alphabetic() => return format!("Key{}", c.to_ascii_uppercase()),
        (Some(c), None) if c.is_ascii_digit() => return format!("Digit{c}"),
        _ => {}
    }
    if let Some(n) = key.strip_prefix('f').filter(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit())) {
        return format!("F{n}");
    }
    match key {
        "space" => "Space",
        "insert" => "Insert",
        "home" => "Home",
        "end" => "End",
        "pageup" => "PageUp",
        "pagedown" => "PageDown",
        "pause" => "Pause",
        "escape" => "Escape",
        "enter" => "Enter",
        "tab" => "Tab",
        "backspace" => "Backspace",
        "delete" => "Delete",
        other => other,
    }
    .to_string()
}

/// A wrapper with a tooltip; `tip` empty leaves it plain.
pub fn with_tip(content: impl IntoElement, tip: impl Into<String>) -> Stateful<gpui_kit::Div> {
    let tip: String = tip.into();
    let d = div().id(uid(("tip", &tip))).flex().flex_none().child(content);
    if tip.is_empty() {
        return d;
    }
    let text = SharedString::from(tip);
    d.tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(text.clone()).build(window, cx))
}
