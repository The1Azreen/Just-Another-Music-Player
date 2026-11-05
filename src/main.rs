#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod audio;
mod discord;
mod lyrics;
mod meta;
mod online;
mod skins;
mod theme;

use std::path::PathBuf;

/// Portable mode: if a `jamp-data` folder sits next to the executable (or the executable's name
/// contains "portable"), settings and the lyrics/cover cache live there instead of in the
/// user profile, so the whole thing can travel on a USB stick.
pub fn portable_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?.join("jamp-data");
    let named_portable = exe
        .file_stem()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.to_ascii_lowercase().contains("portable"));
    if dir.is_dir() || (named_portable && std::fs::create_dir_all(&dir).is_ok()) {
        Some(dir)
    } else {
        None
    }
}

const USAGE: &str = "\
jamp [FILES/FOLDERS/PLAYLISTS...] [options]

  --skin <modern|amp|cd|mp3|vinyl|tape>   start with this skin
  --seek <seconds>                         start the first track at this position
  --fresh                                  ignore and don't save the previous session
  --settings                               open the settings window at start
  --screenshot-dir <dir>                   screenshot every skin into <dir> and quit
  -h, --help                               this text";

fn main() -> eframe::Result {
    let mut opts = app::Options::default();
    let mut size = [1200.0f32, 760.0];
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--skin" => opts.skin = args.next().and_then(|s| skins::Skin::parse(&s)),
            "--seek" => opts.seek = args.next().and_then(|s| s.parse().ok()),
            "--fresh" => opts.fresh = true,
            "--settings" => opts.open_settings = true,
            "--screenshot-dir" => opts.screenshot_dir = args.next().map(PathBuf::from),
            "--size" => {
                if let Some((w, h)) = args.next().as_deref().and_then(|s| s.split_once('x')) {
                    size = [w.parse().unwrap_or(size[0]), h.parse().unwrap_or(size[1])];
                }
            }
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(());
            }
            _ => opts.files.push(PathBuf::from(a)),
        }
    }

    let icon = {
        let img = image::load_from_memory(include_bytes!("../assets/icon.png")).map(|i| i.to_rgba8());
        img.ok().map(|i| egui::IconData { width: i.width(), height: i.height(), rgba: i.into_raw() })
    };
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("JAMP — Just Another Music Player")
        .with_app_id("jamp")
        .with_inner_size(size)
        .with_min_inner_size([860.0, 560.0])
        .with_drag_and_drop(true);
    if opts.screenshot_dir.is_some() {
        // Don't let stray clicks or keystrokes (the window pops up over whatever you're doing)
        // change volume or seek mid-capture.
        viewport = viewport.with_mouse_passthrough(true).with_active(false);
    }
    if let Some(icon) = icon {
        viewport = viewport.with_icon(icon);
    }

    let native = eframe::NativeOptions {
        viewport,
        persistence_path: portable_dir().map(|d| d.join("settings.ron")),
        ..Default::default()
    };
    eframe::run_native("jamp", native, Box::new(|cc| Ok(Box::new(app::App::new(cc, opts)))))
}
