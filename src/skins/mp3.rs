//! Pocket MP3 player with a click wheel. Drag around the wheel to change the volume.

use egui::{Align2, Color32, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};

use super::*;

const SCREEN_BG: Color32 = Color32::from_rgb(0x1B, 0x1B, 0x2F);

pub fn draw(ui: &mut Ui, np: &NowPlaying) -> Vec<Action> {
    let mut actions = Vec::new();
    let avail = ui.available_rect_before_wrap().shrink(12.0);
    let body = fit(avail, 0.6);
    ui.allocate_rect(body, Sense::hover());
    let p = ui.painter_at(body.expand(8.0));
    let u = body.width() / 60.0;

    p.rect_filled(body.translate(Vec2::splat(4.0)), 7.0 * u, Color32::BLACK);
    p.rect_filled(body, 7.0 * u, Color32::from_rgb(0x52, 0x40, 0x92));
    p.rect_filled(body.shrink(1.5 * u), 6.0 * u, Color32::from_rgb(0x3C, 0x2E, 0x7B));
    // top highlight
    p.rect_filled(Rect::from_min_size(body.min + Vec2::new(3.0 * u, 1.5 * u), Vec2::new(body.width() - 6.0 * u, 1.0 * u)), u, white_a(25));
    p.rect_stroke(body, 7.0 * u, Stroke::new(2.0f32, YELLOW), StrokeKind::Inside);

    // ---- screen
    let bezel = Rect::from_min_size(body.min + Vec2::new(4.5 * u, 5.0 * u), Vec2::new(51.0 * u, 42.0 * u));
    p.rect_filled(bezel, 2.0 * u, Color32::BLACK);
    let scr = bezel.shrink(1.5 * u);
    p.rect_filled(scr, u, SCREEN_BG);

    let header = Rect::from_min_size(scr.min, Vec2::new(scr.width(), 5.0 * u));
    p.rect_filled(header, 0.0, PURPLE);
    p.line_segment([header.left_bottom(), header.right_bottom()], Stroke::new(1.0f32, YELLOW));
    let hfont = pixel((3.4 * u).max(10.0));
    p.text(header.center(), Align2::CENTER_CENTER, "Now Playing", hfont.clone(), YELLOW);
    paint_icon(&p, Rect::from_min_size(header.min + Vec2::new(u, 0.8 * u), Vec2::splat(3.4 * u)), if np.playing { Icon::Play } else { Icon::Pause }, TEAL);
    // battery
    let bat = Rect::from_min_size(Pos2::new(header.right() - 7.0 * u, header.top() + 1.4 * u), Vec2::new(5.0 * u, 2.4 * u));
    p.rect_stroke(bat, 0.5, Stroke::new(1.0f32, YELLOW), StrokeKind::Inside);
    p.rect_filled(Rect::from_min_size(bat.min + Vec2::splat(0.6 * u), Vec2::new(3.2 * u, 1.2 * u)), 0.0, TEAL);
    p.rect_filled(Rect::from_min_size(bat.right_top() + Vec2::new(0.0, 0.7 * u), Vec2::new(0.6 * u, u)), 0.0, YELLOW);

    let content = Rect::from_min_max(header.left_bottom() + Vec2::new(2.0 * u, 2.0 * u), scr.right_bottom() - Vec2::new(2.0 * u, 2.0 * u));
    let art = Rect::from_min_size(content.min, Vec2::splat(18.0 * u));
    p.rect_filled(art.translate(Vec2::splat(u * 0.6)), 0.0, Color32::BLACK);
    cover_or_placeholder(&p, art, np.cover);
    p.rect_stroke(art, 0.0, Stroke::new(1.0f32, YELLOW), StrokeKind::Outside);

    let tx = art.right() + 2.0 * u;
    let tw = content.right() - tx;
    let row = |i: f32| Rect::from_min_size(Pos2::new(tx, content.top() + i * 5.0 * u), Vec2::new(tw, 4.6 * u));
    marquee(&p, row(0.0), &np.title(), bold((4.0 * u).max(12.0)), YELLOW, np.time);
    marquee(&p, row(1.0), &np.artist(), pixel((3.6 * u).max(11.0)), TEAL, np.time);
    marquee(&p, row(2.0), &np.meta.map(|m| m.album.clone()).unwrap_or_default(), pixel((3.4 * u).max(10.0)), DIM, np.time);
    if let (Some(i), n) = (np.index, np.count) {
        p.text(row(3.0).left_center(), Align2::LEFT_CENTER, format!("{} of {}", i + 1, n), pixel((3.2 * u).max(10.0)), DIM);
    }

    let viz = Rect::from_min_max(Pos2::new(content.left(), art.bottom() + 1.5 * u), Pos2::new(content.right(), content.bottom() - 6.5 * u));
    let vol_until: f64 = ui.ctx().data(|d| d.get_temp(ui.id().with("mp3-vol-until")).unwrap_or(0.0));
    if np.time < vol_until {
        let bar = Rect::from_center_size(viz.center(), Vec2::new(viz.width() * 0.8, 2.0 * u));
        p.text(bar.center_top() - Vec2::new(0.0, 1.5 * u), Align2::CENTER_BOTTOM, format!("Volume {}%", (np.volume * 100.0).round()), hfont.clone(), YELLOW);
        p.rect_filled(bar, u, INDIGO);
        p.rect_filled(Rect::from_min_size(bar.min, Vec2::new(bar.width() * np.volume, bar.height())), u, TEAL);
    } else {
        spectrum_bars(&p, viz, np.bars, np.peaks, 24, INDIGO, TEAL, MAGENTA);
    }

    let prog = Rect::from_min_size(Pos2::new(content.left(), content.bottom() - 5.0 * u), Vec2::new(content.width(), 1.6 * u));
    if let Some(f) = seek_rail(ui, prog, np.progress(), INDIGO, YELLOW, YELLOW) {
        actions.push(Action::Seek(f as f64 * np.dur));
    }
    let tf = pixel((3.2 * u).max(10.0));
    p.text(prog.left_bottom() + Vec2::new(0.0, 0.8 * u), Align2::LEFT_TOP, fmt_time(np.pos), tf.clone(), TEAL);
    p.text(prog.right_bottom() + Vec2::new(0.0, 0.8 * u), Align2::RIGHT_TOP, format!("-{}", fmt_time((np.dur - np.pos).max(0.0))), tf, TEAL);

    // ---- click wheel
    let wc = Pos2::new(body.center().x, body.top() + 73.0 * u);
    let wr = 20.0 * u;
    p.circle_filled(wc + Vec2::splat(1.5), wr, Color32::from_black_alpha(120));
    p.circle_filled(wc, wr, Color32::from_rgb(0x2B, 0x1B, 0x4F));
    p.circle_stroke(wc, wr, Stroke::new(1.5f32, YELLOW));
    let inner = 7.5 * u;

    let wheel = ui.interact(Rect::from_center_size(wc, Vec2::splat(wr * 2.0)), ui.id().with("mp3-wheel"), Sense::click_and_drag());
    let angle_key = ui.id().with("mp3-wheel-angle");
    if let Some(pos) = wheel.interact_pointer_pos() {
        let d = pos - wc;
        if wheel.dragged() && d.length() > inner {
            let a = d.angle();
            if let Some(prev) = ui.ctx().data(|m| m.get_temp::<f32>(angle_key)) {
                let mut da = a - prev;
                if da > std::f32::consts::PI {
                    da -= std::f32::consts::TAU;
                } else if da < -std::f32::consts::PI {
                    da += std::f32::consts::TAU;
                }
                // one full turn ≈ 0 → 100 %
                actions.push(Action::Volume((np.volume + da / std::f32::consts::TAU).clamp(0.0, 1.0)));
                ui.ctx().data_mut(|m| m.insert_temp(ui.id().with("mp3-vol-until"), np.time + 1.2));
            }
            ui.ctx().data_mut(|m| m.insert_temp(angle_key, a));
        }
    }
    if wheel.drag_stopped() || !wheel.dragged() {
        ui.ctx().data_mut(|m| m.remove::<f32>(angle_key));
    }
    if wheel.clicked() {
        if let Some(pos) = wheel.interact_pointer_pos() {
            let d = pos - wc;
            if d.length() <= inner {
                actions.push(Action::Toggle);
            } else if d.length() <= wr {
                // quadrant buttons
                if d.y.abs() > d.x.abs() {
                    actions.push(if d.y < 0.0 { Action::Open } else { Action::Toggle });
                } else {
                    actions.push(if d.x < 0.0 { Action::Prev } else { Action::Next });
                }
            }
        }
    }

    let lf = bold((3.6 * u).max(11.0));
    p.text(wc - Vec2::new(0.0, wr * 0.74), Align2::CENTER_CENTER, "MENU", lf, YELLOW);
    let s = Vec2::splat(4.5 * u);
    paint_icon(&p, Rect::from_center_size(wc - Vec2::new(wr * 0.74, 0.0), s), Icon::Prev, YELLOW);
    paint_icon(&p, Rect::from_center_size(wc + Vec2::new(wr * 0.74, 0.0), s), Icon::Next, YELLOW);
    let pp = Rect::from_center_size(wc + Vec2::new(0.0, wr * 0.74), s);
    paint_icon(&p, pp.translate(Vec2::new(-1.3 * u, 0.0)), Icon::Play, YELLOW);
    paint_icon(&p, pp.translate(Vec2::new(1.6 * u, 0.0)), Icon::Pause, YELLOW);

    let pressed = wheel.is_pointer_button_down_on()
        && wheel.interact_pointer_pos().is_some_and(|pos| (pos - wc).length() <= inner);
    p.circle_filled(wc, inner, if pressed { TEAL } else { Color32::from_rgb(0x4a, 0x3c, 0x86) });
    p.circle_stroke(wc, inner, Stroke::new(1.5f32, YELLOW));

    // hold switch + headphone jack on the top edge
    let hold = Rect::from_min_size(body.min + Vec2::new(6.0 * u, -1.0 * u), Vec2::new(6.0 * u, 1.6 * u));
    p.rect_filled(hold, 0.5, Color32::from_gray(30));
    p.rect_filled(Rect::from_min_size(hold.min, Vec2::new(2.5 * u, 1.6 * u)), 0.5, MAGENTA);
    p.circle_filled(Pos2::new(body.right() - 9.0 * u, body.top() + 0.2 * u), 1.2 * u, Color32::BLACK);

    actions
}
