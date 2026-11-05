//! Tag reading (lofty) plus local cover/lyrics lookups.

use std::path::Path;

use lofty::file::{AudioFile, TaggedFileExt};
use lofty::picture::PictureType;
use lofty::tag::{Accessor, ItemKey, Tag};

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct TrackMeta {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub year: Option<u32>,
    pub track_no: Option<u32>,
    pub duration: Option<f64>,
    pub bitrate_kbps: Option<u32>,
    pub sample_rate: Option<u32>,
    pub channels: Option<u8>,
    pub codec: String,
    /// Did lofty actually find tags, or are title/artist guessed from the file name?
    pub tagged: bool,
}

impl TrackMeta {
    pub fn display_title(&self) -> String {
        if self.artist.is_empty() {
            self.title.clone()
        } else {
            format!("{} - {}", self.artist, self.title)
        }
    }
}

fn guess_from_filename(path: &Path) -> (String, String) {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("Unknown").to_string();
    // "01 - Artist - Title", "Artist - Title", or just "Title"
    let parts: Vec<&str> = stem.split(" - ").map(str::trim).collect();
    let parts: Vec<&str> = match parts.first() {
        Some(p) if parts.len() > 1 && p.chars().all(|c| c.is_ascii_digit()) => parts[1..].to_vec(),
        _ => parts,
    };
    match parts.as_slice() {
        [artist, title, ..] => (artist.to_string(), title.to_string()),
        [title] => (String::new(), title.to_string()),
        [] => (String::new(), stem),
    }
}

fn best_tag(file: &lofty::file::TaggedFile) -> Option<&Tag> {
    file.primary_tag().or_else(|| file.first_tag())
}

pub fn read_meta(path: &Path) -> TrackMeta {
    let codec = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_uppercase())
        .unwrap_or_default();
    let (guess_artist, guess_title) = guess_from_filename(path);
    let mut meta = TrackMeta { title: guess_title, artist: guess_artist, codec, ..Default::default() };

    let Ok(file) = lofty::read_from_path(path) else {
        if crate::audio::ffmpeg_available() {
            meta.duration = crate::audio::ffprobe_duration(path);
        }
        return meta;
    };

    let props = file.properties();
    let dur = props.duration().as_secs_f64();
    meta.duration = (dur > 0.0).then_some(dur);
    meta.bitrate_kbps = props.audio_bitrate().or(props.overall_bitrate());
    meta.sample_rate = props.sample_rate();
    meta.channels = props.channels();

    if let Some(tag) = best_tag(&file) {
        if let Some(t) = tag.title().filter(|t| !t.trim().is_empty()) {
            meta.title = t.trim().to_string();
            meta.tagged = true;
        }
        if let Some(a) = tag.artist().filter(|a| !a.trim().is_empty()) {
            meta.artist = a.trim().to_string();
            meta.tagged = true;
        }
        if let Some(al) = tag.album() {
            meta.album = al.trim().to_string();
        }
        meta.year = tag.year();
        meta.track_no = tag.track();
    }
    meta
}

/// Cover art embedded in the file itself (front cover preferred).
pub fn embedded_cover(path: &Path) -> Option<Vec<u8>> {
    let file = lofty::read_from_path(path).ok()?;
    let mut fallback = None;
    for tag in file.tags() {
        for pic in tag.pictures() {
            if pic.pic_type() == PictureType::CoverFront {
                return Some(pic.data().to_vec());
            }
            if fallback.is_none() {
                fallback = Some(pic.data().to_vec());
            }
        }
    }
    fallback
}

/// Lyrics embedded in the file's tags (USLT, LYRICS, ©lyr, ...).
pub fn embedded_lyrics(path: &Path) -> Option<String> {
    let file = lofty::read_from_path(path).ok()?;
    file.tags()
        .iter()
        .find_map(|t| t.get_string(&ItemKey::Lyrics).map(str::to_string))
        .filter(|l| !l.trim().is_empty())
}

/// `song.lrc` / `song.txt` sitting next to `song.flac`.
pub fn sidecar_lyrics(path: &Path) -> Option<String> {
    ["lrc", "txt"]
        .iter()
        .map(|ext| path.with_extension(ext))
        .find_map(|p| std::fs::read_to_string(p).ok())
        .filter(|l| !l.trim().is_empty())
}

/// cover.jpg / folder.png / front.jpg / AlbumArt*.jpg in the track's folder.
pub fn folder_cover(path: &Path) -> Option<Vec<u8>> {
    let dir = path.parent()?;
    const NAMES: &[&str] = &["cover", "folder", "front", "album", "albumart", "albumartsmall"];
    let mut entries: Vec<_> = std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).collect();
    entries.sort();
    entries
        .iter()
        .filter(|p| {
            let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
            matches!(ext.as_str(), "jpg" | "jpeg" | "png" | "webp" | "bmp")
        })
        .find(|p| {
            let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase();
            NAMES.iter().any(|n| stem == *n || stem.starts_with("albumart_"))
        })
        .and_then(|p| std::fs::read(p).ok())
}
