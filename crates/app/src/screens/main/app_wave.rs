//! An app's sound beside its name, while it plays: its last two seconds as a small 朱 line on a grey
//! baseline, the newest at the right end, from the app's own session meter (DECISIONS → UI, "Apps
//! show their sound"). It steps ten times a second with the track instead of gliding, and in
//! silence it is blank but keeps its place, so the name never shifts.

use gpui_kit::{IntoElement, Styled, div, px};

use super::live_line::{Geometry, build, live_line};
use crate::app::App;
use crate::model::line::Sample;
use crate::model::source::WAVE_STEPS;
use crate::paint::{LineCap, LineJoin, Path, Point, Stroke, canvas_el};
use crate::theme::p;
use crate::ui::El;

/// The wave of an app (its root PID), `width` × `height`; blank space while it is quiet.
pub fn wave(app: &App, pid: Option<u32>, width: f32, height: f32) -> El {
    let track = pid.filter(|p| *p != 0).and_then(|p| app.app_levels.tracks.get(&p));
    match track {
        Some(t) if !t.quiet(app.now) => {
            let heights = t.heights.clone();
            canvas_el(None, move |frame| {
                let (w, h) = (frame.width(), frame.height());
                let base = h - 1.5;
                frame.stroke(
                    &Path::line(Point::new(0.0, base), Point::new(w, base)),
                    Stroke::default().with_color(p().border).with_width(1.0),
                );
                let g = Geometry {
                    base,
                    up: h - 3.0,
                    down: 0.0,
                    step: w / (WAVE_STEPS as f32 - 1.0),
                    start_x: 0.0,
                    end_pad: 0.0,
                };
                let samples: Vec<Sample> = heights.iter().map(|o| Sample { o: *o, ..Default::default() }).collect();
                let l = live_line(&samples, 0.0, 0, w, g, false);
                let stroke = Stroke::default()
                    .with_color(p().accent)
                    .with_width(1.8)
                    .with_line_cap(LineCap::Round)
                    .with_line_join(LineJoin::Round);
                frame.stroke(&Path::new(|b| build(b, &l.o, |pt| pt)), stroke);
            })
            .flex_none()
            .w(px(width))
            .h(px(height))
            .into_any_element()
        }
        _ => div().flex_none().w(px(width)).h(px(height)).into_any_element(),
    }
}
