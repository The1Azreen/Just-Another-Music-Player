//! Player skins. Each one paints its own device and reports what the user pressed as `Action`s.

pub mod amp;
pub mod cassette;
pub mod cd;
pub mod modern;
pub mod mp3;
pub mod vinyl;

use egui::{Align2, Color32, Pos2, Rect, Response, Sense, Stroke, StrokeKind, TextureHandle, Ui, Vec2};

use crate::audio::StreamInfo;
use crate::lyrics::Lyrics;
use crate::meta::TrackMeta;
use crate::theme::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
pub enum Skin {
    #[default]
    Modern,
    Amp,
    Cd,
    Mp3,
    Vinyl,
    Cassette,
}

impl Skin {
    pub const ALL: [Skin; 6] = [Skin::Modern, Skin::Amp, Skin::Cd, Skin::Mp3, Skin::Vinyl, Skin::Cassette];

    pub fn label(self) -> &'static str {
        match self {
            Skin::Modern => "Modern",
            Skin::Amp => "Amp",
            Skin::Cd => "CD",
            Skin::Mp3 => "MP3",
            Skin::Vinyl => "Vinyl",
            Skin::Cassette => "Tape",
        }
    }

    pub fn parse(s: &str) -> Option<Skin> {
        let s = s.to_ascii_lowercase();
        Skin::ALL.into_iter().find(|k| {
            k.label().to_ascii_lowercase() == s
                || format!("{k:?}").to_ascii_lowercase() == s
                || (s == "winamp" && *k == Skin::Amp)
                || (s == "ipod" && *k == Skin::Mp3)
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
pub enum Repeat {
    #[default]
    Off,
    All,
    One,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Toggle,
    Play,
    Pause,
    Stop,
    Prev,
    Next,
    Seek(f64),
    Volume(f32),
    Open,
    Shuffle,
    Repeat,
    PlayIndex(usize),
}

/// Physical state of the moving parts, integrated each frame by the app.
#[derive(Default)]
pub struct Anim {
    /// Turntable platter angle (rad) and speed (0..1 of 33⅓ rpm, with inertia).
    pub platter: f32,
    pub platter_speed: f32,
    /// Tonearm sweep 0 = rest, then 0.12..1 lead-in to run-out; `arm_lift` 0 = on record.
    pub arm: f32,
    pub arm_lift: f32,
    pub cd: f32,
    pub cd_speed: f32,
    /// CD tray/lid opening 0..1 (open when nothing is loaded).
    pub cd_lid: f32,
    pub reel_l: f32,
    pub reel_r: f32,
    pub tape_speed: f32,
}

pub struct NowPlaying<'a> {
    pub meta: Option<&'a TrackMeta>,
    pub index: Option<usize>,
    pub count: usize,
    pub pos: f64,
    pub dur: f64,
    pub loaded: bool,
    pub playing: bool,
    pub volume: f32,
    pub shuffle: bool,
    pub repeat: Repeat,
    pub cover: Option<&'a TextureHandle>,
    pub lyrics: Option<&'a Lyrics>,
    pub bars: &'a [f32],
    pub peaks: &'a [f32],
    pub wave: &'a [f32],
    pub info: StreamInfo,
    pub time: f64,
    pub anim: &'a Anim,
}

impl NowPlaying<'_> {
    pub fn progress(&self) -> f32 {
        if self.dur > 0.0 {
            (self.pos / self.dur).clamp(0.0, 1.0) as f32
        } else {
            0.0
        }
    }

    pub fn title(&self) -> String {
        self.meta.map(|m| m.title.clone()).unwrap_or_else(|| "No track loaded".into())
    }

    pub fn artist(&self) -> String {
        self.meta.map(|m| m.artist.clone()).unwrap_or_default()
    }

    pub fn current_lyric(&self) -> Option<&str> {
        let l = self.lyrics?;
        let i = l.current_line(self.pos)?;
        Some(l.lines[i].1.as_str()).filter(|s| !s.is_empty())
    }
}

// ---------------------------------------------------------------------------------------------
// Shared drawing

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Play,
    Pause,
    Stop,
    Prev,
    Next,
    Eject,
    Shuffle,
    Repeat,
    RepeatOne,
    Rewind,
    FastForward,
}

pub fn paint_icon(p: &egui::Painter, r: Rect, icon: Icon, c: Color32) {
    let s = r.height().min(r.width()) * 0.5;
    let ctr = r.center();
    let tri = |p: &egui::Painter, cx: f32, dir: f32, w: f32| {
        p.add(egui::Shape::convex_polygon(
            vec![
                Pos2::new(cx - w * 0.5 * dir, ctr.y - s * 0.55),
                Pos2::new(cx + w * 0.5 * dir, ctr.y),
                Pos2::new(cx - w * 0.5 * dir, ctr.y + s * 0.55),
            ],
            c,
            Stroke::NONE,
        ));
    };
    match icon {
        Icon::Play => tri(p, ctr.x + s * 0.08, 1.0, s * 1.0),
        Icon::Pause => {
            for dx in [-0.28, 0.28] {
                p.rect_filled(Rect::from_center_size(ctr + Vec2::new(dx * s, 0.0), Vec2::new(s * 0.3, s * 1.1)), 0.0, c);
            }
        }
        Icon::Stop => {
            p.rect_filled(Rect::from_center_size(ctr, Vec2::splat(s * 0.95)), 0.0, c);
        }
        Icon::Prev | Icon::Next => {
            let d = if icon == Icon::Next { 1.0 } else { -1.0 };
            tri(p, ctr.x - s * 0.25 * d, d, s * 0.6);
            tri(p, ctr.x + s * 0.25 * d, d, s * 0.6);
            p.rect_filled(Rect::from_center_size(ctr + Vec2::new(s * 0.62 * d, 0.0), Vec2::new(s * 0.14, s * 1.1)), 0.0, c);
        }
        Icon::Rewind | Icon::FastForward => {
            let d = if icon == Icon::FastForward { 1.0 } else { -1.0 };
            tri(p, ctr.x - s * 0.3 * d, d, s * 0.6);
            tri(p, ctr.x + s * 0.3 * d, d, s * 0.6);
        }
        Icon::Eject => {
            p.add(egui::Shape::convex_polygon(
                vec![
                    Pos2::new(ctr.x, ctr.y - s * 0.6),
                    Pos2::new(ctr.x + s * 0.6, ctr.y + s * 0.1),
                    Pos2::new(ctr.x - s * 0.6, ctr.y + s * 0.1),
                ],
                c,
                Stroke::NONE,
            ));
            p.rect_filled(Rect::from_center_size(ctr + Vec2::new(0.0, s * 0.42), Vec2::new(s * 1.2, s * 0.18)), 0.0, c);
        }
        Icon::Shuffle => {
            let st = Stroke::new(s * 0.14, c);
            let (l, rr) = (ctr.x - s * 0.7, ctr.x + s * 0.55);
            p.line_segment([Pos2::new(l, ctr.y - s * 0.4), Pos2::new(rr, ctr.y + s * 0.4)], st);
            p.line_segment([Pos2::new(l, ctr.y + s * 0.4), Pos2::new(rr, ctr.y - s * 0.4)], st);
            for y in [-0.4, 0.4] {
                p.circle_filled(Pos2::new(rr + s * 0.05, ctr.y + y * s), s * 0.17, c);
            }
        }
        Icon::Repeat | Icon::RepeatOne => {
            let st = Stroke::new(s * 0.14, c);
            let rr = Rect::from_center_size(ctr, Vec2::new(s * 1.4, s * 0.9));
            p.rect_stroke(rr, s * 0.3, st, StrokeKind::Middle);
            p.add(egui::Shape::convex_polygon(
                vec![
                    Pos2::new(rr.center().x + s * 0.1, rr.top() - s * 0.25),
                    Pos2::new(rr.center().x + s * 0.4, rr.top()),
                    Pos2::new(rr.center().x + s * 0.1, rr.top() + s * 0.25),
                ],
                c,
                Stroke::NONE,
            ));
            if icon == Icon::RepeatOne {
                p.text(ctr, Align2::CENTER_CENTER, "1", bold(s * 0.9), c);
            }
        }
    }
}

/// A square pixel-art button carrying a transport icon.
pub fn icon_button(ui: &mut Ui, size: Vec2, icon: Icon, lit: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let p = ui.painter();
    let (bg, fg, border) = if resp.is_pointer_button_down_on() {
        (TEAL, INK, YELLOW)
    } else if resp.hovered() {
        (INDIGO, TEAL, TEAL)
    } else if lit {
        (INDIGO, MAGENTA, YELLOW)
    } else {
        (INK, YELLOW, YELLOW)
    };
    p.rect_filled(rect, 2.0, bg);
    p.rect_stroke(rect, 2.0, Stroke::new(1.5f32, border), StrokeKind::Inside);
    paint_icon(p, rect.shrink(4.0), icon, fg);
    resp
}

/// Hot spot on top of something already painted. Returns the click response.
pub fn hotspot(ui: &mut Ui, rect: Rect, id: &str) -> Response {
    ui.interact(rect, ui.id().with(id), Sense::click())
}

/// A horizontal progress/seek rail. Returns the fraction clicked or dragged to.
pub fn seek_rail(ui: &mut Ui, rect: Rect, frac: f32, track: Color32, fill: Color32, knob: Color32) -> Option<f32> {
    let resp = ui.interact(rect.expand2(Vec2::new(0.0, 4.0)), ui.id().with(("rail", rect.min.x as i32, rect.min.y as i32)), Sense::click_and_drag());
    let p = ui.painter();
    p.rect_filled(rect, rect.height() * 0.5, track);
    let mut f = frac;
    if let Some(pos) = resp.interact_pointer_pos() {
        f = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
    }
    let filled = Rect::from_min_max(rect.min, Pos2::new(rect.left() + rect.width() * f, rect.bottom()));
    p.rect_filled(filled, rect.height() * 0.5, fill);
    p.circle_filled(Pos2::new(filled.right(), rect.center().y), rect.height() * 0.9 + 1.0, knob);
    if resp.drag_stopped() || resp.clicked() {
        return Some(f);
    }
    None
}

/// Scrolling text clipped to `rect`.
pub fn marquee(p: &egui::Painter, rect: Rect, text: &str, font: egui::FontId, color: Color32, time: f64) {
    let galley = p.layout_no_wrap(text.to_owned(), font, color);
    let w = galley.size().x;
    let clip = p.with_clip_rect(rect.intersect(p.clip_rect()));
    let y = rect.center().y - galley.size().y * 0.5;
    if w <= rect.width() {
        clip.galley(Pos2::new(rect.left(), y), galley, color);
        return;
    }
    let gap = 48.0;
    let off = ((time * 40.0) as f32) % (w + gap);
    clip.galley(Pos2::new(rect.left() - off, y), galley.clone(), color);
    clip.galley(Pos2::new(rect.left() - off + w + gap, y), galley, color);
}

/// Bars with falling peak caps. `bars`/`peaks` are resampled to however many fit.
pub fn spectrum_bars(p: &egui::Painter, rect: Rect, bars: &[f32], peaks: &[f32], n: usize, low: Color32, high: Color32, peak: Color32) {
    if bars.is_empty() || n == 0 {
        return;
    }
    let gap = 1.0;
    let bw = (rect.width() - gap * (n as f32 - 1.0)) / n as f32;
    for i in 0..n {
        let src = i * bars.len() / n;
        let v = bars[src];
        let x = rect.left() + i as f32 * (bw + gap);
        let h = v * rect.height();
        // stacked segments give the classic LED-meter look
        let seg = 3.0;
        let mut y = rect.bottom();
        while y > rect.bottom() - h {
            let t = (rect.bottom() - y) / rect.height();
            let top = (y - seg + 1.0).max(rect.bottom() - h);
            p.rect_filled(Rect::from_min_max(Pos2::new(x, top), Pos2::new(x + bw, y)), 0.0, lerp_color(low, high, t));
            y -= seg;
        }
        if let Some(pk) = peaks.get(src) {
            let py = rect.bottom() - pk * rect.height();
            p.rect_filled(Rect::from_min_max(Pos2::new(x, py - 1.5), Pos2::new(x + bw, py)), 0.0, peak);
        }
    }
}

pub fn oscilloscope(p: &egui::Painter, rect: Rect, wave: &[f32], color: Color32) {
    if wave.len() < 2 {
        p.line_segment([rect.left_center(), rect.right_center()], Stroke::new(1.0f32, color));
        return;
    }
    let pts: Vec<Pos2> = wave
        .iter()
        .enumerate()
        .map(|(i, s)| {
            Pos2::new(
                rect.left() + i as f32 / (wave.len() - 1) as f32 * rect.width(),
                rect.center().y - s.clamp(-1.0, 1.0) * rect.height() * 0.48,
            )
        })
        .collect();
    p.add(egui::Shape::line(pts, Stroke::new(1.2f32, color)));
}

/// Screws, the little detail that makes a drawn device feel physical.
pub fn screw(p: &egui::Painter, c: Pos2, r: f32, angle: f32) {
    p.circle_filled(c, r, Color32::from_gray(150));
    p.circle_stroke(c, r, Stroke::new(1.0f32, Color32::from_gray(60)));
    let d = Vec2::angled(angle) * r * 0.7;
    p.line_segment([c - d, c + d], Stroke::new(1.2f32, Color32::from_gray(60)));
}

/// Placeholder art when no cover exists: the site's palette in a pixel pattern.
pub fn no_cover(p: &egui::Painter, rect: Rect) {
    p.rect_filled(rect, 0.0, PURPLE);
    let n = 8;
    let cell = rect.width() / n as f32;
    for y in 0..n {
        for x in 0..n {
            if (x * 7 + y * 3) % 5 == 0 {
                let r = Rect::from_min_size(rect.min + Vec2::new(x as f32 * cell, y as f32 * cell), Vec2::splat(cell));
                p.rect_filled(r, 0.0, INDIGO);
            }
        }
    }
    paint_note(p, rect.center(), rect.height() * 0.4, YELLOW);
}

/// A beamed pair of eighth notes, drawn rather than typed since pixel fonts rarely have ♫.
pub fn paint_note(p: &egui::Painter, c: Pos2, size: f32, color: Color32) {
    let s = size / 10.0;
    let heads = [c + Vec2::new(-3.0 * s, 3.5 * s), c + Vec2::new(3.0 * s, 2.5 * s)];
    for h in heads {
        p.circle_filled(h, 1.8 * s, color);
        p.line_segment([h + Vec2::new(1.6 * s, 0.0), h + Vec2::new(1.6 * s, -7.0 * s)], Stroke::new(1.0 * s, color));
    }
    p.add(egui::Shape::convex_polygon(
        vec![
            heads[0] + Vec2::new(1.1 * s, -7.0 * s),
            heads[1] + Vec2::new(2.1 * s, -8.0 * s),
            heads[1] + Vec2::new(2.1 * s, -6.2 * s),
            heads[0] + Vec2::new(1.1 * s, -5.2 * s),
        ],
        color,
        Stroke::NONE,
    ));
}

/// Cover art or placeholder in a square rect.
pub fn cover_or_placeholder(p: &egui::Painter, rect: Rect, cover: Option<&TextureHandle>) {
    match cover {
        Some(t) => image_in(p, rect, t.id(), Color32::WHITE),
        None => no_cover(p, rect),
    }
}

/// Fits a square of aspect `aspect` (w/h) into `avail`, centred.
pub fn fit(avail: Rect, aspect: f32) -> Rect {
    let w = avail.width().min(avail.height() * aspect);
    Rect::from_center_size(avail.center(), Vec2::new(w, w / aspect))
}
