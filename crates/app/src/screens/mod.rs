//! The screens. Each has its own `State`, `Msg`, `update` and `view`; `app.rs` routes between them.

pub mod dialogs;
pub mod form;
pub mod history;
pub mod main_view;
pub mod models;
pub mod notices;
pub mod session_line;
pub mod session_menu;
pub mod settings;
pub mod titlebar;
pub mod viewer;
pub mod wizard;

use crate::app::App;

/// Whether a spinner shows anywhere, so the app ticks to turn it.
pub fn spinning(app: &App) -> bool {
    main_view::spinning(app)
        || models::spinning(app)
        || wizard::spinning(app)
        || settings::spinning(app)
        || dialogs::spinning(app)
}

/// What the screens keep fresh after every batch of messages (a list or deck off screen starts
/// afresh when it shows again).
pub fn housekeeping(app: &App) {
    main_view::housekeeping(app);
    viewer::housekeeping(app);
}
