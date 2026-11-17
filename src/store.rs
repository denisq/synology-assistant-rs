//! Small JSON files in the per-user config directory
//! (`~/Library/Application Support/SynologyAssistantRS` on macOS).

use serde::{Serialize, de::DeserializeOwned};
use std::io;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn path(file: &str) -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("SynologyAssistantRS")
        .join(file)
}

pub fn load<T: DeserializeOwned + Default>(file: &str) -> T {
    std::fs::read(path(file))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

pub fn save<T: Serialize + ?Sized>(file: &str, value: &T) -> io::Result<()> {
    let p = path(file);
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)?;
    }
    // Write then rename, so a crash never leaves a half-written file.
    let tmp = p.with_extension("tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(value)?)?;
    std::fs::rename(tmp, p)
}

/// Seconds since the Unix epoch (wall clock, for things persisted across runs).
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// "just now", "5 min ago", "3 h ago", "2 days ago".
pub fn ago(then: u64, now: u64) -> String {
    let s = now.saturating_sub(then);
    match s {
        0..60 => "just now".into(),
        60..3600 => format!("{} min ago", s / 60),
        3600..86400 => format!("{} h ago", s / 3600),
        _ => {
            let d = s / 86400;
            format!("{d} day{} ago", if d == 1 { "" } else { "s" })
        }
    }
}

/// "45 min", "2 h 10 min", "1 day 3 h".
pub fn duration(secs: u64) -> String {
    let (d, h, m) = (secs / 86400, secs % 86400 / 3600, secs % 3600 / 60);
    match (d, h) {
        (0, 0) => format!("{} min", m.max(1)),
        (0, _) if m == 0 => format!("{h} h"),
        (0, _) => format!("{h} h {m} min"),
        _ => format!("{d} day{} {h} h", if d == 1 { "" } else { "s" }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_times() {
        assert_eq!(ago(100, 110), "just now");
        assert_eq!(ago(0, 300), "5 min ago");
        assert_eq!(ago(0, 3 * 3600 + 5), "3 h ago");
        assert_eq!(ago(0, 86400), "1 day ago");
        assert_eq!(duration(20), "1 min");
        assert_eq!(duration(2 * 3600 + 600), "2 h 10 min");
        assert_eq!(duration(3600), "1 h");
        assert_eq!(duration(2 * 86400 + 3600), "2 days 1 h");
    }
}
