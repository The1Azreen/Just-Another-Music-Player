//! Discord Rich Presence over Discord's local IPC pipe/socket. No SDK and no extra crates: the
//! protocol is a little-endian (opcode, length) header followed by JSON.
//!
//! Everything runs on one background thread: it connects when enabled, quietly retries while
//! Discord isn't running, and coalesces updates to stay under Discord's rate limit.

use std::io::{Read, Write};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

/// Shown when the cover has no public URL (embedded or local art).
const ICON_URL: &str = "https://raw.githubusercontent.com/The1Azreen/Just-Another-Music-Player/main/assets/icon.png";
/// Discord allows about 5 activity updates per 20 s.
const MIN_INTERVAL: Duration = Duration::from_secs(4);
const RETRY: Duration = Duration::from_secs(15);

#[derive(Clone, Debug, PartialEq)]
pub struct Activity {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub cover_url: Option<String>,
    /// Unix ms the track started / will end, while playing.
    pub span: Option<(u64, u64)>,
    pub paused: bool,
}

enum Msg {
    Config { enabled: bool, client_id: String },
    Activity(Option<Activity>),
}

pub struct Presence {
    tx: Sender<Msg>,
    status: Arc<Mutex<String>>,
}

impl Presence {
    pub fn new() -> Self {
        let (tx, rx) = channel();
        let status = Arc::new(Mutex::new("Off".to_string()));
        let st = status.clone();
        std::thread::spawn(move || {
            let set_status = |s: &str| {
                if let Ok(mut g) = st.lock() {
                    *g = s.to_string();
                }
            };
            let (mut enabled, mut client_id) = (false, String::new());
            let mut want: Option<Activity> = None;
            let mut conn: Option<Conn> = None;
            let mut dirty = false;
            let mut next_try = Instant::now();
            let mut last_sent: Option<Instant> = None;

            loop {
                match rx.recv_timeout(Duration::from_millis(500)) {
                    Ok(Msg::Config { enabled: e, client_id: id }) => {
                        if e != enabled || id != client_id {
                            if let Some(mut c) = conn.take() {
                                let _ = c.set_activity(None); // don't leave a stale status behind
                            }
                            enabled = e;
                            client_id = id;
                            dirty = true;
                            next_try = Instant::now();
                        }
                    }
                    Ok(Msg::Activity(a)) => {
                        if a != want {
                            want = a;
                            dirty = true;
                        }
                    }
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => break,
                }

                if !enabled {
                    set_status("Off");
                    continue;
                }
                if client_id.trim().is_empty() {
                    set_status("Paste your Application ID to connect");
                    continue;
                }
                if conn.is_none() {
                    if Instant::now() < next_try {
                        continue;
                    }
                    match Conn::open(client_id.trim()) {
                        Ok((c, user)) => {
                            set_status(&format!("Connected as {user}"));
                            conn = Some(c);
                            dirty = true;
                        }
                        Err(e) => {
                            set_status(&e);
                            next_try = Instant::now() + RETRY;
                            continue;
                        }
                    }
                }
                if dirty && last_sent.is_none_or(|t| t.elapsed() >= MIN_INTERVAL) {
                    let c = conn.as_mut().expect("connected above");
                    match c.set_activity(want.as_ref()) {
                        Ok(None) => dirty = false,
                        Ok(Some(err)) => {
                            set_status(&format!("Discord rejected the update: {err}"));
                            dirty = false;
                        }
                        Err(e) => {
                            set_status(&format!("Lost connection to Discord ({e}), retrying…"));
                            conn = None;
                            next_try = Instant::now() + Duration::from_secs(5);
                        }
                    }
                    last_sent = Some(Instant::now());
                }
            }
            if let Some(mut c) = conn {
                let _ = c.set_activity(None);
            }
        });
        Presence { tx, status }
    }

    pub fn configure(&self, enabled: bool, client_id: &str) {
        let _ = self.tx.send(Msg::Config { enabled, client_id: client_id.to_string() });
    }

    pub fn set(&self, activity: Option<Activity>) {
        let _ = self.tx.send(Msg::Activity(activity));
    }

    pub fn status(&self) -> String {
        self.status.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

// ---------------------------------------------------------------------------------------------
// IPC

#[cfg(windows)]
type Stream = std::fs::File;
#[cfg(unix)]
type Stream = std::os::unix::net::UnixStream;

struct Conn {
    stream: Stream,
    nonce: u64,
}

fn connect() -> Option<Stream> {
    for i in 0..10 {
        #[cfg(windows)]
        {
            let pipe = format!(r"\\.\pipe\discord-ipc-{i}");
            if let Ok(f) = std::fs::OpenOptions::new().read(true).write(true).open(pipe) {
                return Some(f);
            }
        }
        #[cfg(unix)]
        {
            let base = ["XDG_RUNTIME_DIR", "TMPDIR", "TMP", "TEMP"]
                .iter()
                .find_map(|v| std::env::var_os(v))
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| "/tmp".into());
            // plain install, Flatpak, Snap
            for sub in ["", "app/com.discordapp.Discord", "snap.discord"] {
                let path = base.join(sub).join(format!("discord-ipc-{i}"));
                if let Ok(s) = std::os::unix::net::UnixStream::connect(path) {
                    return Some(s);
                }
            }
        }
    }
    None
}

impl Conn {
    fn open(client_id: &str) -> Result<(Conn, String), String> {
        let stream = connect().ok_or("Discord isn't running, will keep trying")?;
        let mut c = Conn { stream, nonce: 0 };
        let io = |e: std::io::Error| format!("Couldn't talk to Discord: {e}");
        c.send(0, &json!({ "v": 1, "client_id": client_id })).map_err(io)?;
        let (op, v) = c.recv().map_err(io)?;
        if op == 2 {
            let msg = v["message"].as_str().unwrap_or("check the Application ID");
            return Err(format!("Discord refused the connection: {msg}"));
        }
        let user = &v["data"]["user"];
        let name = user["global_name"].as_str().or(user["username"].as_str()).unwrap_or("you");
        Ok((c, name.to_string()))
    }

    fn send(&mut self, op: u32, payload: &Value) -> std::io::Result<()> {
        let body = payload.to_string();
        let mut frame = Vec::with_capacity(8 + body.len());
        frame.extend_from_slice(&op.to_le_bytes());
        frame.extend_from_slice(&(body.len() as u32).to_le_bytes());
        frame.extend_from_slice(body.as_bytes());
        self.stream.write_all(&frame)?;
        self.stream.flush()
    }

    fn recv(&mut self) -> std::io::Result<(u32, Value)> {
        let mut header = [0u8; 8];
        self.stream.read_exact(&mut header)?;
        let op = u32::from_le_bytes(header[..4].try_into().unwrap());
        let len = u32::from_le_bytes(header[4..].try_into().unwrap()) as usize;
        let mut body = vec![0u8; len.min(1 << 20)];
        self.stream.read_exact(&mut body)?;
        Ok((op, serde_json::from_slice(&body).unwrap_or(Value::Null)))
    }

    /// Ok(Some(msg)) when Discord answered with an error (bad field etc.).
    fn set_activity(&mut self, a: Option<&Activity>) -> std::io::Result<Option<String>> {
        self.nonce += 1;
        let activity = a.map(activity_json).unwrap_or(Value::Null);
        let cmd = json!({
            "cmd": "SET_ACTIVITY",
            "args": { "pid": std::process::id(), "activity": activity },
            "nonce": self.nonce.to_string(),
        });
        self.send(1, &cmd)?;
        let (op, v) = self.recv()?;
        if op == 2 {
            return Err(std::io::Error::new(std::io::ErrorKind::ConnectionAborted, "closed by Discord"));
        }
        Ok((v["evt"] == "ERROR").then(|| v["data"]["message"].as_str().unwrap_or("unknown error").to_string()))
    }
}

/// Discord wants 2..=128 characters in each text field.
fn field(s: &str) -> String {
    let mut t: String = s.trim().chars().take(128).collect();
    while t.chars().count() < 2 {
        t.push('\u{2800}'); // blank braille cell: renders empty but counts as a character
    }
    t
}

fn activity_json(a: &Activity) -> Value {
    let who = if a.artist.is_empty() { "Unknown artist" } else { &a.artist };
    let state = if a.paused { format!("Paused · {who}") } else { format!("by {who}") };
    let mut assets = json!({
        "large_image": a.cover_url.as_deref().unwrap_or(ICON_URL),
        "large_text": field(if a.album.is_empty() { "JAMP" } else { &a.album }),
    });
    if a.cover_url.is_some() {
        assets["small_image"] = json!(ICON_URL);
        assets["small_text"] = json!("JAMP");
    }
    let mut activity = json!({
        "type": 2, // "Listening to"
        "details": field(&a.title),
        "state": field(&state),
        "assets": assets,
    });
    if let Some((start, end)) = a.span {
        activity["timestamps"] = json!({ "start": start, "end": end });
    }
    activity
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fields_respect_discord_limits() {
        assert_eq!(field("a").chars().count(), 2);
        assert_eq!(field(&"x".repeat(300)).chars().count(), 128);
        let a = Activity { title: "曲".into(), artist: String::new(), album: String::new(), cover_url: None, span: Some((1, 2)), paused: false };
        let v = activity_json(&a);
        assert_eq!(v["type"], 2);
        assert_eq!(v["assets"]["large_image"], ICON_URL);
        assert_eq!(v["timestamps"]["end"], 2);
    }
}
