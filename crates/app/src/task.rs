//! What an update asks for besides changing the app's state: messages to handle next, core calls
//! that answer with a message later, and things only the window can do. `ui::run` carries them out.

use std::future::Future;
use std::pin::Pin;

use gpui_kit::SharedString;

use crate::app::Message;

/// Something only the window, or the app as a whole, can do.
#[derive(Debug, Clone)]
pub enum Cmd {
    /// Focuses a text box (by its key), with the caret at the end of its text.
    Focus(SharedString),
    /// Sizes the window, keeping it at least `min` (logical px), resizable or not.
    Size {
        w: f32,
        h: f32,
        min_w: f32,
        min_h: f32,
        resizable: bool,
    },
    /// Keeps the window above others, or not.
    OnTop(bool),
    Minimize,
    Hide,
    Show,
    /// Quits the app.
    Exit,
}

pub type BoxFuture = Pin<Box<dyn Future<Output = Message>>>;

/// Work an update hands back, in the shape of Iced's `Task` that the screens were written for.
#[must_use]
pub enum Task {
    None,
    Done(Message),
    Future(BoxFuture),
    Cmd(Cmd),
    Batch(Vec<Task>),
    /// The second runs once the first, and every message it produced, has been handed on.
    Chain(Box<Task>, Box<Task>),
}

impl Task {
    pub fn none() -> Self {
        Task::None
    }

    pub fn done(m: Message) -> Self {
        Task::Done(m)
    }

    pub fn future(f: impl Future<Output = Message> + 'static) -> Self {
        Task::Future(Box::pin(f))
    }

    pub fn cmd(c: Cmd) -> Self {
        Task::Cmd(c)
    }

    pub fn batch(tasks: impl IntoIterator<Item = Task>) -> Self {
        let tasks: Vec<Task> = tasks.into_iter().filter(|t| !matches!(t, Task::None)).collect();
        match tasks.len() {
            0 => Task::None,
            _ => Task::Batch(tasks),
        }
    }

    pub fn chain(self, next: Task) -> Self {
        match (self, next) {
            (Task::None, n) => n,
            (s, Task::None) => s,
            (s, n) => Task::Chain(Box::new(s), Box::new(n)),
        }
    }

    /// Whether the task waits for nothing, so it can run at once.
    pub fn is_instant(&self) -> bool {
        match self {
            Task::None | Task::Done(_) | Task::Cmd(_) => true,
            Task::Future(_) => false,
            Task::Batch(v) => v.iter().all(Task::is_instant),
            Task::Chain(a, b) => a.is_instant() && b.is_instant(),
        }
    }
}

/// Focuses the text box with this key.
pub fn focus(key: impl Into<SharedString>) -> Task {
    Task::Cmd(Cmd::Focus(key.into()))
}

/// Quits the app.
pub fn exit() -> Task {
    Task::Cmd(Cmd::Exit)
}
