//! Turntable: platter spins up/down with inertia, the cover is the record label, and the tonearm
//! tracks inward as the song plays. Click anywhere on the vinyl to drop the needle there.

use egui::{Align2, Color32, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui, Vec2};

use super::*;

const WOOD_DARK: Color32 = Color32::from_rgb(0x20, 0x16, 0x3a);
const WOOD: Color32 = Color32::from_rgb(0x34, 0x24, 0x5c);
const VINYL: Color32 = Color32::from_rgb(0x0e, 0x0c, 0x14);

/// Groove radii as a fraction of the record radius.
const R_OUTER: f32 = 0.94;
const R_INNER: f32 = 0.42;

pub fn draw(ui: &mut Ui, np: &NowPlaying) -> Vec<Action> {
    let mut actions = Vec::new();
    let avail = ui.available_rect_before_wrap().shrink(12.0);
    let deck = fit(avail, 1.38);
    ui.allocate_rect(deck, Sense::hover());
    let p = ui.painter_at(deck.expand(8.0));
    let u = deck.height() / 100.0;

    // plinth
    p.rect_filled(deck.translate(Vec2::splat(4.0)), 6.0 * u, Color32::BLACK);
    p.rect_filled(deck, 6.0 * u, WOOD);
    for i in 0..12 {
        // faint wood grain
        let y = deck.top() + (i as f32 + 0.5) * deck.height() / 12.0;
        p.line_segment(
            [Pos2::new(deck.left() + 6.0, y), Pos2::new(deck.right() - 6.0, y + (i % 3) as f32 - 1.0)],
            Stroke::new(1.0f32, WOOD_DARK.gamma_multiply(0.5)),
        );
    }
    p.rect_stroke(deck, 6.0 * u, Stroke::new(2.0f32, YELLOW), StrokeKind::Inside);

    let c = Pos2::new(deck.left() + deck.height() * 0.52, deck.center().y);
    let r = deck.height() * 0.44;

    // platter with strobe dots (they visibly crawl as it spins)
    p.circle_filled(c + Vec2::splat(3.0), r * 1.04, Color32::BLACK);
    p.circle_filled(c, r * 1.04, Color32::from_gray(90));
    let dots = 60;
    for i in 0..dots {
        let a = np.anim.platter + i as f32 / dots as f32 * std::f32::consts::TAU;
        p.circle_filled(c + Vec2::angled(a) * r * 1.015, u * 0.45, Color32::from_gray(if i % 2 == 0 { 200 } else { 120 }));
    }

    // record
    if np.loaded {
        p.circle_filled(c, r, VINYL);
        for g in 0..26 {
            let gr = r * (R_INNER + 0.02 + (R_OUTER - R_INNER) * g as f32 / 25.0);
            let shade = if g % 5 == 0 { 34 } else { 24 };
            p.circle_stroke(c, gr, Stroke::new(0.8f32, Color32::from_gray(shade)));
        }
        // fixed light reflections (light source doesn't rotate with the record)
        for (a0, alpha) in [(-2.4f32, 14u8), (-2.37, 14), (-2.34, 14), (0.75, 10), (0.78, 10)] {
            let mut pts = vec![c];
            for k in 0..=10 {
                pts.push(c + Vec2::angled(a0 + k as f32 * 0.035) * r * 0.97);
            }
            p.add(Shape::convex_polygon(pts, white_a(alpha), Stroke::NONE));
        }
        p.circle_filled(c, r * R_INNER, VINYL);

        // label = cover art, rotating with the platter
        let lr = r * 0.34;
        match np.cover {
            Some(t) => textured_disc(&p, c, lr, 0.0, np.anim.platter, t.id(), Color32::WHITE),
            None => {
                p.circle_filled(c, lr, MAGENTA);
                p.circle_filled(c, lr * 0.75, PURPLE);
                let mark = Vec2::angled(np.anim.platter);
                p.line_segment([c + mark * lr * 0.4, c + mark * lr * 0.95], Stroke::new(2.0f32, YELLOW));
            }
        }
        // a tiny label sticker so the spin reads even on a plain cover
        let sticker = c + Vec2::angled(np.anim.platter + 1.2) * lr * 0.78;
        p.circle_filled(sticker, u * 1.2, YELLOW);
        p.circle_stroke(c, lr, Stroke::new(1.0f32, Color32::from_black_alpha(120)));

        // click-to-drop-the-needle
        let rec = Rect::from_center_size(c, Vec2::splat(r * 2.0));
        let resp = ui.interact(rec, ui.id().with("vinyl-seek"), Sense::click());
        if let Some(pos) = resp.interact_pointer_pos() {
            let d = (pos - c).length() / r;
            if (R_INNER..=R_OUTER).contains(&d) {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                if resp.clicked() && np.dur > 0.0 {
                    let frac = (R_OUTER - d) / (R_OUTER - R_INNER);
                    actions.push(Action::Seek(frac as f64 * np.dur));
                }
            }
        }
    } else {
        p.circle_filled(c, r * 0.98, Color32::from_gray(70));
        p.circle_stroke(c, r * 0.7, Stroke::new(1.0f32, Color32::from_gray(60)));
    }
    // spindle
    p.circle_filled(c, u * 1.1, Color32::from_gray(210));

    // tonearm
    let pivot = Pos2::new(c.x + r * 1.28, c.y - r * 0.78);
    let arm_len = r * 1.32;
    let groove = R_OUTER - (R_OUTER - R_INNER) * np.progress();
    let on_record = needle_angle(pivot, c, arm_len, r * groove);
    let rest = std::f32::consts::FRAC_PI_2 + 0.12;
    let angle = rest + (on_record - rest) * np.anim.arm;
    let lift = np.anim.arm_lift;
    let tip = pivot + Vec2::angled(angle) * arm_len;
    let shadow = Vec2::splat(3.0 + 6.0 * lift);

    // arm rest post
    let post = pivot + Vec2::angled(rest) * arm_len * 0.72 + Vec2::new(u * 3.0, 0.0);
    p.circle_filled(post, u * 1.6, Color32::from_gray(40));
    p.circle_filled(post, u * 1.0, Color32::from_gray(140));

    let back = pivot - Vec2::angled(angle) * r * 0.32;
    p.line_segment([back + shadow, tip + shadow], Stroke::new(u * 1.5, Color32::from_black_alpha(110)));
    p.circle_filled(pivot, u * 5.0, Color32::from_gray(35));
    p.circle_stroke(pivot, u * 5.0, Stroke::new(1.5f32, YELLOW));
    p.line_segment([back, tip], Stroke::new(u * 1.3 * (1.0 + lift * 0.08), Color32::from_gray(205)));
    // counterweight
    p.circle_filled(back, u * 3.2, Color32::from_gray(60));
    p.circle_stroke(back, u * 3.2, Stroke::new(1.0f32, Color32::from_gray(150)));
    // headshell: angled block at the tip
    let head_dir = Vec2::angled(angle + 0.38);
    let n = head_dir.rot90();
    let h0 = tip - head_dir * u * 1.0;
    let h1 = tip + head_dir * u * 5.5;
    p.add(Shape::convex_polygon(
        vec![h0 + n * u * 1.6, h1 + n * u * 1.6, h1 - n * u * 1.6, h0 - n * u * 1.6],
        Color32::from_gray(30),
        Stroke::new(1.0f32, YELLOW),
    ));
    p.circle_filled(pivot, u * 2.0, Color32::from_gray(170));

    // controls column on the right
    let col = Rect::from_min_max(Pos2::new(c.x + r * 1.18, c.y + r * 0.05), Pos2::new(deck.right() - 4.0 * u, deck.bottom() - 10.0 * u));
    let btn = Rect::from_min_size(Pos2::new(col.left(), col.bottom() - 13.0 * u), Vec2::new(14.0 * u, 13.0 * u));
    let b = hotspot(ui, btn, "vinyl-start");
    p.rect_filled(btn, 2.0 * u, if b.hovered() { INDIGO } else { Color32::from_gray(30) });
    p.rect_stroke(btn, 2.0 * u, Stroke::new(1.5f32, YELLOW), StrokeKind::Inside);
    p.text(btn.center() - Vec2::new(0.0, 2.0 * u), Align2::CENTER_CENTER, "START", pixel((3.2 * u).max(9.0)), YELLOW);
    p.text(btn.center() + Vec2::new(0.0, 2.5 * u), Align2::CENTER_CENTER, "STOP", pixel((3.2 * u).max(9.0)), YELLOW);
    if b.clicked() {
        actions.push(Action::Toggle);
    }
    // 33/45 lamps
    for (i, label) in ["33", "45"].iter().enumerate() {
        let lc = Pos2::new(btn.right() + 5.0 * u + i as f32 * 7.0 * u, btn.top() + 3.0 * u);
        let on = i == 0 && np.playing;
        p.circle_filled(lc, u * 1.2, if on { MAGENTA } else { Color32::from_gray(50) });
        p.text(lc + Vec2::new(0.0, 4.0 * u), Align2::CENTER_CENTER, *label, pixel((3.0 * u).max(9.0)), YELLOW);
    }
    // pitch-slider lookalike doubling as volume
    let rail = Rect::from_min_size(Pos2::new(col.right() - 3.0 * u, col.top() + 4.0 * u), Vec2::new(2.0 * u, col.height() - 8.0 * u));
    p.rect_filled(rail, u, Color32::BLACK);
    let ky = rail.bottom() - rail.height() * np.volume;
    let knob = Rect::from_center_size(Pos2::new(rail.center().x, ky), Vec2::new(6.0 * u, 3.0 * u));
    p.rect_filled(knob, 1.0, Color32::from_gray(190));
    p.line_segment([knob.left_center(), knob.right_center()], Stroke::new(1.0f32, MAGENTA));
    p.text(Pos2::new(rail.center().x, rail.top() - 2.5 * u), Align2::CENTER_CENTER, "VOL", pixel((3.0 * u).max(9.0)), YELLOW);
    let vr = ui.interact(rail.expand2(Vec2::new(4.0 * u, 2.0 * u)), ui.id().with("vinyl-vol"), Sense::click_and_drag());
    if let Some(pos) = vr.interact_pointer_pos() {
        if vr.dragged() || vr.clicked() {
            actions.push(Action::Volume(((rail.bottom() - pos.y) / rail.height()).clamp(0.0, 1.0)));
        }
    }

    // brand plate + now playing text along the bottom edge
    let text_left = deck.left() + 4.0 * u;
    p.text(Pos2::new(deck.right() - 4.0 * u, deck.bottom() - 2.5 * u), Align2::RIGHT_BOTTOM, "JAMP  DIRECT DRIVE", bold((3.4 * u).max(11.0)), YELLOW);
    let info = Rect::from_min_max(Pos2::new(text_left, deck.bottom() - 6.0 * u), Pos2::new(c.x + r, deck.bottom() - 1.0 * u));
    let line = format!("{}  ·  {}  /  {}", np.meta.map(|m| m.display_title()).unwrap_or_else(|| "Drop a record on".into()), fmt_time(np.pos), fmt_time(np.dur));
    marquee(&p, info, &line, pixel((4.0 * u).max(12.0)), TEAL, np.time);

    actions
}

/// Angle (from `pivot`) that puts a stylus `len` away onto a groove of radius `groove`.
fn needle_angle(pivot: Pos2, center: Pos2, len: f32, groove: f32) -> f32 {
    let to_c = center - pivot;
    let d = to_c.length();
    let cos_phi = ((d * d + len * len - groove * groove) / (2.0 * d * len)).clamp(-1.0, 1.0);
    // of the two intersections, the one on the arm-rest side
    to_c.angle() - cos_phi.acos()
}
