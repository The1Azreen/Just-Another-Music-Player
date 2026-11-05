//! Plain and LRC (time-synced) lyrics.

#[derive(Clone, Debug, Default)]
pub struct Lyrics {
    /// (timestamp in seconds, text). Timestamps are all `Some` when `synced`.
    pub lines: Vec<(Option<f64>, String)>,
    pub synced: bool,
}

/// Parses "[mm:ss.xx]" / "[mm:ss:xx]" / "[mm:ss]".
fn parse_stamp(s: &str) -> Option<f64> {
    let (m, rest) = s.split_once(':')?;
    let m: f64 = m.trim().parse().ok()?;
    let rest = rest.replacen(':', ".", 1);
    let sec: f64 = rest.trim().parse().ok()?;
    Some(m * 60.0 + sec)
}

impl Lyrics {
    pub fn parse(text: &str) -> Lyrics {
        let mut timed = Vec::new();
        let mut plain = Vec::new();
        let mut offset = 0.0;

        for raw in text.lines() {
            let mut line = raw.trim();
            let mut stamps = Vec::new();
            while let Some(rest) = line.strip_prefix('[') {
                let Some(end) = rest.find(']') else { break };
                let inner = &rest[..end];
                if let Some(t) = parse_stamp(inner) {
                    stamps.push(t);
                } else if let Some(o) = inner.strip_prefix("offset:") {
                    // LRC offset is in ms; positive means lyrics appear earlier.
                    offset = o.trim().parse::<f64>().unwrap_or(0.0) / 1000.0;
                } else if inner.contains(':') {
                    // [ar:..] [ti:..] [length:..] metadata
                } else {
                    break;
                }
                line = rest[end + 1..].trim_start();
            }
            if stamps.is_empty() {
                if !raw.trim_start().starts_with('[') {
                    plain.push(raw.trim_end().to_string());
                }
            } else {
                for t in stamps {
                    timed.push((t, line.to_string()));
                }
            }
        }

        if timed.is_empty() {
            while plain.last().is_some_and(|l| l.is_empty()) {
                plain.pop();
            }
            return Lyrics { lines: plain.into_iter().map(|l| (None, l)).collect(), synced: false };
        }
        timed.sort_by(|a, b| a.0.total_cmp(&b.0));
        Lyrics {
            lines: timed.into_iter().map(|(t, l)| (Some((t - offset).max(0.0)), l)).collect(),
            synced: true,
        }
    }

    /// Index of the line being sung at `pos` seconds.
    pub fn current_line(&self, pos: f64) -> Option<usize> {
        if !self.synced {
            return None;
        }
        let idx = self.lines.partition_point(|(t, _)| t.unwrap_or(0.0) <= pos + 0.15);
        idx.checked_sub(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lrc() {
        let l = Lyrics::parse("[ar:Someone]\n[00:01.00]one\n[00:03.50][00:10.00]two\n[00:05.00]three");
        assert!(l.synced);
        assert_eq!(l.lines.len(), 4);
        assert_eq!(l.lines[1].1, "two");
        assert_eq!(l.current_line(0.5), None);
        assert_eq!(l.current_line(4.0), Some(1));
        assert_eq!(l.current_line(11.0), Some(3));
    }

    #[test]
    fn parses_plain() {
        let l = Lyrics::parse("hello\nworld\n\n");
        assert!(!l.synced);
        assert_eq!(l.lines.len(), 2);
    }
}
