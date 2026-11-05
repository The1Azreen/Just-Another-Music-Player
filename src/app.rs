use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Instant;

use egui::{Align2, Color32, Key, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, TextureHandle, Ui, Vec2};

use crate::audio::{self, Player};
use crate::discord;
use crate::lyrics::Lyrics;
use crate::meta::{self, TrackMeta};
use crate::online::{self, Extras, Origin};
use crate::skins::{self, Action, Anim, Icon, NowPlaying, Repeat, Skin};
use crate::theme::*;

const BANDS: usize = 32;
const ROW_H: f32 = 20.0;
const STARTUP_VOLUME: f32 = 0.05;

#[derive(Default)]
pub struct Options {
    pub files: Vec<PathBuf>,
    pub skin: Option<Skin>,
    /// Screenshot each skin into this directory, then quit.
    pub screenshot_dir: Option<PathBuf>,
    /// Start position in seconds (handy for screenshots: tonearm mid-record, tape half-wound).
    pub seek: Option<f64>,
    pub fresh: bool,
    pub open_settings: bool,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct Saved {
    playlist: Vec<PathBuf>,
    current: Option<usize>,
    skin: Skin,
    volume: f32,
    shuffle: bool,
    repeat: Repeat,
    online: bool,
    glow: bool,
    discord: bool,
    discord_client_id: String,
}

impl Default for Saved {
    fn default() -> Self {
        Saved {
            playlist: Vec::new(),
            current: None,
            skin: Skin::Modern,
            volume: STARTUP_VOLUME,
            shuffle: false,
            repeat: Repeat::All,
            online: true,
            glow: true,
            discord: false,
            discord_client_id: String::new(),
        }
    }
}

struct ScreenshotPlan {
    dir: PathBuf,
    queue: Vec<Skin>,
    next_at: f64,
    waiting: bool,
}

struct Viz {
    bars: Vec<f32>,
    peaks: Vec<f32>,
    wave: Vec<f32>,
}

pub struct App {
    player: Player,
    playlist: Vec<PathBuf>,
    library: HashMap<PathBuf, TrackMeta>,
    current: Option<usize>,
    selected: Option<usize>,
    skin: Skin,
    shuffle: bool,
    repeat: Repeat,
    online: bool,
    glow: bool,
    discord: bool,
    discord_client_id: String,
    presence: discord::Presence,
    last_presence: Option<discord::Activity>,
    cover_url: Option<String>,
    show_settings: bool,
    history: Vec<usize>,
    rng: u64,

    filter: String,
    filtered: Vec<usize>,
    filter_dirty: bool,
    scroll_to_current: bool,

    meta_tx: Sender<PathBuf>,
    meta_rx: Receiver<(PathBuf, TrackMeta)>,
    extras_tx: Sender<Extras>,
    extras_rx: Receiver<Extras>,

    cover: Option<(TextureHandle, Origin)>,
    lyrics: Option<(Lyrics, Origin)>,
    extras_loading: bool,
    last_lyric_line: Option<usize>,
    right_tab_lyrics: bool,

    viz: Viz,
    anim: Anim,
    started: Instant,
    last_frame: f64,
    status: Option<(String, f64)>,
    pending_seek: Option<f64>,
    screenshot: Option<ScreenshotPlan>,
    persist: bool,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, opts: Options) -> Self {
        install(&cc.egui_ctx);

        let saved: Saved = if opts.fresh || opts.screenshot_dir.is_some() {
            Saved::default()
        } else {
            cc.storage.and_then(|s| eframe::get_value(s, "jamp")).unwrap_or_default()
        };

        // Tag reading happens on one background thread so big folders don't stall the UI.
        let (meta_tx, meta_jobs) = channel::<PathBuf>();
        let (meta_done, meta_rx) = channel();
        let ctx = cc.egui_ctx.clone();
        std::thread::spawn(move || {
            for path in meta_jobs {
                let m = meta::read_meta(&path);
                if meta_done.send((path, m)).is_err() {
                    break;
                }
                ctx.request_repaint();
            }
        });
        let (extras_tx, extras_rx) = channel();

        let mut player = Player::new();
        // Always start quiet rather than restoring whatever the last session ended at.
        player.set_volume(STARTUP_VOLUME);

        let mut app = App {
            player,
            playlist: Vec::new(),
            library: HashMap::new(),
            current: None,
            selected: None,
            skin: opts.skin.unwrap_or(saved.skin),
            shuffle: saved.shuffle,
            repeat: saved.repeat,
            online: saved.online,
            glow: saved.glow,
            discord: saved.discord,
            discord_client_id: saved.discord_client_id,
            presence: discord::Presence::new(),
            last_presence: None,
            cover_url: None,
            show_settings: opts.open_settings,
            history: Vec::new(),
            rng: 0x9E37_79B9_7F4A_7C15 ^ std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1),
            filter: String::new(),
            filtered: Vec::new(),
            filter_dirty: true,
            scroll_to_current: false,
            meta_tx,
            meta_rx,
            extras_tx,
            extras_rx,
            cover: None,
            lyrics: None,
            extras_loading: false,
            last_lyric_line: None,
            right_tab_lyrics: false,
            viz: Viz { bars: vec![0.0; BANDS], peaks: vec![0.0; BANDS], wave: Vec::new() },
            anim: Anim { cd_lid: 1.0, arm_lift: 1.0, ..Default::default() },
            started: Instant::now(),
            last_frame: 0.0,
            status: None,
            pending_seek: opts.seek,
            screenshot: opts.screenshot_dir.map(|dir| ScreenshotPlan { dir, queue: Skin::ALL.to_vec(), next_at: 4.0, waiting: false }),
            persist: !opts.fresh,
        };
        if app.screenshot.is_some() {
            app.persist = false;
        }
        set_glow(app.glow);
        app.presence.configure(app.discord, &app.discord_client_id);

        let restored: Vec<PathBuf> = saved.playlist.into_iter().filter(|p| p.exists()).collect();
        let had_restored = !restored.is_empty();
        app.add_paths(restored);
        if !opts.files.is_empty() {
            let start = app.playlist.len();
            app.add_paths(opts.files);
            if app.playlist.len() > start {
                app.play_index(start, &cc.egui_ctx);
            }
        } else if had_restored {
            if let Some(i) = saved.current.filter(|i| *i < app.playlist.len()) {
                app.load_index(i, true, &cc.egui_ctx);
            }
        }
        app
    }

    fn time(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    fn set_status(&mut self, s: impl Into<String>) {
        self.status = Some((s.into(), self.time() + 6.0));
    }

    fn rand(&mut self) -> u64 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        self.rng
    }

    // ----------------------------------------------------------------------------- playlist

    fn add_paths(&mut self, paths: Vec<PathBuf>) {
        let mut files = Vec::new();
        for p in paths {
            collect(&p, &mut files);
        }
        for f in files {
            if !self.library.contains_key(&f) {
                // placeholder from the file name until the tag reader gets to it
                let mut m = TrackMeta::default();
                m.title = f.file_stem().and_then(|s| s.to_str()).unwrap_or("?").to_string();
                self.library.insert(f.clone(), m);
                let _ = self.meta_tx.send(f.clone());
            }
            self.playlist.push(f);
        }
        self.filter_dirty = true;
    }

    fn remove(&mut self, idx: usize) {
        if idx >= self.playlist.len() {
            return;
        }
        self.playlist.remove(idx);
        self.current = match self.current {
            Some(c) if c == idx => {
                self.player.stop();
                None
            }
            Some(c) if c > idx => Some(c - 1),
            c => c,
        };
        self.selected = self.selected.filter(|s| *s < self.playlist.len());
        self.history.clear();
        self.filter_dirty = true;
    }

    fn clear(&mut self) {
        self.player.stop();
        self.playlist.clear();
        self.current = None;
        self.selected = None;
        self.cover = None;
        self.lyrics = None;
        self.history.clear();
        self.filter_dirty = true;
    }

    fn meta_of(&self, idx: usize) -> Option<&TrackMeta> {
        self.playlist.get(idx).and_then(|p| self.library.get(p))
    }

    fn load_index(&mut self, idx: usize, paused: bool, ctx: &egui::Context) {
        let Some(path) = self.playlist.get(idx).cloned() else { return };
        if let Err(e) = self.player.load(&path, 0.0, paused) {
            self.set_status(e);
            return;
        }
        if let Some(prev) = self.current.filter(|c| *c != idx) {
            self.history.push(prev);
        }
        self.current = Some(idx);
        self.selected = Some(idx);
        self.scroll_to_current = true;
        self.cover = None;
        self.cover_url = None;
        self.lyrics = None;
        self.last_lyric_line = None;
        self.extras_loading = true;

        let meta = self.library.get(&path).cloned().filter(|m| m.tagged || m.duration.is_some());
        let online = self.online;
        let tx = self.extras_tx.clone();
        let ctx2 = ctx.clone();
        std::thread::spawn(move || {
            let meta = meta.unwrap_or_else(|| meta::read_meta(&path));
            let _ = tx.send(online::resolve(&path, &meta, online));
            ctx2.request_repaint();
        });

        let title = self.meta_of(idx).map(|m| m.display_title()).unwrap_or_default();
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!("{title} — JAMP")));
    }

    fn play_index(&mut self, idx: usize, ctx: &egui::Context) {
        self.load_index(idx, false, ctx);
    }

    fn next_index(&mut self, manual: bool) -> Option<usize> {
        let n = self.playlist.len();
        if n == 0 {
            return None;
        }
        if !manual && self.repeat == Repeat::One {
            return self.current;
        }
        if self.shuffle && n > 1 {
            loop {
                let i = (self.rand() % n as u64) as usize;
                if Some(i) != self.current {
                    return Some(i);
                }
            }
        }
        match self.current {
            None => Some(0),
            Some(c) if c + 1 < n => Some(c + 1),
            Some(_) if manual || self.repeat == Repeat::All => Some(0),
            Some(_) => None,
        }
    }

    fn act(&mut self, a: Action, ctx: &egui::Context) {
        match a {
            Action::Toggle => {
                if self.player.is_playing() {
                    self.player.set_paused(true);
                } else {
                    self.act(Action::Play, ctx);
                }
            }
            Action::Play => {
                if self.player.is_paused() {
                    self.player.set_paused(false);
                } else if !self.player.is_loaded() {
                    if let Some(i) = self.selected.or(self.current).or((!self.playlist.is_empty()).then_some(0)) {
                        self.play_index(i, ctx);
                    } else {
                        self.open_files();
                    }
                }
            }
            Action::Pause => {
                if self.player.is_loaded() {
                    self.player.set_paused(!self.player.is_paused());
                }
            }
            Action::Stop => {
                self.player.stop();
            }
            Action::Next => {
                if let Some(i) = self.next_index(true) {
                    self.play_index(i, ctx);
                }
            }
            Action::Prev => {
                if self.player.position() > 3.0 {
                    self.player.seek(0.0);
                } else if let Some(i) = if self.shuffle { self.history.pop() } else { None }
                    .or_else(|| self.current.map(|c| if c == 0 { self.playlist.len().saturating_sub(1) } else { c - 1 }))
                {
                    let hist = std::mem::take(&mut self.history);
                    self.play_index(i, ctx);
                    self.history = hist;
                }
            }
            Action::Seek(t) => {
                if self.player.is_loaded() {
                    let dur = self.duration();
                    self.player.seek(if dur > 0.0 { t.min(dur - 0.25) } else { t });
                }
            }
            Action::Volume(v) => self.player.set_volume(v),
            Action::Open => self.open_files(),
            Action::Shuffle => self.shuffle = !self.shuffle,
            Action::Repeat => {
                self.repeat = match self.repeat {
                    Repeat::Off => Repeat::All,
                    Repeat::All => Repeat::One,
                    Repeat::One => Repeat::Off,
                }
            }
            Action::PlayIndex(i) => self.play_index(i, ctx),
        }
    }

    fn open_files(&mut self) {
        if let Some(files) = rfd::FileDialog::new()
            .add_filter("Audio", audio::AUDIO_EXTENSIONS)
            .add_filter("Playlists", &["m3u", "m3u8"])
            .add_filter("All files", &["*"])
            .pick_files()
        {
            self.add_paths(files);
        }
    }

    fn open_folder(&mut self) {
        if let Some(dirs) = rfd::FileDialog::new().pick_folders() {
            self.add_paths(dirs);
        }
    }

    fn duration(&self) -> f64 {
        self.current.and_then(|i| self.meta_of(i)).and_then(|m| m.duration).unwrap_or(0.0)
    }

    // ----------------------------------------------------------------------------- per frame

    fn pump(&mut self, ctx: &egui::Context) {
        while let Ok((path, m)) = self.meta_rx.try_recv() {
            self.library.insert(path, m);
            self.filter_dirty = true;
        }
        while let Ok(ex) = self.extras_rx.try_recv() {
            if self.current.and_then(|i| self.playlist.get(i)) != Some(&ex.path) {
                continue; // stale: the user moved on
            }
            self.extras_loading = false;
            self.cover = ex.cover.map(|(img, o)| (ctx.load_texture("cover", img, egui::TextureOptions::LINEAR), o));
            self.lyrics = ex.lyrics.map(|(l, o)| (Lyrics::parse(&l), o));
            self.cover_url = ex.cover_url;
        }
        if let Some(t) = self.pending_seek {
            if self.player.is_loaded() && self.duration() > 0.0 {
                self.player.seek(t);
                self.pending_seek = None;
            }
        }
        if self.player.track_finished() {
            match self.next_index(false) {
                Some(i) => self.play_index(i, ctx),
                None => self.player.stop(),
            }
        }
        if let Some(e) = self.player.last_error.take() {
            self.set_status(e);
        }
    }

    fn update_viz(&mut self, dt: f32) {
        let samples = self.player.recent_samples(2048);
        let rate = self.player.info.sample_rate.max(8000);
        let fresh = if self.player.is_playing() { audio::spectrum(&samples, BANDS, rate) } else { vec![0.0; BANDS] };
        for i in 0..BANDS {
            let b = &mut self.viz.bars[i];
            *b = if fresh[i] > *b { *b + (fresh[i] - *b) * 0.6 } else { (*b - dt * 1.6).max(fresh[i]) };
            let pk = &mut self.viz.peaks[i];
            *pk = if *b > *pk { *b } else { (*pk - dt * 0.45).max(0.0) };
        }
        let tail = &samples[samples.len().saturating_sub(512)..];
        self.viz.wave = if self.player.is_playing() { tail.iter().step_by(2).copied().collect() } else { Vec::new() };
    }

    fn update_anim(&mut self, dt: f32) {
        let loaded = self.player.is_loaded();
        let playing = self.player.is_playing();
        let a = &mut self.anim;
        let approach = |v: &mut f32, target: f32, rate: f32| *v += (target - *v) * (1.0 - (-dt * rate).exp());
        let toward = |v: &mut f32, target: f32, step: f32| {
            *v = if *v < target { (*v + step).min(target) } else { (*v - step).max(target) }
        };

        let spin = if playing { 1.0 } else { 0.0 };
        approach(&mut a.platter_speed, spin, 2.2); // a heavy platter takes a moment
        a.platter = (a.platter + a.platter_speed * 3.49 * dt) % std::f32::consts::TAU; // 33⅓ rpm
        toward(&mut a.arm, if loaded { 1.0 } else { 0.0 }, dt * 0.9);
        approach(&mut a.arm_lift, if playing && a.arm >= 1.0 { 0.0 } else { 1.0 }, 5.0);

        approach(&mut a.cd_speed, spin, 3.5);
        a.cd = (a.cd + a.cd_speed * 8.0 * dt) % std::f32::consts::TAU;
        approach(&mut a.cd_lid, if loaded { 0.0 } else { 1.0 }, 4.0);

        approach(&mut a.tape_speed, spin, 9.0);
        let dur = self.current.and_then(|i| self.playlist.get(i)).and_then(|p| self.library.get(p)).and_then(|m| m.duration).unwrap_or(0.0);
        let progress = if dur > 0.0 { (self.player.position() / dur).clamp(0.0, 1.0) as f32 } else { 0.0 };
        let (l, r) = skins::cassette::pack_radii(progress);
        a.reel_l += a.tape_speed * 1.6 / l * dt;
        a.reel_r += a.tape_speed * 1.6 / r * dt;
    }

    fn keyboard(&mut self, ctx: &egui::Context) {
        if ctx.wants_keyboard_input() {
            return;
        }
        let mut acts = Vec::new();
        ctx.input(|i| {
            if i.key_pressed(Key::Space) {
                acts.push(Action::Toggle);
            }
            if i.key_pressed(Key::ArrowRight) {
                acts.push(Action::Seek(self.player.position() + 5.0));
            }
            if i.key_pressed(Key::ArrowLeft) {
                acts.push(Action::Seek((self.player.position() - 5.0).max(0.0)));
            }
            if i.key_pressed(Key::ArrowUp) {
                acts.push(Action::Volume(self.player.volume() + 0.05));
            }
            if i.key_pressed(Key::ArrowDown) {
                acts.push(Action::Volume(self.player.volume() - 0.05));
            }
            if i.key_pressed(Key::N) {
                acts.push(Action::Next);
            }
            if i.key_pressed(Key::B) {
                acts.push(Action::Prev);
            }
            if i.modifiers.command && i.key_pressed(Key::O) {
                acts.push(Action::Open);
            }
        });
        for a in acts {
            self.act(a, ctx);
        }
        if ctx.input(|i| i.key_pressed(Key::Delete)) {
            if let Some(s) = self.selected {
                self.remove(s);
            }
        }
        if ctx.input(|i| i.key_pressed(Key::Enter)) {
            if let Some(s) = self.selected {
                self.play_index(s, ctx);
            }
        }
    }

    fn now_playing(&self) -> NowPlaying<'_> {
        NowPlaying {
            meta: self.current.and_then(|i| self.meta_of(i)),
            index: self.current,
            count: self.playlist.len(),
            pos: self.player.position(),
            dur: self.duration(),
            loaded: self.player.is_loaded(),
            playing: self.player.is_playing(),
            volume: self.player.volume(),
            shuffle: self.shuffle,
            repeat: self.repeat,
            cover: self.cover.as_ref().map(|(t, _)| t),
            lyrics: self.lyrics.as_ref().map(|(l, _)| l),
            bars: &self.viz.bars,
            peaks: &self.viz.peaks,
            wave: &self.viz.wave,
            info: self.player.info,
            time: self.time(),
            anim: &self.anim,
        }
    }

    // ----------------------------------------------------------------------------- panels

    fn header(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.add_space(6.0);
            let (r, _) = ui.allocate_exact_size(Vec2::new(150.0, 52.0), Sense::hover());
            let p = ui.painter();
            p.text(r.left_center() + Vec2::new(3.0, 3.0), Align2::LEFT_CENTER, "JAMP", bold(48.0), MAGENTA);
            glow_text(p, r.left_center(), Align2::LEFT_CENTER, "JAMP", bold(48.0), MAGENTA);
            p.text(r.left_center(), Align2::LEFT_CENTER, "JAMP", bold(48.0), YELLOW);
            ui.vertical(|ui| {
                ui.add_space(10.0);
                ui.label(RichText::new("Just Another").color(TEAL));
                ui.label(RichText::new("Music Player").color(TEAL));
            });
            ui.add_space(18.0);

            // skins as desktop icons, like the site's header
            for skin in Skin::ALL {
                if desktop_icon(ui, skin, self.skin == skin).clicked() {
                    self.skin = skin;
                }
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(8.0);
                let online = ui.selectable_label(self.online, if self.online { "Online: on" } else { "Online: off" })
                    .on_hover_text("Fetch missing lyrics (LRCLIB) and covers (iTunes / Cover Art Archive)\nonly when the file has none embedded.");
                if online.clicked() {
                    self.online = !self.online;
                }
                if ui.add(egui::Button::new("Settings").selected(self.show_settings)).clicked() {
                    self.show_settings = !self.show_settings;
                }
                if ui.button("Add folder").clicked() {
                    self.open_folder();
                }
                if ui.button("Open files").clicked() {
                    self.open_files();
                }
            });
        });
    }

    fn settings_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_settings;
        egui::Window::new(RichText::new("Settings").font(bold(18.0)))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(400.0)
            .pivot(Align2::CENTER_CENTER)
            .default_pos(ctx.screen_rect().center())
            .show(ctx, |ui| {
                let heading = |ui: &mut Ui, text: &str| {
                    ui.add_space(6.0);
                    ui.label(RichText::new(text).font(bold(16.0)).color(TEAL));
                    ui.separator();
                };

                heading(ui, "Look");
                if ui.checkbox(&mut self.glow, "Neon glow (HDR-style bloom)").changed() {
                    set_glow(self.glow);
                }

                heading(ui, "Lyrics & covers");
                ui.checkbox(&mut self.online, "Look up missing lyrics and covers online")
                    .on_hover_text("Only when the file has none embedded and there's no .lrc / cover.jpg beside it.");
                ui.horizontal(|ui| {
                    if ui.button("Clear cache").clicked() {
                        online::clear_cache();
                        self.set_status("Cache cleared");
                    }
                    if ui.button("Re-fetch current track").clicked() {
                        online::clear_cache();
                        if let Some(c) = self.current {
                            let paused = self.player.is_paused();
                            let pos = self.player.position();
                            self.load_index(c, paused, ctx);
                            self.player.seek(pos);
                        }
                    }
                });

                heading(ui, "Discord");
                let mut changed = ui.checkbox(&mut self.discord, "Show what I'm playing on Discord").changed();
                ui.horizontal(|ui| {
                    ui.label("Application ID");
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut self.discord_client_id)
                            .hint_text("e.g. 1234567890123456789")
                            .desired_width(200.0),
                    );
                    if r.changed() {
                        self.discord_client_id.retain(|c| c.is_ascii_digit());
                        changed = true;
                    }
                });
                if changed {
                    self.last_presence = None;
                    self.presence.configure(self.discord, &self.discord_client_id);
                }
                ui.label(RichText::new(self.presence.status()).small().color(if self.discord { YELLOW } else { DIM }));
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    ui.label(RichText::new("Create an application at ").small().color(DIM));
                    ui.hyperlink_to(RichText::new("discord.com/developers").small(), "https://discord.com/developers/applications");
                    ui.label(RichText::new(" and paste its Application ID. The Discord desktop app must be running.").small().color(DIM));
                });

                heading(ui, "Playlist");
                if ui.button("Clear playlist").clicked() {
                    self.clear();
                }
                let ff = if audio::ffmpeg_available() { "ffmpeg: found (extra formats on)" } else { "ffmpeg: not found (native formats only)" };
                ui.label(RichText::new(ff).small().color(DIM));
            });
        self.show_settings = open;
    }

    /// Pushes "now playing" to Discord, but only when something visible changed: the start
    /// time is recomputed every frame and wobbles slightly, which must not count as an update.
    fn sync_presence(&mut self) {
        if !self.discord {
            return;
        }
        let activity = self.current.filter(|_| self.player.is_loaded()).and_then(|i| self.meta_of(i)).map(|m| {
            let paused = self.player.is_paused();
            let dur_ms = (m.duration.unwrap_or(0.0) * 1000.0) as u64;
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            let start = now_ms.saturating_sub((self.player.position() * 1000.0) as u64);
            discord::Activity {
                title: m.title.clone(),
                artist: m.artist.clone(),
                album: m.album.clone(),
                cover_url: self.cover_url.clone(),
                span: (!paused && dur_ms > 0).then_some((start, start + dur_ms)),
                paused,
            }
        });
        let same = match (&activity, &self.last_presence) {
            (None, None) => true,
            (Some(a), Some(b)) => {
                let spans_close = match (a.span, b.span) {
                    (Some((x, _)), Some((y, _))) => x.abs_diff(y) < 2000,
                    (None, None) => true,
                    _ => false,
                };
                spans_close
                    && a.title == b.title
                    && a.artist == b.artist
                    && a.album == b.album
                    && a.cover_url == b.cover_url
                    && a.paused == b.paused
            }
            _ => false,
        };
        if !same {
            self.presence.set(activity.clone());
            self.last_presence = activity;
        }
    }

    fn taskbar(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        let mut acts = Vec::new();
        let np = self.now_playing();
        ui.horizontal_centered(|ui| {
            let s = Vec2::new(30.0, 26.0);
            for (icon, a) in [(Icon::Prev, Action::Prev), (if np.playing { Icon::Pause } else { Icon::Play }, Action::Toggle), (Icon::Stop, Action::Stop), (Icon::Next, Action::Next)] {
                if skins::icon_button(ui, s, icon, false).clicked() {
                    acts.push(a);
                }
            }
            if skins::icon_button(ui, s, Icon::Shuffle, np.shuffle).on_hover_text("Shuffle").clicked() {
                acts.push(Action::Shuffle);
            }
            let rep = if np.repeat == Repeat::One { Icon::RepeatOne } else { Icon::Repeat };
            if skins::icon_button(ui, s, rep, np.repeat != Repeat::Off).on_hover_text(format!("Repeat: {:?}", np.repeat)).clicked() {
                acts.push(Action::Repeat);
            }
            vline(ui);

            let time = format!("{} / {}", fmt_time(np.pos), fmt_time(np.dur));
            let tg = ui.painter().layout_no_wrap(time.clone(), pixel(16.0), INK);
            let right_w = 140.0 + 120.0 + tg.size().x + 40.0;
            let w = (ui.available_width() - right_w).max(120.0);
            let (rail, _) = ui.allocate_exact_size(Vec2::new(w, 26.0), Sense::hover());
            let r = Rect::from_center_size(rail.center(), Vec2::new(rail.width(), 8.0));
            ui.painter().rect_filled(r.expand(2.0), 2.0, INK);
            if let Some(f) = skins::seek_rail(ui, r, np.progress(), INK, YELLOW, WHITE) {
                acts.push(Action::Seek(f as f64 * np.dur));
            }
            ui.label(RichText::new(time).color(INK));
            vline(ui);
            let (vr, _) = ui.allocate_exact_size(Vec2::new(90.0, 26.0), Sense::hover());
            let vr = Rect::from_center_size(vr.center(), Vec2::new(vr.width(), 6.0));
            ui.painter().rect_filled(vr.expand(2.0), 2.0, INK);
            if let Some(v) = skins::seek_rail(ui, vr, np.volume, INK, TEAL, WHITE) {
                acts.push(Action::Volume(v));
            }
            ui.label(RichText::new(format!("{:>3}%", (np.volume * 100.0).round())).color(INK));

            // "site-date" box on the far right
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let total: f64 = self.playlist.iter().filter_map(|p| self.library.get(p)?.duration).sum();
                let text = format!("{} tracks · {}", self.playlist.len(), fmt_time(total));
                egui::Frame::new().fill(INK).stroke(Stroke::new(2.0f32, YELLOW)).inner_margin(egui::Margin::symmetric(6, 2)).show(ui, |ui| {
                    ui.label(RichText::new(text).color(YELLOW));
                });
            });
        });
        // vol/seek use the live value while dragging; re-apply volume continuously
        for a in acts {
            self.act(a, ctx);
        }
    }

    fn playlist_view(&mut self, ui: &mut Ui, ctx: &egui::Context, compact: bool) {
        ui.horizontal(|ui| {
            let r = ui.add(egui::TextEdit::singleline(&mut self.filter).hint_text("Search…").desired_width(ui.available_width() - 70.0));
            if r.changed() {
                self.filter_dirty = true;
            }
            if ui.button("+ Add").clicked() {
                self.open_files();
            }
        });
        if self.filter_dirty {
            let q = self.filter.to_lowercase();
            self.filtered = (0..self.playlist.len())
                .filter(|&i| {
                    q.is_empty()
                        || self.meta_of(i).is_some_and(|m| {
                            m.title.to_lowercase().contains(&q) || m.artist.to_lowercase().contains(&q) || m.album.to_lowercase().contains(&q)
                        })
                })
                .collect();
            self.filter_dirty = false;
        }

        if self.playlist.is_empty() {
            ui.add_space(20.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("Drop music files or folders here").color(TEAL));
                ui.label(RichText::new("or use Open files / Add folder").small().color(DIM));
            });
            return;
        }

        let width = ui.available_width();
        let cols: &[(&str, f32)] = if compact {
            &[("#", 0.1), ("Title", 0.72), ("Time", 0.18)]
        } else {
            &[("#", 0.06), ("Title", 0.34), ("Artist", 0.24), ("Album", 0.26), ("Codec", 0.1)]
        };
        let (hdr, _) = ui.allocate_exact_size(Vec2::new(width, ROW_H), Sense::hover());
        ui.painter().rect_filled(hdr, 0.0, PURPLE);
        let mut x = hdr.left();
        for (name, frac) in cols {
            ui.painter().text(Pos2::new(x + 4.0, hdr.center().y), Align2::LEFT_CENTER, *name, bold(14.0), YELLOW);
            x += frac * width;
        }

        let rows = self.filtered.clone();
        let mut sa = egui::ScrollArea::vertical().auto_shrink([false, false]);
        if self.scroll_to_current {
            if let Some(pos) = self.current.and_then(|c| rows.iter().position(|r| *r == c)) {
                let spacing = ui.spacing().item_spacing.y;
                sa = sa.vertical_scroll_offset((pos as f32 * (ROW_H + spacing) - 80.0).max(0.0));
            }
            self.scroll_to_current = false;
        }
        let mut acts = Vec::new();
        let mut remove = None;
        sa.show_rows(ui, ROW_H, rows.len(), |ui, range| {
            for vis in range {
                let idx = rows[vis];
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, ROW_H), Sense::click());
                let p = ui.painter();
                let playing = self.current == Some(idx);
                let bg = if self.selected == Some(idx) {
                    INDIGO
                } else if resp.hovered() {
                    Color32::from_rgb(0x2a, 0x22, 0x50)
                } else if vis % 2 == 1 {
                    Color32::from_rgb(0x22, 0x20, 0x3a)
                } else {
                    Color32::TRANSPARENT
                };
                p.rect_filled(rect, 0.0, bg);
                if playing {
                    p.rect_stroke(rect.shrink(0.5), 0.0, Stroke::new(1.0f32, MAGENTA), StrokeKind::Inside);
                    glow_rect(p, rect, 0.0, MAGENTA, 4.0);
                }
                let fg = if playing { MAGENTA } else { YELLOW };
                let m = self.meta_of(idx).cloned().unwrap_or_default();
                let num = (idx + 1).to_string();
                let time = m.duration.map(fmt_time).unwrap_or_default();
                let cells: Vec<String> = if compact {
                    vec![num, m.display_title(), time]
                } else {
                    vec![num, m.title.clone(), m.artist.clone(), m.album.clone(), format!("{} {}", m.codec, time)]
                };
                let mut x = rect.left();
                for (i, ((_, frac), text)) in cols.iter().zip(cells).enumerate() {
                    let cw = frac * width;
                    let cell = Rect::from_min_size(Pos2::new(x + 4.0, rect.top()), Vec2::new(cw - 8.0, ROW_H));
                    if i == 0 && playing {
                        skins::paint_icon(p, Rect::from_center_size(cell.left_center() + Vec2::new(6.0, 0.0), Vec2::splat(14.0)), if self.player.is_playing() { Icon::Play } else { Icon::Pause }, MAGENTA);
                    } else {
                        let col = if i >= 2 && !playing { TEAL.gamma_multiply(0.9) } else { fg };
                        let align = if compact && i == 2 { Align2::RIGHT_CENTER } else { Align2::LEFT_CENTER };
                        let at = if align == Align2::RIGHT_CENTER { cell.right_center() } else { cell.left_center() };
                        p.with_clip_rect(cell).text(at, align, visual(&text), pixel(16.0), col);
                    }
                    x += cw;
                }
                if resp.clicked() {
                    self.selected = Some(idx);
                }
                if resp.double_clicked() {
                    acts.push(Action::PlayIndex(idx));
                }
                resp.context_menu(|ui| {
                    if ui.button("Play").clicked() {
                        acts.push(Action::PlayIndex(idx));
                        ui.close_menu();
                    }
                    if ui.button("Remove").clicked() {
                        remove = Some(idx);
                        ui.close_menu();
                    }
                    if ui.button("Show in folder").clicked() {
                        if let Some(dir) = self.playlist[idx].parent() {
                            let _ = open_in_file_manager(dir);
                        }
                        ui.close_menu();
                    }
                });
            }
        });
        if let Some(r) = remove {
            self.remove(r);
        }
        for a in acts {
            self.act(a, ctx);
        }
    }

    fn lyrics_view(&mut self, ui: &mut Ui) {
        let pos = self.player.position();
        match &self.lyrics {
            None => {
                ui.add_space(8.0);
                ui.vertical_centered(|ui| {
                    let msg = if self.extras_loading {
                        "Looking for lyrics…"
                    } else if self.current.is_none() {
                        "Nothing playing"
                    } else if self.online {
                        "No lyrics found"
                    } else {
                        "No embedded lyrics (online lookup is off)"
                    };
                    ui.label(RichText::new(msg).color(DIM));
                });
            }
            Some((lyr, origin)) => {
                let src = match origin {
                    Origin::Online => "LRCLIB",
                    o => o.label(),
                };
                ui.label(RichText::new(format!("{} · {}", if lyr.synced { "synced" } else { "plain" }, src)).small().color(DIM));
                ui.separator();
                let cur = lyr.current_line(pos);
                let changed = cur != self.last_lyric_line;
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    ui.vertical_centered(|ui| {
                        for (i, (_, line)) in lyr.lines.iter().enumerate() {
                            let text = if line.is_empty() { "~".to_string() } else { visual(line).into_owned() };
                            let rt = if Some(i) == cur {
                                RichText::new(text).font(bold(18.0)).color(TEAL)
                            } else if lyr.synced && cur.is_some_and(|c| i < c) {
                                RichText::new(text).color(DIM)
                            } else {
                                RichText::new(text).color(YELLOW)
                            };
                            let r = ui.add(egui::Label::new(rt).wrap());
                            if Some(i) == cur && changed {
                                r.scroll_to_me(Some(egui::Align::Center));
                            }
                        }
                        ui.add_space(40.0);
                    });
                });
                self.last_lyric_line = cur;
            }
        }
    }

    fn take_screenshots(&mut self, ctx: &egui::Context) {
        let now = self.time();
        let Some(plan) = &mut self.screenshot else { return };
        let mut done_one = None;
        ctx.input(|i| {
            for e in &i.raw.events {
                if let egui::Event::Screenshot { image, .. } = e {
                    done_one = Some(image.clone());
                }
            }
        });
        // egui may run a frame twice with the same input (multi-pass), so the same reply can show
        // up again: only take it while a capture is actually pending.
        if let (Some(img), true, false) = (done_one, plan.waiting, plan.queue.is_empty()) {
            let skin = plan.queue.remove(0);
            let path = plan.dir.join(format!("{}.png", skin.label().to_lowercase()));
            let _ = std::fs::create_dir_all(&plan.dir);
            let _ = image::save_buffer(&path, img.as_raw(), img.width() as u32, img.height() as u32, image::ColorType::Rgba8);
            eprintln!("saved {}", path.display());
            plan.waiting = false;
            plan.next_at = now + 1.5;
            match plan.queue.first() {
                Some(s) => self.skin = *s,
                None => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            }
            return;
        }
        if !plan.waiting {
            if let Some(s) = plan.queue.first() {
                self.skin = *s;
            }
            if now >= plan.next_at {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
                plan.waiting = true;
            }
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let now = self.time();
        let dt = ((now - self.last_frame) as f32).clamp(0.0, 0.1);
        self.last_frame = now;

        self.pump(ctx);
        self.update_viz(dt);
        self.update_anim(dt);
        self.keyboard(ctx);

        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        if !dropped.is_empty() {
            let was_empty = self.playlist.is_empty();
            let start = self.playlist.len();
            self.add_paths(dropped);
            if (was_empty || !self.player.is_loaded()) && self.playlist.len() > start {
                self.play_index(start, ctx);
            }
        }

        // the whole window is the night sky
        paint_night_sky(&ctx.layer_painter(egui::LayerId::background()), ctx.screen_rect(), now);

        egui::TopBottomPanel::top("header").frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(8, 6))).show(ctx, |ui| self.header(ui));

        egui::TopBottomPanel::bottom("taskbar")
            .exact_height(40.0)
            .frame(egui::Frame::new().fill(MAGENTA).inner_margin(egui::Margin::symmetric(6, 4)))
            .show(ctx, |ui| {
                let r = ui.max_rect().expand2(Vec2::new(6.0, 4.0));
                ui.painter().line_segment([r.left_top(), r.right_top()], Stroke::new(2.0f32, WHITE));
                self.taskbar(ui, ctx);
            });

        let pad = egui::Frame::new().inner_margin(egui::Margin::same(10));
        let mut acts = Vec::new();
        match self.skin {
            Skin::Modern => {
                egui::SidePanel::left("np").resizable(true).default_width(300.0).width_range(220.0..=480.0).frame(pad).show(ctx, |ui| {
                    let src = self.cover.as_ref().map(|(_, o)| o.label());
                    window(ui, "Now Playing", |ui| skins::modern::now_playing(ui, &self.now_playing(), src));
                    ui.add_space(12.0);
                    window(ui, "Lyrics", |ui| self.lyrics_view(ui));
                });
                egui::CentralPanel::default().frame(pad).show(ctx, |ui| {
                    window(ui, "Playlist", |ui| self.playlist_view(ui, ctx, false));
                });
            }
            Skin::Amp => {
                egui::SidePanel::right("lyrics").resizable(true).default_width(320.0).frame(pad).show(ctx, |ui| {
                    window(ui, "Lyrics", |ui| self.lyrics_view(ui));
                });
                egui::CentralPanel::default().frame(pad).show(ctx, |ui| {
                    let avail = ui.available_rect_before_wrap();
                    let scale = (avail.width() / skins::amp::BASE.x).min(avail.height() * 0.45 / skins::amp::BASE.y).max(1.0);
                    let size = skins::amp::BASE * scale;
                    let rect = Rect::from_min_size(Pos2::new(avail.center().x - size.x / 2.0, avail.top()), size);
                    let np = self.now_playing();
                    acts = skins::amp::draw(ui, rect, &np);
                    ui.add_space(14.0);
                    let below = Rect::from_min_max(Pos2::new(rect.left(), rect.bottom() + 14.0), Pos2::new(rect.right(), avail.bottom()));
                    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(below));
                    window(&mut child, "Playlist Editor", |ui| self.playlist_view(ui, ctx, true));
                });
            }
            device => {
                egui::SidePanel::right("side").resizable(true).default_width(360.0).width_range(260.0..=560.0).frame(pad).show(ctx, |ui| {
                    let title = if self.right_tab_lyrics { "Lyrics" } else { "Playlist" };
                    window(ui, title, |ui| {
                        ui.horizontal(|ui| {
                            ui.selectable_value(&mut self.right_tab_lyrics, false, "Playlist");
                            ui.selectable_value(&mut self.right_tab_lyrics, true, "Lyrics");
                        });
                        ui.separator();
                        if self.right_tab_lyrics {
                            self.lyrics_view(ui);
                        } else {
                            self.playlist_view(ui, ctx, true);
                        }
                    });
                });
                egui::CentralPanel::default().frame(pad).show(ctx, |ui| {
                    let name = match device {
                        Skin::Cd => "CD Player",
                        Skin::Mp3 => "MP3 Player",
                        Skin::Vinyl => "Turntable",
                        _ => "Cassette Deck",
                    };
                    window(ui, name, |ui| {
                        let np = self.now_playing();
                        let line = np.current_lyric().map(str::to_string);
                        let h = ui.available_height() - if line.is_some() { 30.0 } else { 0.0 };
                        let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), h.max(200.0)), Sense::hover());
                        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
                        acts = match device {
                            Skin::Cd => skins::cd::draw(&mut child, &np),
                            Skin::Mp3 => skins::mp3::draw(&mut child, &np),
                            Skin::Vinyl => skins::vinyl::draw(&mut child, &np),
                            _ => skins::cassette::draw(&mut child, &np),
                        };
                        if let Some(l) = line {
                            ui.vertical_centered(|ui| ui.label(RichText::new(l).font(bold(18.0)).color(TEAL)));
                        }
                    });
                });
            }
        }
        for a in acts {
            self.act(a, ctx);
        }

        if let Some((msg, until)) = &self.status {
            if now < *until {
                egui::Area::new("status".into()).anchor(Align2::CENTER_BOTTOM, Vec2::new(0.0, -52.0)).show(ctx, |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| ui.label(RichText::new(msg).color(YELLOW)));
                });
            } else {
                self.status = None;
            }
        }

        self.settings_window(ctx);
        self.sync_presence();
        self.take_screenshots(ctx);

        // Smooth animation while something moves; a slow tick for the drifting stars otherwise.
        let a = &self.anim;
        let moving = self.player.is_playing() || a.platter_speed > 0.01 || a.cd_speed > 0.01 || a.tape_speed > 0.01 || (a.arm > 0.0 && a.arm < 1.0) || (a.cd_lid > 0.01 && a.cd_lid < 0.99) || self.viz.peaks.iter().any(|p| *p > 0.0);
        if moving || self.screenshot.is_some() {
            ctx.request_repaint();
        } else {
            ctx.request_repaint_after(std::time::Duration::from_millis(66));
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        if !self.persist {
            return;
        }
        let saved = Saved {
            playlist: self.playlist.clone(),
            current: self.current,
            skin: self.skin,
            volume: self.player.volume(),
            shuffle: self.shuffle,
            repeat: self.repeat,
            online: self.online,
            glow: self.glow,
            discord: self.discord,
            discord_client_id: self.discord_client_id.clone(),
        };
        eframe::set_value(storage, "jamp", &saved);
    }
}

/// Expands folders (recursively, sorted) and .m3u playlists into audio files.
fn collect(p: &Path, out: &mut Vec<PathBuf>) {
    if p.is_dir() {
        let Ok(rd) = std::fs::read_dir(p) else { return };
        let mut entries: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
        entries.sort_by_key(|e| e.to_string_lossy().to_lowercase());
        for e in entries {
            collect(&e, out);
        }
        return;
    }
    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    if ext == "m3u" || ext == "m3u8" {
        if let Ok(text) = std::fs::read_to_string(p) {
            let base = p.parent().unwrap_or(Path::new("."));
            for line in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
                let entry = Path::new(line);
                let full = if entry.is_absolute() { entry.to_path_buf() } else { base.join(entry) };
                if audio::is_audio_file(&full) && full.exists() {
                    out.push(full);
                }
            }
        }
    } else if audio::is_audio_file(p) {
        out.push(p.to_path_buf());
    }
}

fn open_in_file_manager(dir: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    let prog = "explorer";
    #[cfg(target_os = "macos")]
    let prog = "open";
    #[cfg(all(unix, not(target_os = "macos")))]
    let prog = "xdg-open";
    std::process::Command::new(prog).arg(dir).spawn().map(|_| ())
}

fn vline(ui: &mut Ui) {
    let (r, _) = ui.allocate_exact_size(Vec2::new(4.0, 28.0), Sense::hover());
    ui.painter().rect_filled(Rect::from_center_size(r.center(), Vec2::new(2.0, 26.0)), 0.0, YELLOW);
    ui.painter().rect_filled(Rect::from_center_size(r.center() + Vec2::new(2.0, 0.0), Vec2::new(1.0, 26.0)), 0.0, WHITE);
}

/// A pixel icon with a label under it, like the site's desktop shortcuts.
fn desktop_icon(ui: &mut Ui, skin: Skin, active: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(58.0, 56.0), Sense::click());
    let p = ui.painter();
    if active || resp.hovered() {
        p.rect_filled(rect, 3.0, if active { INDIGO } else { white_a(10) });
        p.rect_stroke(rect, 3.0, Stroke::new(1.0f32, if active { YELLOW } else { TEAL }), StrokeKind::Inside);
    }
    let ic = Rect::from_center_size(rect.center_top() + Vec2::new(0.0, 19.0), Vec2::splat(30.0));
    let c = ic.center();
    let st = Stroke::new(1.5f32, INK);
    match skin {
        Skin::Modern => {
            p.rect_filled(ic, 2.0, WHITE);
            p.rect_filled(Rect::from_min_size(ic.min, Vec2::new(30.0, 7.0)), 2.0, TEAL);
            for i in 0..3 {
                let y = ic.top() + 12.0 + i as f32 * 6.0;
                p.line_segment([Pos2::new(ic.left() + 4.0, y), Pos2::new(ic.right() - 4.0, y)], Stroke::new(2.0f32, if i == 1 { MAGENTA } else { INK }));
            }
        }
        Skin::Amp => {
            let r = Rect::from_center_size(c, Vec2::new(30.0, 18.0));
            p.rect_filled(r, 1.0, Color32::from_rgb(0x52, 0x40, 0x92));
            p.rect_filled(Rect::from_min_size(r.min + Vec2::new(3.0, 3.0), Vec2::new(12.0, 8.0)), 0.0, Color32::BLACK);
            for (i, h) in [4.0, 7.0, 5.0, 8.0].iter().enumerate() {
                p.rect_filled(Rect::from_min_size(Pos2::new(r.left() + 4.0 + i as f32 * 2.7, r.top() + 11.0 - h), Vec2::new(2.0, *h)), 0.0, TEAL);
            }
            p.rect_filled(Rect::from_min_size(r.min + Vec2::new(17.0, 4.0), Vec2::new(10.0, 3.0)), 0.0, YELLOW);
            p.rect_stroke(r, 1.0, Stroke::new(1.0f32, YELLOW), StrokeKind::Inside);
        }
        Skin::Cd => {
            p.circle_filled(c, 14.0, Color32::from_gray(210));
            p.circle_filled(c, 11.0, TEAL.gamma_multiply(0.7));
            p.circle_filled(c, 7.0, MAGENTA.gamma_multiply(0.6));
            p.circle_filled(c, 3.5, Color32::from_gray(240));
            p.circle_stroke(c, 14.0, st);
        }
        Skin::Mp3 => {
            let r = Rect::from_center_size(c, Vec2::new(19.0, 29.0));
            p.rect_filled(r, 3.0, WHITE);
            p.rect_filled(Rect::from_min_size(r.min + Vec2::new(3.0, 3.0), Vec2::new(13.0, 10.0)), 1.0, TEAL);
            p.circle_stroke(r.center() + Vec2::new(0.0, 6.5), 5.0, st);
            p.circle_filled(r.center() + Vec2::new(0.0, 6.5), 1.8, INK);
        }
        Skin::Vinyl => {
            p.circle_filled(c, 14.0, Color32::from_gray(20));
            for k in [11.0, 8.5] {
                p.circle_stroke(c, k, Stroke::new(0.8f32, Color32::from_gray(80)));
            }
            p.circle_filled(c, 5.0, MAGENTA);
            p.circle_filled(c, 1.2, WHITE);
            p.line_segment([c + Vec2::new(13.0, -13.0), c + Vec2::new(4.0, 6.0)], Stroke::new(2.0f32, Color32::from_gray(200)));
        }
        Skin::Cassette => {
            let r = Rect::from_center_size(c, Vec2::new(30.0, 19.0));
            p.rect_filled(r, 2.0, WHITE);
            p.rect_filled(Rect::from_min_size(r.min + Vec2::new(3.0, 3.0), Vec2::new(24.0, 9.0)), 1.0, INK);
            for dx in [-6.0, 6.0] {
                p.circle_filled(Pos2::new(c.x + dx, r.top() + 7.5), 3.0, WHITE);
            }
            p.line_segment([Pos2::new(r.left() + 7.0, r.bottom() - 2.0), Pos2::new(r.right() - 7.0, r.bottom() - 2.0)], st);
        }
    }
    let fg = if active { YELLOW } else if resp.hovered() { TEAL } else { WHITE };
    p.text(rect.center_bottom() - Vec2::new(0.0, 3.0), Align2::CENTER_BOTTOM, skin.label(), pixel(14.0), fg);
    resp.on_hover_text(format!("{} skin", skin.label()))
}
