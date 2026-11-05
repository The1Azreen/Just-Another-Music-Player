//! foobar-style layout: the playlist does the talking; this is the now-playing column beside it.

use egui::{Align2, Color32, Rect, RichText, Sense, Ui, Vec2};

use super::*;

pub fn now_playing(ui: &mut Ui, np: &NowPlaying, cover_src: Option<&str>) {
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(w), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect.translate(Vec2::splat(3.0)), 0.0, Color32::BLACK);
    cover_or_placeholder(p, rect, np.cover);
    p.rect_stroke(rect, 0.0, egui::Stroke::new(2.0f32, YELLOW), egui::StrokeKind::Outside);
    if let Some(src) = cover_src {
        let tag = Rect::from_min_size(rect.right_bottom() - Vec2::new(70.0, 16.0), Vec2::new(70.0, 16.0));
        p.rect_filled(tag, 0.0, Color32::from_black_alpha(170));
        p.text(tag.center(), Align2::CENTER_CENTER, src, pixel(12.0), TEAL);
    }
    ui.add_space(6.0);

    ui.label(RichText::new(visual(&np.title())).font(bold(18.0)).color(YELLOW));
    if let Some(m) = np.meta {
        if !m.artist.is_empty() {
            ui.label(RichText::new(visual(&m.artist)).color(TEAL));
        }
        if !m.album.is_empty() {
            let year = m.year.map(|y| format!(" ({y})")).unwrap_or_default();
            ui.label(RichText::new(format!("{}{}", visual(&m.album), year)).color(DIM));
        }
        let mut tech = vec![m.codec.clone()];
        if let Some(b) = m.bitrate_kbps {
            tech.push(format!("{b} kbps"));
        }
        if let Some(r) = m.sample_rate {
            tech.push(format!("{:.1} kHz", r as f32 / 1000.0));
        }
        if let Some(c) = m.channels {
            tech.push(if c == 1 { "mono".into() } else if c == 2 { "stereo".into() } else { format!("{c} ch") });
        }
        if np.info.via_ffmpeg {
            tech.push("via ffmpeg".into());
        }
        ui.label(RichText::new(tech.join(" · ")).small().color(DIM));
    }
    ui.add_space(4.0);
    let (viz, _) = ui.allocate_exact_size(Vec2::new(w, 48.0), Sense::hover());
    ui.painter().rect_filled(viz, 0.0, Color32::BLACK);
    spectrum_bars(ui.painter(), viz.shrink(3.0), np.bars, np.peaks, 32, INDIGO, TEAL, MAGENTA);
}
