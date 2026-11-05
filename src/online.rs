//! Cover + lyrics resolution. Local sources always win; the network is only touched when the
//! file has nothing embedded (and nothing sits next to it).
//!
//! Lyrics: LRCLIB (free, no key, synced LRC when available).
//! Covers: iTunes Search API, then MusicBrainz + Cover Art Archive.
//! Results (including misses) are cached on disk so each song is looked up once.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;

use crate::meta::{self, TrackMeta};

const USER_AGENT: &str = concat!(
    "JAMP/",
    env!("CARGO_PKG_VERSION"),
    " ( https://github.com/The1Azreen/Just-Another-Music-Player )"
);
const MISS: &[u8] = b"__none__";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    Embedded,
    Local,
    Cache,
    Online,
}

impl Origin {
    pub fn label(self) -> &'static str {
        match self {
            Origin::Embedded => "embedded",
            Origin::Local => "local file",
            Origin::Cache => "cached",
            Origin::Online => "online",
        }
    }
}

pub struct Extras {
    pub path: PathBuf,
    pub cover: Option<(egui::ColorImage, Origin)>,
    pub lyrics: Option<(String, Origin)>,
}

pub fn cache_dir() -> PathBuf {
    let dir = crate::portable_dir()
        .map(|d| d.join("cache"))
        .unwrap_or_else(|| dirs::cache_dir().unwrap_or_else(std::env::temp_dir).join("jamp"));
    let _ = std::fs::create_dir_all(dir.join("lyrics"));
    let _ = std::fs::create_dir_all(dir.join("covers"));
    dir
}

pub fn clear_cache() {
    let dir = cache_dir();
    let _ = std::fs::remove_dir_all(&dir);
    cache_dir();
}

/// FNV-1a, so cache file names stay stable across builds.
fn key(parts: &[&str]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for p in parts {
        for b in p.trim().to_lowercase().bytes().chain([0u8]) {
            h ^= b as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    }
    format!("{h:016x}")
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(5))
        .timeout(Duration::from_secs(12))
        .user_agent(USER_AGENT)
        .build()
}

fn decode_cover(bytes: &[u8]) -> Option<egui::ColorImage> {
    let img = image::load_from_memory(bytes).ok()?;
    // Covers can be 3000px; nothing here draws them larger than ~512.
    let img = if img.width() > 512 || img.height() > 512 {
        img.thumbnail(512, 512)
    } else {
        img
    };
    let rgba = img.to_rgba8();
    Some(egui::ColorImage::from_rgba_unmultiplied(
        [rgba.width() as usize, rgba.height() as usize],
        rgba.as_raw(),
    ))
}

/// Runs on a worker thread.
pub fn resolve(path: &Path, meta: &TrackMeta, allow_online: bool) -> Extras {
    let lyrics = meta::embedded_lyrics(path)
        .map(|l| (l, Origin::Embedded))
        .or_else(|| meta::sidecar_lyrics(path).map(|l| (l, Origin::Local)))
        .or_else(|| cached_or_fetch("lyrics", &lyrics_key(meta), allow_online, || fetch_lyrics(meta))
            .and_then(|(b, o)| String::from_utf8(b).ok().map(|s| (s, o))));

    let cover = meta::embedded_cover(path)
        .and_then(|b| decode_cover(&b))
        .map(|c| (c, Origin::Embedded))
        .or_else(|| meta::folder_cover(path).and_then(|b| decode_cover(&b)).map(|c| (c, Origin::Local)))
        .or_else(|| {
            cached_or_fetch("covers", &cover_key(meta), allow_online, || fetch_cover(meta))
                .and_then(|(b, o)| decode_cover(&b).map(|c| (c, o)))
        });

    Extras { path: path.to_path_buf(), cover, lyrics }
}

fn lyrics_key(meta: &TrackMeta) -> Option<String> {
    (!meta.artist.is_empty() && !meta.title.is_empty()).then(|| key(&[&meta.artist, &meta.title]))
}

fn cover_key(meta: &TrackMeta) -> Option<String> {
    if meta.artist.is_empty() {
        return None;
    }
    let what = if meta.album.is_empty() { &meta.title } else { &meta.album };
    (!what.is_empty()).then(|| key(&[&meta.artist, what]))
}

fn cached_or_fetch(
    kind: &str,
    key: &Option<String>,
    allow_online: bool,
    fetch: impl FnOnce() -> Option<Vec<u8>>,
) -> Option<(Vec<u8>, Origin)> {
    let key = key.as_ref()?;
    let file = cache_dir().join(kind).join(key);
    if let Ok(bytes) = std::fs::read(&file) {
        return (bytes != MISS).then_some((bytes, Origin::Cache));
    }
    if !allow_online {
        return None;
    }
    match fetch() {
        Some(bytes) => {
            let _ = std::fs::write(&file, &bytes);
            Some((bytes, Origin::Online))
        }
        None => {
            let _ = std::fs::write(&file, MISS);
            None
        }
    }
}

fn get_json(agent: &ureq::Agent, url: &str, query: &[(&str, &str)]) -> Option<Value> {
    let mut req = agent.get(url);
    for (k, v) in query {
        req = req.query(k, v);
    }
    req.call().ok()?.into_json().ok()
}

fn get_bytes(agent: &ureq::Agent, url: &str) -> Option<Vec<u8>> {
    let resp = agent.get(url).call().ok()?;
    let mut buf = Vec::new();
    resp.into_reader().take(20 * 1024 * 1024).read_to_end(&mut buf).ok()?;
    (!buf.is_empty()).then_some(buf)
}

fn lyrics_from(v: &Value) -> Option<String> {
    ["syncedLyrics", "plainLyrics"]
        .iter()
        .find_map(|k| v.get(*k).and_then(Value::as_str).filter(|s| !s.trim().is_empty()))
        .map(str::to_string)
}

fn fetch_lyrics(meta: &TrackMeta) -> Option<Vec<u8>> {
    let agent = agent();
    let dur = meta.duration.map(|d| format!("{}", d.round() as u64)).unwrap_or_default();
    let mut q = vec![("artist_name", meta.artist.as_str()), ("track_name", meta.title.as_str())];
    if !meta.album.is_empty() {
        q.push(("album_name", meta.album.as_str()));
    }
    if !dur.is_empty() {
        q.push(("duration", dur.as_str()));
    }
    if let Some(l) = get_json(&agent, "https://lrclib.net/api/get", &q).as_ref().and_then(lyrics_from) {
        return Some(l.into_bytes());
    }
    // Fuzzy fallback: album names and durations often disagree between releases.
    let results = get_json(
        &agent,
        "https://lrclib.net/api/search",
        &[("artist_name", &meta.artist), ("track_name", &meta.title)],
    )?;
    let arr = results.as_array()?;
    let best = arr
        .iter()
        .filter(|r| lyrics_from(r).is_some())
        .min_by_key(|r| {
            let d = r.get("duration").and_then(Value::as_f64).unwrap_or(0.0);
            let synced = r.get("syncedLyrics").and_then(Value::as_str).is_some();
            ((d - meta.duration.unwrap_or(d)).abs() as i64) * 2 + if synced { 0 } else { 1 }
        })?;
    lyrics_from(best).map(String::into_bytes)
}

fn fetch_cover(meta: &TrackMeta) -> Option<Vec<u8>> {
    let agent = agent();
    let (entity, term) = if meta.album.is_empty() {
        ("song", format!("{} {}", meta.artist, meta.title))
    } else {
        ("album", format!("{} {}", meta.artist, meta.album))
    };
    let itunes = get_json(
        &agent,
        "https://itunes.apple.com/search",
        &[("term", &term), ("entity", entity), ("limit", "5"), ("media", "music")],
    );
    let artist_lc = meta.artist.to_lowercase();
    if let Some(url) = itunes.as_ref().and_then(|v| v.get("results")?.as_array().cloned()).and_then(|rs| {
        rs.iter()
            .find(|r| {
                r.get("artistName")
                    .and_then(Value::as_str)
                    .is_some_and(|a| a.to_lowercase().contains(&artist_lc) || artist_lc.contains(&a.to_lowercase()))
            })
            .and_then(|r| r.get("artworkUrl100")?.as_str().map(str::to_string))
    }) {
        if let Some(b) = get_bytes(&agent, &url.replace("100x100bb", "600x600bb")) {
            return Some(b);
        }
    }

    let release = if meta.album.is_empty() { &meta.title } else { &meta.album };
    let mb = get_json(
        &agent,
        "https://musicbrainz.org/ws/2/release/",
        &[
            ("query", &format!("release:\"{}\" AND artist:\"{}\"", release.replace('"', ""), meta.artist.replace('"', ""))),
            ("fmt", "json"),
            ("limit", "3"),
        ],
    )?;
    for rel in mb.get("releases")?.as_array()? {
        let Some(id) = rel.get("id").and_then(Value::as_str) else { continue };
        if let Some(b) = get_bytes(&agent, &format!("https://coverartarchive.org/release/{id}/front-500")) {
            return Some(b);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song(artist: &str, title: &str, album: &str, dur: f64) -> TrackMeta {
        TrackMeta { artist: artist.into(), title: title.into(), album: album.into(), duration: Some(dur), ..Default::default() }
    }

    #[test]
    fn cache_keys_ignore_case_and_whitespace() {
        assert_eq!(key(&["Daft Punk ", "One More Time"]), key(&["daft punk", "one more time"]));
        assert_ne!(key(&["ab", "c"]), key(&["a", "bc"]));
    }

    /// Hits the real services: `cargo test -- --ignored`
    #[test]
    #[ignore]
    fn fetches_lyrics_and_cover_online() {
        let m = song("Daft Punk", "One More Time", "Discovery", 320.0);
        let lyr = fetch_lyrics(&m).expect("lyrics");
        assert!(String::from_utf8(lyr).unwrap().contains("One more time"));
        let cover = fetch_cover(&m).expect("cover");
        assert!(decode_cover(&cover).is_some());
    }
}
