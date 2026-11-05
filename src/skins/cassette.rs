//! Cassette deck. Tape moves from the left pack to the right one as the song plays; the reels
//! turn at constant tape speed, so the emptier reel spins faster, like the real thing.

use egui::{Align2, Color32, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui, Vec2};

use super::*;

const SHELL: Color32 = Color32::from_rgb(0x1f, 0x1a, 0x2e);
const TAPE: Color32 = Color32::from_rgb(0x4a, 0x2c, 0x1c);
const HUB_RATIO: f32 = 0.36;

/// (left, right) tape pack radii, 1.0 = a full reel. Area is conserved as tape moves across.
pub fn pack_radii(progress: f32) -> (f32, f32) {
    let h2 = HUB_RATIO * HUB_RATIO;
    let l = (h2 + (1.0 - progress) * (1.0 - h2)).sqrt();
    let r = (h2 + progress * (1.0 - h2)).sqrt();
    (l, r)
}

pub fn draw(ui: &mut Ui, np: &NowPlaying) -> Vec<Action> {
    let mut actions = Vec::new();
    let avail = ui.available_rect_before_wrap().shrink(12.0);
    let deck = fit(avail, 1.45);
    ui.allocate_rect(deck, Sense::hover());
    let p = ui.painter_at(deck.expand(8.0));
    let u = deck.width() / 145.0;

    p.rect_filled(deck.translate(Vec2::splat(4.0)), 4.0 * u, Color32::BLACK);
    p.rect_filled(deck, 4.0 * u, Color32::from_rgb(0x2a, 0x22, 0x44));
    p.rect_stroke(deck, 4.0 * u, Stroke::new(2.0f32, YELLOW), StrokeKind::Inside);
    p.text(deck.left_top() + Vec2::new(5.0 * u, 4.0 * u), Align2::LEFT_TOP, "JAMP  STEREO CASSETTE DECK", bold((3.4 * u).max(11.0)), YELLOW);

    // cassette door
    let door = Rect::from_min_size(deck.min + Vec2::new(5.0 * u, 11.0 * u), Vec2::new(92.0 * u, 62.0 * u));
    p.rect_filled(door, 3.0 * u, Color32::from_rgb(0x14, 0x10, 0x22));
    p.rect_stroke(door, 3.0 * u, Stroke::new(1.5f32, TEAL), StrokeKind::Inside);
    let shell = door.shrink2(Vec2::new(4.0 * u, 4.0 * u));

    if np.loaded {
        draw_cassette(&p, shell, np);
    } else {
        p.text(shell.center(), Align2::CENTER_CENTER, "NO TAPE", bold((5.0 * u).max(14.0)), DIM);
    }
    // smoked door glass over everything
    p.rect_filled(door.shrink(2.0), 3.0 * u, white_a(8));
    p.line_segment([door.left_top() + Vec2::new(10.0 * u, 2.0), door.left_top() + Vec2::new(30.0 * u, 2.0)], Stroke::new(2.0f32, white_a(25)));

    // right-hand side: counter + VU meters
    let side = Rect::from_min_max(Pos2::new(door.right() + 5.0 * u, door.top()), Pos2::new(deck.right() - 5.0 * u, door.bottom()));
    let counter = Rect::from_min_size(side.min, Vec2::new(side.width(), 11.0 * u));
    p.rect_filled(counter, 1.0, Color32::BLACK);
    let turns = (np.anim.reel_r / std::f32::consts::TAU) as u32 % 1000;
    let digits = format!("{turns:03}");
    let dw = counter.width() * 0.5 / 3.0;
    for (i, ch) in digits.chars().enumerate() {
        let r = Rect::from_min_size(Pos2::new(counter.left() + 2.0 * u + i as f32 * (dw + 1.0), counter.top() + 1.5 * u), Vec2::new(dw, counter.height() - 3.0 * u));
        p.rect_filled(r, 1.0, WHITE);
        p.text(r.center(), Align2::CENTER_CENTER, ch.to_string(), bold(r.height() * 0.9), INK);
    }
    p.text(counter.right_center() - Vec2::new(2.0 * u, 0.0), Align2::RIGHT_CENTER, "COUNT", pixel((3.0 * u).max(9.0)), YELLOW);

    let half = np.bars.len() / 2;
    let avg = |s: &[f32]| if s.is_empty() { 0.0 } else { s.iter().sum::<f32>() / s.len() as f32 };
    let levels = [avg(&np.bars[..half]), avg(&np.bars[half..])];
    let vu_h = (side.height() - counter.height() - 6.0 * u) / 2.0;
    for (i, (label, level)) in ["LO", "HI"].iter().zip(levels).enumerate() {
        let r = Rect::from_min_size(Pos2::new(side.left(), counter.bottom() + 3.0 * u + i as f32 * (vu_h + 3.0 * u)), Vec2::new(side.width(), vu_h));
        vu_meter(&p, r, level * 1.25, label, u);
    }

    // piano keys
    let keys_y = door.bottom() + 6.0 * u;
    let keys: [(&str, Option<Icon>, bool); 6] = [
        ("REC", None, false),
        ("PLAY", Some(Icon::Play), np.playing),
        ("REW", Some(Icon::Rewind), false),
        ("F.FWD", Some(Icon::FastForward), false),
        ("STOP", Some(Icon::Stop), false),
        ("PAUSE", Some(Icon::Pause), np.loaded && !np.playing),
    ];
    let kw = (deck.width() - 10.0 * u - 5.0 * u) / keys.len() as f32;
    for (i, (label, icon, down)) in keys.iter().enumerate() {
        let r = Rect::from_min_size(Pos2::new(deck.left() + 5.0 * u + i as f32 * (kw + 1.0 * u), keys_y), Vec2::new(kw, deck.bottom() - keys_y - 5.0 * u));
        let resp = hotspot(ui, r, label);
        let pressed = *down || resp.is_pointer_button_down_on();
        let face = if pressed { r.translate(Vec2::new(0.0, 1.5 * u)) } else { r };
        p.rect_filled(r.translate(Vec2::new(0.0, 2.5 * u)), 2.0, Color32::BLACK);
        p.rect_filled(face, 2.0, if resp.hovered() { Color32::from_gray(205) } else if pressed { Color32::from_gray(150) } else { Color32::from_gray(185) });
        p.rect_stroke(face, 2.0, Stroke::new(1.0f32, Color32::from_gray(90)), StrokeKind::Inside);
        let ir = Rect::from_center_size(face.center() - Vec2::new(0.0, 2.0 * u), Vec2::splat(face.height() * 0.42));
        match icon {
            Some(ic) => paint_icon(&p, ir, *ic, INK),
            None => {
                p.circle_filled(ir.center(), ir.height() * 0.25, Color32::from_rgb(200, 40, 50));
            }
        }
        p.text(face.center_bottom() - Vec2::new(0.0, 2.0 * u), Align2::CENTER_BOTTOM, *label, pixel((3.0 * u).max(9.0)), INK);
        if resp.clicked() {
            match *label {
                "PLAY" => actions.push(Action::Play),
                "REW" => actions.push(Action::Seek((np.pos - 10.0).max(0.0))),
                "F.FWD" => actions.push(Action::Seek(np.pos + 10.0)),
                "STOP" => actions.push(Action::Stop),
                "PAUSE" => actions.push(Action::Pause),
                _ => {}
            }
        }
    }
    actions
}

fn draw_cassette(p: &egui::Painter, shell: Rect, np: &NowPlaying) {
    let s = shell.width() / 100.0;
    p.rect_filled(shell, 3.0 * s, SHELL);
    p.rect_stroke(shell, 3.0 * s, Stroke::new(1.0f32, Color32::from_gray(80)), StrokeKind::Inside);
    for (x, y) in [(4.0, 4.0), (96.0, 4.0), (4.0, 59.0), (96.0, 59.0), (50.0, 59.0)] {
        screw(p, shell.min + Vec2::new(x * s, y * s), 1.6 * s, 0.7);
    }

    // window: the tape packs are only ever seen through it
    let window = Rect::from_min_size(shell.min + Vec2::new(18.0 * s, 13.0 * s), Vec2::new(64.0 * s, 24.0 * s));
    p.rect_filled(window, 2.0 * s, Color32::from_rgb(0x2a, 0x24, 0x38));
    let (l, r) = pack_radii(np.progress());
    let full = 13.5 * s;
    let cl = Pos2::new(shell.left() + 31.0 * s, window.center().y);
    let cr = Pos2::new(shell.left() + 69.0 * s, window.center().y);
    let through = p.with_clip_rect(window.intersect(p.clip_rect()));
    for (c, pack, angle) in [(cl, l, np.anim.reel_l), (cr, r, np.anim.reel_r)] {
        through.circle_filled(c, pack * full, TAPE);
        for k in 1..4 {
            // sheen rings on the pack
            through.circle_stroke(c, pack * full * (0.55 + k as f32 * 0.12), Stroke::new(0.6f32, Color32::from_rgba_unmultiplied(120, 80, 50, 70)));
        }
        reel_hub(&through, c, HUB_RATIO * full, angle);
    }
    // smoked plastic over the window
    through.rect_filled(window, 2.0 * s, Color32::from_rgba_unmultiplied(30, 20, 60, 50));
    through.line_segment([window.left_top() + Vec2::new(4.0 * s, 3.0 * s), window.left_top() + Vec2::new(14.0 * s, 3.0 * s)], Stroke::new(1.0 * s, white_a(40)));

    // label, drawn around the window
    let label = Rect::from_min_size(shell.min + Vec2::new(8.0 * s, 4.0 * s), Vec2::new(84.0 * s, 42.0 * s));
    let cream = Color32::from_rgb(0xf3, 0xe9, 0xd2);
    for part in [
        Rect::from_min_max(label.min, Pos2::new(label.right(), window.top())),
        Rect::from_min_max(Pos2::new(label.left(), window.bottom()), label.max),
        Rect::from_min_max(Pos2::new(label.left(), window.top()), Pos2::new(window.left(), window.bottom())),
        Rect::from_min_max(Pos2::new(window.right(), window.top()), Pos2::new(label.right(), window.bottom())),
    ] {
        p.rect_filled(part, 0.0, cream);
    }
    p.rect_stroke(window, 2.0 * s, Stroke::new(1.0f32, Color32::from_gray(90)), StrokeKind::Outside);
    p.rect_filled(Rect::from_min_size(Pos2::new(label.left(), window.top() - 2.5 * s), Vec2::new(label.width(), 1.5 * s)), 0.0, MAGENTA);
    p.rect_filled(Rect::from_min_size(Pos2::new(label.left(), window.bottom() + 1.0 * s), Vec2::new(label.width(), 1.5 * s)), 0.0, TEAL);
    p.text(label.min + Vec2::new(2.5 * s, 0.5 * s), Align2::LEFT_TOP, "A", bold(6.5 * s), INK);
    let title_r = Rect::from_min_size(label.min + Vec2::new(9.0 * s, 0.3 * s), Vec2::new(label.width() - 11.0 * s, 6.5 * s));
    marquee(p, title_r, &np.meta.map(|m| m.display_title()).unwrap_or_default(), bold(5.0 * s), INK, np.time);
    p.text(label.left_bottom() + Vec2::new(3.0 * s, -1.0 * s), Align2::LEFT_BOTTOM, "TYPE II  HIGH (CrO2)  90", pixel((3.0 * s).max(9.0)), INK);
    p.text(label.right_bottom() + Vec2::new(-3.0 * s, -1.0 * s), Align2::RIGHT_BOTTOM, "JAMP", bold((3.5 * s).max(10.0)), MAGENTA);

    // bottom trapezoid with head openings
    let bottom = shell.bottom();
    let trap = vec![
        Pos2::new(shell.left() + 18.0 * s, bottom),
        Pos2::new(shell.left() + 23.0 * s, bottom - 9.0 * s),
        Pos2::new(shell.right() - 23.0 * s, bottom - 9.0 * s),
        Pos2::new(shell.right() - 18.0 * s, bottom),
    ];
    p.add(Shape::convex_polygon(trap, Color32::from_rgb(0x2a, 0x24, 0x38), Stroke::new(1.0f32, Color32::from_gray(80))));
    for x in [30.0, 47.0, 53.0, 70.0] {
        p.rect_filled(Rect::from_center_size(Pos2::new(shell.left() + x * s, bottom - 3.0 * s), Vec2::new(3.5 * s, 3.5 * s)), 0.5, Color32::BLACK);
    }
    p.line_segment(
        [Pos2::new(shell.left() + 22.0 * s, bottom - 5.5 * s), Pos2::new(shell.right() - 22.0 * s, bottom - 5.5 * s)],
        Stroke::new(1.2 * s, TAPE),
    );
    // the deck's tape head, pressed against the tape while playing
    let head_y = if np.playing { bottom - 6.0 * s } else { bottom - 1.0 * s };
    p.rect_filled(Rect::from_center_size(Pos2::new(shell.center().x, head_y + 2.5 * s), Vec2::new(8.0 * s, 4.0 * s)), 1.0, Color32::from_gray(180));
}

fn reel_hub(p: &egui::Painter, c: Pos2, r: f32, angle: f32) {
    p.circle_filled(c, r, Color32::from_gray(225));
    p.circle_filled(c, r * 0.55, Color32::from_gray(40));
    for i in 0..6 {
        let a = angle + i as f32 * std::f32::consts::TAU / 6.0;
        let d = Vec2::angled(a);
        let n = d.rot90();
        let base = c + d * r * 0.3;
        let tipp = c + d * r * 0.62;
        p.add(Shape::convex_polygon(
            vec![base + n * r * 0.1, tipp + n * r * 0.07, tipp - n * r * 0.07, base - n * r * 0.1],
            Color32::from_gray(225),
            Stroke::NONE,
        ));
    }
    // spokes on the outer flange, so rotation is visible
    for i in 0..3 {
        let a = angle + i as f32 * std::f32::consts::TAU / 3.0;
        p.line_segment([c + Vec2::angled(a) * r * 0.62, c + Vec2::angled(a) * r * 0.95], Stroke::new(1.2f32, Color32::from_gray(140)));
    }
}

fn vu_meter(p: &egui::Painter, r: Rect, level: f32, label: &str, u: f32) {
    p.rect_filled(r, 2.0, Color32::from_rgb(0xf6, 0xe7, 0xb8));
    p.rect_stroke(r, 2.0, Stroke::new(1.0f32, Color32::from_gray(80)), StrokeKind::Inside);
    let pivot = Pos2::new(r.center().x, r.bottom() + r.height() * 0.35);
    let len = r.height() * 1.05;
    let (a0, a1) = (-2.25f32, -0.89f32);
    for i in 0..=10 {
        let t = i as f32 / 10.0;
        let a = a0 + (a1 - a0) * t;
        let col = if t > 0.7 { Color32::from_rgb(200, 30, 40) } else { INK };
        p.line_segment([pivot + Vec2::angled(a) * len * 0.82, pivot + Vec2::angled(a) * len * (if i % 5 == 0 { 0.93 } else { 0.88 })], Stroke::new(1.0f32, col));
    }
    let a = a0 + (a1 - a0) * level.clamp(0.0, 1.05);
    let clip = p.with_clip_rect(r.shrink(1.0));
    clip.line_segment([pivot, pivot + Vec2::angled(a) * len * 0.95], Stroke::new(1.3f32, Color32::from_rgb(180, 20, 30)));
    p.text(r.left_top() + Vec2::new(2.0 * u, 1.5 * u), Align2::LEFT_TOP, label, pixel((3.0 * u).max(9.0)), INK);
    p.text(r.right_top() + Vec2::new(-2.0 * u, 1.5 * u), Align2::RIGHT_TOP, "VU", bold((3.0 * u).max(9.0)), INK);
}
