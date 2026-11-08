//! The UI's icons: Lucide's, as the React UI drew them (`brand/icons/`, ISC licence), tinted at draw
//! time.

use std::borrow::Cow;

use gpui_kit::{AssetSource, Radians, SharedString, Styled, Svg, Transformation, px, svg};

use crate::theme::Color;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Icon {
    ArrowDown,
    ArrowLeft,
    ArrowRightLeft,
    Camera,
    Check,
    ChevronDown,
    ChevronRight,
    CircleAlert,
    CircleCheckBig,
    Copy,
    Download,
    Ellipsis,
    FileAudio,
    FileCode,
    FileOutput,
    FileText,
    FileType,
    Gauge,
    History,
    ImagePlus,
    Info,
    Keyboard,
    Link,
    LoaderCircle,
    Maximize,
    Mic,
    Minimize,
    Minus,
    Pause,
    Pencil,
    Pin,
    Play,
    Plus,
    RefreshCw,
    Scissors,
    Search,
    ShieldCheck,
    Star,
    StepForward,
    Trash,
    TriangleAlert,
    X,
}

macro_rules! files {
    ($($name:literal),*) => {
        [$(($name, include_bytes!(concat!("../brand/icons/", $name, ".svg")).as_slice())),*]
    };
}

/// Each icon's file name and drawing, in the order of `Icon`.
static FILES: [(&str, &[u8]); 42] = files!(
    "arrow-down",
    "arrow-left",
    "arrow-right-left",
    "camera",
    "check",
    "chevron-down",
    "chevron-right",
    "circle-alert",
    "circle-check-big",
    "copy",
    "download",
    "ellipsis",
    "file-audio",
    "file-code",
    "file-output",
    "file-text",
    "file-type",
    "gauge",
    "history",
    "image-plus",
    "info",
    "keyboard",
    "link-2",
    "loader-circle",
    "maximize-2",
    "mic",
    "minimize-2",
    "minus",
    "pause",
    "pencil",
    "pin",
    "play",
    "plus",
    "refresh-cw",
    "scissors",
    "search",
    "shield-check",
    "star",
    "step-forward",
    "trash-2",
    "triangle-alert",
    "x"
);

const PREFIX: &str = "chidori/icons/";

/// The app's icons for GPUI, and GPUI Kit's own for its components.
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui_kit::Result<Option<Cow<'static, [u8]>>> {
        if let Some(name) = path.strip_prefix(PREFIX).and_then(|p| p.strip_suffix(".svg")) {
            return Ok(FILES.iter().find(|(n, _)| *n == name).map(|(_, b)| Cow::Borrowed(*b)));
        }
        gpui_kit::assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> gpui_kit::Result<Vec<SharedString>> {
        gpui_kit::assets::Assets.list(path)
    }
}

fn path_of(which: Icon) -> SharedString {
    format!("{PREFIX}{}.svg", FILES[which as usize].0).into()
}

/// An icon at a size, in one colour.
pub fn icon(which: Icon, size: f32, color: Color) -> Svg {
    svg().path(path_of(which)).flex_none().size(px(size)).text_color(color)
}

/// An icon turned by `turn` (0..1 of a full turn).
pub fn turned(which: Icon, size: f32, color: Color, turn: f32) -> Svg {
    icon(which, size, color).with_transformation(Transformation::rotate(Radians(turn * std::f32::consts::TAU)))
}
