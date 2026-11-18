//! Minimal translation layer.
//!
//! The English text in the code is the key. Each other language is one JSON file in
//! `locales/` mapping English text to its translation, e.g. `locales/es.json`:
//!
//! ```json
//! { "Search": "Buscar", "Waking {name}…": "Despertando {name}…" }
//! ```
//!
//! To add a language: copy `locales/es.json` to `locales/<code>.json`, translate the
//! values (keep the `{placeholders}`), and add it to [`LANGUAGES`]. Missing entries
//! fall back to English; `cargo test` lists them.

use std::cell::Cell;
use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};

pub type Table = HashMap<&'static str, &'static str>;

/// (code, name in that language, translations JSON).
pub const LANGUAGES: &[(&str, &str, &str)] = &[
    ("en", "English", "{}"),
    ("es", "Español", include_str!("../locales/es.json")),
];

fn tables() -> &'static Vec<(&'static str, Table)> {
    static T: OnceLock<Vec<(&'static str, Table)>> = OnceLock::new();
    T.get_or_init(|| {
        LANGUAGES
            .iter()
            .map(|(code, _, json)| {
                let map: HashMap<String, String> = serde_json::from_str(json).unwrap_or_default();
                let table = map
                    .into_iter()
                    .filter(|(_, v)| !v.is_empty())
                    .map(|(k, v)| (&*Box::leak(k.into_boxed_str()), &*Box::leak(v.into_boxed_str())))
                    .collect();
                (*code, table)
            })
            .collect()
    })
}

static CURRENT: RwLock<&str> = RwLock::new("en");

thread_local! {
    /// Per-thread override, so tests can switch language without affecting each other.
    static OVERRIDE: Cell<Option<&'static str>> = const { Cell::new(None) };
}

/// Supported language code for a locale like "es-ES" or "es_MX.UTF-8"; `None` if unsupported.
pub fn supported(locale: &str) -> Option<&'static str> {
    let code = locale.split(['-', '_', '.']).next()?.to_lowercase();
    LANGUAGES.iter().find(|(c, _, _)| *c == code).map(|(c, _, _)| *c)
}

pub fn set_language(code: &str) {
    if let Some(c) = supported(code) {
        *CURRENT.write().unwrap() = c;
    }
}

/// Use `code` on the current thread only (for tests).
pub fn set_thread_language(code: Option<&str>) {
    OVERRIDE.with(|o| o.set(code.and_then(supported)));
}

pub fn language() -> &'static str {
    OVERRIDE.with(|o| o.get()).unwrap_or_else(|| *CURRENT.read().unwrap())
}

/// The language to start with: an explicit choice, `SYNOLOGY_ASSISTANT_LANG`, then the OS.
pub fn detect(saved: Option<&str>) -> &'static str {
    saved
        .and_then(supported)
        .or_else(|| {
            std::env::var("SYNOLOGY_ASSISTANT_LANG")
                .ok()
                .as_deref()
                .and_then(supported)
        })
        .or_else(|| sys_locale::get_locales().find_map(|l| supported(&l)))
        .unwrap_or("en")
}

fn misses() -> &'static RwLock<Vec<&'static str>> {
    static M: OnceLock<RwLock<Vec<&'static str>>> = OnceLock::new();
    M.get_or_init(Default::default)
}

/// Translate `s` into the current language (English text is returned as is).
pub fn tr(s: &'static str) -> &'static str {
    let lang = language();
    if lang == "en" {
        return s;
    }
    let table = tables().iter().find(|(c, _)| *c == lang).map(|(_, t)| t);
    match table.and_then(|t| t.get(s)) {
        Some(t) => t,
        None => {
            let mut m = misses().write().unwrap();
            if !m.contains(&s) {
                m.push(s);
            }
            s
        }
    }
}

/// Texts that had no translation so far (used by tests).
pub fn missing() -> Vec<&'static str> {
    misses().read().unwrap().clone()
}

/// Replace `{name}` placeholders.
pub fn fill(template: &str, args: &[(&str, String)]) -> String {
    let mut out = template.to_string();
    for (k, v) in args {
        out = out.replace(&format!("{{{k}}}"), v);
    }
    out
}

/// `t!("Search")` → translated `&'static str`;
/// `t!("Waking {name}…", name = n)` → translated `String` with placeholders filled.
#[macro_export]
macro_rules! t {
    ($s:literal) => {
        $crate::i18n::tr($s)
    };
    ($s:literal, $($k:ident = $v:expr),+ $(,)?) => {
        $crate::i18n::fill($crate::i18n::tr($s), &[$((stringify!($k), ($v).to_string())),+])
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_and_fallback() {
        set_thread_language(Some("es"));
        assert_eq!(tr("Search"), "Buscar");
        assert_eq!(tr("No such text"), "No such text");
        assert!(
            crate::t!("Waking {name}… this usually takes 1–3 minutes.", name = "nas").starts_with("Despertando nas")
        );
        set_thread_language(None);
        assert_eq!(tr("Search"), "Search");
        assert_eq!(supported("es_ES.UTF-8"), Some("es"));
        assert_eq!(supported("fr-FR"), None);
    }

    /// Every `t!("…")` text in the source has a Spanish translation with the same placeholders.
    #[test]
    fn spanish_is_complete() {
        let es: HashMap<String, String> = serde_json::from_str(LANGUAGES[1].2).unwrap();
        let mut missing = Vec::new();
        let mut bad = Vec::new();
        for key in keys_in_source() {
            match es.get(&key) {
                None => missing.push(key),
                Some(v) if placeholders(v) != placeholders(&key) => bad.push(key),
                _ => {}
            }
        }
        assert!(
            missing.is_empty(),
            "missing in locales/es.json:\n{}",
            missing.join("\n")
        );
        assert!(bad.is_empty(), "placeholders differ:\n{}", bad.join("\n"));
    }

    /// Writes `locales/template.json` with every text to translate (existing Spanish
    /// values are kept as a reference): `cargo test --lib dump_template -- --ignored`
    #[test]
    #[ignore = "writes locales/template.json"]
    fn dump_template() {
        let mut map = serde_json::Map::new();
        for k in keys_in_source() {
            map.insert(k, serde_json::Value::String(String::new()));
        }
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/locales/template.json");
        std::fs::write(path, serde_json::to_string_pretty(&map).unwrap() + "\n").unwrap();
    }

    fn placeholders(s: &str) -> Vec<String> {
        let mut v: Vec<String> = s
            .split('{')
            .skip(1)
            .filter_map(|p| p.split_once('}').map(|(k, _)| k.to_string()))
            .collect();
        v.sort();
        v
    }

    /// The (unescaped) text of every `t!("…")` literal in src/, plus the help content.
    pub fn keys_in_source() -> Vec<String> {
        let mut keys = Vec::new();
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/src");
        for e in std::fs::read_dir(dir).unwrap() {
            let path = e.unwrap().path();
            if path.ends_with("i18n.rs") {
                continue; // only documentation examples here
            }
            // Windows checkouts use CRLF; Rust strips it in string continuations, so do the same.
            let src = std::fs::read_to_string(path).unwrap().replace("\r\n", "\n");
            let mut rest = src.as_str();
            while let Some(i) = rest.find("t!(") {
                // skip things like `format!(` or `assert!(`
                let before = rest[..i].chars().last();
                rest = &rest[i + 3..];
                if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                    continue;
                }
                let trimmed = rest.trim_start();
                let Some(body) = trimmed.strip_prefix('"') else {
                    continue;
                };
                let (lit, after) = read_literal(body);
                keys.push(lit);
                rest = after;
            }
        }
        for s in crate::help::all_texts() {
            keys.push(s.to_string());
        }
        keys.sort();
        keys.dedup();
        keys
    }

    /// Read a Rust string literal body (after the opening quote), unescaping it.
    fn read_literal(s: &str) -> (String, &str) {
        let mut out = String::new();
        let mut chars = s.char_indices().peekable();
        while let Some((i, c)) = chars.next() {
            match c {
                '"' => return (out, &s[i + 1..]),
                '\\' => match chars.next() {
                    Some((_, 'n')) => out.push('\n'),
                    Some((_, '"')) => out.push('"'),
                    Some((_, '\\')) => out.push('\\'),
                    Some((_, '\n')) => {
                        while chars.peek().is_some_and(|(_, c)| c.is_whitespace()) {
                            chars.next();
                        }
                    }
                    Some((_, c)) => out.push(c),
                    None => break,
                },
                c => out.push(c),
            }
        }
        (out, "")
    }
}
