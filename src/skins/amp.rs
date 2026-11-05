//! Late-90s desktop amp: LCD with seven-segment time, spectrum/oscilloscope (click it to
//! switch), scrolling title, kbps/kHz readout, and chunky transport buttons. Laid out on a
//! 275×116 grid and scaled up, re-coloured in the site palette.

use egui::{Align2, Color32, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};

use super::*;

pub const BASE: Vec2 = Vec2::new(275.0, 116.0);
const LCD_ON: Color32 = TEAL;
const LCD_OFF: Color32 = Color32::from_rgb(0x15, 0x22, 0x2a);

/// Draws the main amp window inside `rect` (aspect 275:116, top-left aligned).
pub fn draw(ui: &mut Ui, rect: Rect, np: &NowPlaying) -> Vec<Action> {
    let mut actions = Vec::new();
    let s = rect.width() / BASE.x;
    let at = |x: f32, y: f32, w: f32, h: f32| Rect::from_min_size(rect.min + Vec2::new(x, y) * s, Vec2::new(w, h) * s);
    let p = ui.painter_at(rect.expand(6.0));
    ui.allocate_rect(rect, Sense::hover());

    // chrome
    p.rect_filled(rect.translate(Vec2::splat(4.0)), 0.0, Color32::BLACK);
    p.rect_filled(rect, 0.0, Color32::from_rgb(0x34, 0x26, 0x66));
    for i in 0..(BASE.y as i32 / 2) {
        let y = rect.top() + i as f32 * 2.0 * s;
        p.line_segment([Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)], Stroke::new(s * 0.5, white_a(4)));
    }
    p.rect_stroke(rect, 0.0, Stroke::new(2.0f32, YELLOW), StrokeKind::Inside);

    // title bar with grooves
    let bar = at(2.0, 2.0, 271.0, 12.0);
    p.rect_filled(bar, 0.0, PURPLE);
    for i in 0..4 {
        let y = bar.top() + (3.0 + i as f32 * 2.0) * s;
        p.line_segment([Pos2::new(bar.left() + 8.0 * s, y), Pos2::new(bar.right() - 30.0 * s, y)], Stroke::new(s * 0.6, YELLOW.gamma_multiply(0.35)));
    }
    let label = "J A M P";
    let tr = Rect::from_center_size(bar.center(), Vec2::new(52.0 * s, bar.height()));
    p.rect_filled(tr, 0.0, PURPLE);
    p.text(tr.center(), Align2::CENTER_CENTER, label, bold(8.5 * s), YELLOW);
    for i in 0..3 {
        let b = at(245.0 + i as f32 * 9.0, 4.0, 8.0, 8.0);
        p.rect_filled(b, 0.0, INK);
        p.rect_stroke(b, 0.0, Stroke::new(1.0f32, YELLOW), StrokeKind::Inside);
    }

    // LCD
    let lcd = at(9.0, 20.0, 96.0, 42.0);
    p.rect_filled(lcd, 0.0, Color32::BLACK);
    p.rect_stroke(lcd, 0.0, Stroke::new(1.0f32, INDIGO), StrokeKind::Outside);
    let state_icon = if !np.loaded { Icon::Stop } else if np.playing { Icon::Play } else { Icon::Pause };
    paint_icon(&p, at(13.0, 25.0, 9.0, 9.0), state_icon, if np.playing { MAGENTA } else { LCD_ON });

    let t = np.pos as u64;
    let (m, sec) = ((t / 60).min(99), t % 60);
    let digits = [m / 10, m % 10, sec / 10, sec % 10];
    for (i, d) in digits.iter().enumerate() {
        let x = 36.0 + i as f32 * 12.0 + if i >= 2 { 6.0 } else { 0.0 };
        seven_seg(&p, at(x, 25.0, 9.0, 14.0), *d as u8, LCD_ON, LCD_OFF, s);
    }
    for dy in [29.0, 35.0] {
        p.rect_filled(at(61.5, dy, 1.6, 1.6), 0.0, if np.playing && (np.time * 2.0) as i64 % 2 == 0 { LCD_OFF } else { LCD_ON });
    }

    let viz = at(14.0, 43.0, 87.0, 16.0);
    let osc_key = ui.id().with("amp-osc");
    let osc: bool = ui.ctx().data(|d| d.get_temp(osc_key).unwrap_or(false));
    if hotspot(ui, viz, "amp-viz").clicked() {
        ui.ctx().data_mut(|d| d.insert_temp(osc_key, !osc));
    }
    // dotted grid like an LCD
    for gx in 0..22 {
        for gy in 0..4 {
            p.rect_filled(at(14.0 + gx as f32 * 4.0, 44.0 + gy as f32 * 4.0, 0.6, 0.6), 0.0, LCD_OFF);
        }
    }
    if osc {
        oscilloscope(&p, viz, np.wave, LCD_ON);
    } else {
        spectrum_bars(&p, viz, np.bars, np.peaks, 19, Color32::from_rgb(0x2b, 0x9a, 0xb0), YELLOW, MAGENTA);
    }

    // marquee
    let mq = at(111.0, 22.0, 156.0, 13.0);
    p.rect_filled(mq, 0.0, Color32::BLACK);
    let text = match np.meta {
        Some(m) => format!("{}. {} ({})", np.index.map(|i| i + 1).unwrap_or(0), m.display_title(), fmt_time(np.dur)),
        None => "JAMP · drop some music in · ".into(),
    };
    marquee(&p, mq.shrink2(Vec2::new(3.0 * s, 0.0)), &text, pixel(9.0 * s), LCD_ON, np.time);

    // kbps / kHz / mono / stereo
    let small = pixel((6.5 * s).max(9.0));
    let kbps = np.meta.and_then(|m| m.bitrate_kbps).map(|b| b.to_string()).unwrap_or_default();
    let khz = if np.info.sample_rate > 0 {
        (np.info.sample_rate / 1000).to_string()
    } else {
        np.meta.and_then(|m| m.sample_rate).map(|r| (r / 1000).to_string()).unwrap_or_default()
    };
    for (x, w, val, unit) in [(111.0, 20.0, kbps.as_str(), "kbps"), (156.0, 14.0, khz.as_str(), "kHz")] {
        let b = at(x, 40.0, w, 10.0);
        p.rect_filled(b, 0.0, Color32::BLACK);
        p.text(b.right_center() - Vec2::new(1.5 * s, 0.0), Align2::RIGHT_CENTER, val, pixel(8.0 * s), LCD_ON);
        p.text(b.right_center() + Vec2::new(2.0 * s, 0.0), Align2::LEFT_CENTER, unit, small.clone(), YELLOW);
    }
    let stereo = np.info.channels >= 2 || np.meta.and_then(|m| m.channels).unwrap_or(0) >= 2;
    let lit = |on: bool| if on && np.loaded { LCD_ON } else { LCD_OFF };
    p.text(at(212.0, 40.0, 1.0, 10.0).left_center(), Align2::LEFT_CENTER, "mono", small.clone(), lit(!stereo));
    p.text(at(236.0, 40.0, 1.0, 10.0).left_center(), Align2::LEFT_CENTER, "stereo", small.clone(), lit(stereo));

    // volume + balance
    let vol = at(108.0, 58.0, 66.0, 6.0);
    if let Some(f) = seek_rail(ui, vol, np.volume, Color32::BLACK, lerp_color(Color32::from_rgb(0x2b, 0x9a, 0xb0), MAGENTA, np.volume), YELLOW) {
        actions.push(Action::Volume(f));
    }
    let bal = at(178.0, 58.0, 36.0, 6.0);
    p.rect_filled(bal, 3.0 * s, Color32::BLACK);
    p.circle_filled(bal.center(), 3.5 * s, YELLOW);
    for (x, lbl) in [(219.0, "EQ"), (243.0, "PL")] {
        let b = at(x, 57.0, 22.0, 9.0);
        p.rect_filled(b, 0.0, INK);
        p.rect_stroke(b, 0.0, Stroke::new(1.0f32, YELLOW), StrokeKind::Inside);
        p.text(b.center(), Align2::CENTER_CENTER, lbl, small.clone(), if lbl == "PL" { LCD_ON } else { DIM });
    }

    // seek
    let seek = at(16.0, 72.0, 248.0, 8.0);
    p.rect_filled(seek.expand(1.0), 0.0, Color32::BLACK);
    if np.loaded {
        if let Some(f) = seek_rail(ui, seek.shrink(2.0 * s), np.progress(), INK, INDIGO, YELLOW) {
            actions.push(Action::Seek(f as f64 * np.dur));
        }
    }

    // transport
    let icons = [(Icon::Prev, Action::Prev), (Icon::Play, Action::Play), (Icon::Pause, Action::Pause), (Icon::Stop, Action::Stop), (Icon::Next, Action::Next)];
    for (i, (icon, act)) in icons.into_iter().enumerate() {
        let b = at(16.0 + i as f32 * 23.0, 88.0, 22.0, 18.0);
        if chunky(ui, &p, b, icon, &format!("amp-{i}"), s) {
            actions.push(act);
        }
    }
    if chunky(ui, &p, at(136.0, 89.0, 22.0, 16.0), Icon::Eject, "amp-eject", s) {
        actions.push(Action::Open);
    }
    for (x, w, label, on, act) in [
        (164.0, 44.0, "SHUFFLE", np.shuffle, Action::Shuffle),
        (210.0, 28.0, match np.repeat { Repeat::One => "REP 1", _ => "REP" }, np.repeat != Repeat::Off, Action::Repeat),
    ] {
        let b = at(x, 90.0, w, 14.0);
        let r = hotspot(ui, b, label);
        p.rect_filled(b, 0.0, if r.hovered() { INDIGO } else { INK });
        p.rect_stroke(b, 0.0, Stroke::new(1.0f32, YELLOW), StrokeKind::Inside);
        p.circle_filled(b.left_center() + Vec2::new(4.0 * s, 0.0), 1.8 * s, if on { MAGENTA } else { LCD_OFF });
        p.text(b.center() + Vec2::new(3.0 * s, 0.0), Align2::CENTER_CENTER, label, small.clone(), YELLOW);
        if r.clicked() {
            actions.push(act);
        }
    }
    paint_note(&p, at(258.0, 97.0, 0.0, 0.0).min, 14.0 * s, YELLOW);
    actions
}

fn chunky(ui: &mut Ui, p: &egui::Painter, b: Rect, icon: Icon, id: &str, s: f32) -> bool {
    let r = hotspot(ui, b, id);
    let down = r.is_pointer_button_down_on();
    p.rect_filled(b, 0.0, Color32::from_rgb(0x52, 0x40, 0x92));
    // bevel
    let (hi, lo) = if down { (Color32::BLACK, Color32::from_gray(170)) } else { (Color32::from_gray(170), Color32::BLACK) };
    p.line_segment([b.left_bottom(), b.left_top()], Stroke::new(s, hi));
    p.line_segment([b.left_top(), b.right_top()], Stroke::new(s, hi));
    p.line_segment([b.right_top(), b.right_bottom()], Stroke::new(s, lo));
    p.line_segment([b.right_bottom(), b.left_bottom()], Stroke::new(s, lo));
    let ir = b.shrink(4.0 * s).translate(if down { Vec2::splat(s * 0.8) } else { Vec2::ZERO });
    paint_icon(p, ir, icon, if r.hovered() { TEAL } else { YELLOW });
    r.clicked()
}

/// Classic seven-segment digit.
fn seven_seg(p: &egui::Painter, r: Rect, d: u8, on: Color32, off: Color32, s: f32) {
    const SEGS: [u8; 10] = [0x3F, 0x06, 0x5B, 0x4F, 0x66, 0x6D, 0x7D, 0x07, 0x7F, 0x6F];
    let mask = SEGS[(d % 10) as usize];
    let t = 1.6 * s;
    let (l, rgt, top, mid, bot) = (r.left(), r.right(), r.top(), r.center().y, r.bottom());
    let segs = [
        Rect::from_min_max(Pos2::new(l + t, top), Pos2::new(rgt - t, top + t)),           // a
        Rect::from_min_max(Pos2::new(rgt - t, top + t), Pos2::new(rgt, mid - t * 0.5)),   // b
        Rect::from_min_max(Pos2::new(rgt - t, mid + t * 0.5), Pos2::new(rgt, bot - t)),   // c
        Rect::from_min_max(Pos2::new(l + t, bot - t), Pos2::new(rgt - t, bot)),           // d
        Rect::from_min_max(Pos2::new(l, mid + t * 0.5), Pos2::new(l + t, bot - t)),       // e
        Rect::from_min_max(Pos2::new(l, top + t), Pos2::new(l + t, mid - t * 0.5)),       // f
        Rect::from_min_max(Pos2::new(l + t, mid - t * 0.5), Pos2::new(rgt - t, mid + t * 0.5)), // g
    ];
    for (i, seg) in segs.iter().enumerate() {
        let lit = mask & (1 << i) != 0;
        if lit {
            glow_fill(p, *seg, on, 1.6 * s);
        }
        p.rect_filled(*seg, 0.0, if lit { on } else { off });
    }
}
