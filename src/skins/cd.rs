//! Portable CD player seen from above through its clear lid. The cover is printed on the disc,
//! which spins up when playing; the lid swings open when nothing is loaded.

use egui::{Align2, Color32, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui, Vec2};

use super::*;

pub fn draw(ui: &mut Ui, np: &NowPlaying) -> Vec<Action> {
    let mut actions = Vec::new();
    let avail = ui.available_rect_before_wrap().shrink(12.0);
    let area = fit(avail, 0.82);
    ui.allocate_rect(area, Sense::hover());
    let p = ui.painter_at(area.expand(8.0));
    let u = area.width() / 100.0;

    let c = Pos2::new(area.center().x, area.top() + 48.0 * u);
    let body_r = 46.0 * u;

    // body
    p.circle_filled(c + Vec2::splat(4.0), body_r, Color32::BLACK);
    p.circle_filled(c, body_r, Color32::from_rgb(0x4a, 0x3c, 0x86));
    p.circle_filled(c, body_r * 0.97, Color32::from_rgb(0x3c, 0x2e, 0x7b));
    // brushed-metal rings
    for k in 0..14 {
        p.circle_stroke(c, body_r * (0.82 + k as f32 * 0.011), Stroke::new(0.7f32, white_a(10 + (k % 3) as u8 * 6)));
    }
    p.circle_stroke(c, body_r, Stroke::new(2.0f32, YELLOW));

    // disc well
    let well_r = body_r * 0.8;
    p.circle_filled(c, well_r, Color32::from_rgb(0x12, 0x0e, 0x1e));

    let disc_r = well_r * 0.96;
    if np.loaded {
        let a = np.anim.cd;
        p.circle_filled(c, disc_r, Color32::from_gray(200));
        match np.cover {
            Some(t) => textured_disc(&p, c, disc_r * 0.99, disc_r * 0.16, a, t.id(), Color32::WHITE),
            None => {
                ring(&p, c, disc_r * 0.99, disc_r * 0.16, PURPLE);
                for i in 0..3 {
                    let d = Vec2::angled(a + i as f32 * 2.094);
                    p.line_segment([c + d * disc_r * 0.3, c + d * disc_r * 0.9], Stroke::new(3.0 * u, INDIGO));
                }
                p.text(c + Vec2::angled(a) * disc_r * 0.6, Align2::CENTER_CENTER, "JAMP", bold(5.0 * u), YELLOW);
            }
        }
        // iridescent sheen: fixed to the light, not the disc
        let sheen = [
            (-0.6f32, Color32::from_rgba_unmultiplied(110, 230, 250, 46)),
            (-0.35, Color32::from_rgba_unmultiplied(255, 110, 240, 40)),
            (-0.1, Color32::from_rgba_unmultiplied(250, 210, 80, 34)),
            (2.6, Color32::from_rgba_unmultiplied(110, 230, 250, 34)),
            (2.85, Color32::from_rgba_unmultiplied(255, 110, 240, 30)),
        ];
        for (a0, col) in sheen {
            let mut pts = vec![c + Vec2::angled(a0) * disc_r * 0.2, c + Vec2::angled(a0 + 0.25) * disc_r * 0.2];
            for k in (0..=6).rev() {
                pts.push(c + Vec2::angled(a0 + k as f32 * 0.04) * disc_r * 0.98);
            }
            p.add(Shape::convex_polygon(pts, col, Stroke::NONE));
        }
        // clear hub + mirror band
        ring(&p, c, disc_r * 0.33, disc_r * 0.16, white_a(70));
        p.circle_stroke(c, disc_r * 0.33, Stroke::new(1.0f32, white_a(120)));
        p.circle_stroke(c, disc_r, Stroke::new(1.0f32, Color32::from_gray(160)));
    }
    // spindle
    p.circle_filled(c, disc_r * 0.15, Color32::from_gray(30));
    p.circle_filled(c, disc_r * 0.09, Color32::from_gray(170));
    for i in 0..3 {
        let a = np.anim.cd + i as f32 * 2.094;
        p.circle_filled(c + Vec2::angled(a) * disc_r * 0.12, u * 0.6, Color32::from_gray(90));
    }

    // lid: clear dome when closed, slides up out of the way when open
    let lid_off = np.anim.cd_lid * body_r * 0.55;
    let lc = c - Vec2::new(0.0, lid_off);
    let lid_alpha = (1.0 - np.anim.cd_lid * 0.6).max(0.2);
    p.circle_stroke(lc, well_r * 1.02, Stroke::new(2.0f32, white_a((90.0 * lid_alpha) as u8)));
    let mut glint = Vec::new();
    for k in 0..=12 {
        glint.push(lc + Vec2::angled(-2.6 + k as f32 * 0.06) * well_r * 0.92);
    }
    p.add(Shape::line(glint, Stroke::new(3.0 * u * 0.4, white_a((60.0 * lid_alpha) as u8))));

    // LCD strip
    let lcd = Rect::from_center_size(Pos2::new(c.x, area.top() + 101.0 * u), Vec2::new(70.0 * u, 13.0 * u));
    p.rect_filled(lcd.translate(Vec2::splat(3.0)), 2.0, Color32::BLACK);
    p.rect_filled(lcd, 2.0, Color32::from_rgb(0x9c, 0xc8, 0x8c));
    p.rect_stroke(lcd, 2.0, Stroke::new(2.0f32, YELLOW), StrokeKind::Inside);
    let lcd_ink = Color32::from_rgb(0x1a, 0x2a, 0x1a);
    let track_no = np.index.map(|i| format!("{:02}", i + 1)).unwrap_or_else(|| "--".into());
    p.text(lcd.left_center() + Vec2::new(3.0 * u, 0.0), Align2::LEFT_CENTER, format!("TRK {track_no}"), bold((4.0 * u).max(11.0)), lcd_ink);
    p.text(lcd.right_center() - Vec2::new(3.0 * u, 0.0), Align2::RIGHT_CENTER, fmt_time(np.pos), bold((6.0 * u).max(14.0)), lcd_ink);
    let state = if !np.loaded { "NO DISC" } else if np.playing { "PLAY" } else { "PAUSE" };
    p.text(lcd.center(), Align2::CENTER_CENTER, state, pixel((3.6 * u).max(10.0)), lcd_ink);
    // progress ticks under the digits
    let prog = Rect::from_min_size(lcd.left_bottom() + Vec2::new(3.0 * u, -2.5 * u), Vec2::new(lcd.width() - 6.0 * u, 1.2 * u));
    if let Some(f) = seek_rail(ui, prog, np.progress(), Color32::from_rgb(0x7c, 0xa8, 0x6c), lcd_ink, lcd_ink) {
        actions.push(Action::Seek(f as f64 * np.dur));
    }

    // buttons below the LCD
    let row_y = lcd.bottom() + 4.0 * u;
    let size = Vec2::new(12.0 * u, 8.0 * u);
    let icons = [Icon::Prev, Icon::Stop, if np.playing { Icon::Pause } else { Icon::Play }, Icon::Next, Icon::Eject];
    let total = icons.len() as f32 * size.x + (icons.len() - 1) as f32 * 2.0 * u;
    let mut ui_row = ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_min_size(Pos2::new(c.x - total / 2.0, row_y), Vec2::new(total + 1.0, size.y + 1.0))));
    ui_row.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0 * u;
        for icon in icons {
            if icon_button(ui, size, icon, icon == Icon::Pause).clicked() {
                actions.push(match icon {
                    Icon::Prev => Action::Prev,
                    Icon::Stop => Action::Stop,
                    Icon::Play | Icon::Pause => Action::Toggle,
                    Icon::Next => Action::Next,
                    _ => Action::Open,
                });
            }
        }
    });

    // brand + "anti-skip" badge on the body
    p.text(c + Vec2::new(0.0, body_r * 0.9), Align2::CENTER_CENTER, "JAMP  DIGITAL AUDIO", bold((3.4 * u).max(10.0)), YELLOW);
    p.text(c + Vec2::new(body_r * 0.62, -body_r * 0.78), Align2::CENTER_CENTER, "ESP 40s", pixel((3.0 * u).max(9.0)), TEAL);
    let led = c + Vec2::new(-body_r * 0.62, -body_r * 0.78);
    p.circle_filled(led, u * 1.1, if np.playing { MAGENTA } else { Color32::from_gray(60) });

    actions
}
