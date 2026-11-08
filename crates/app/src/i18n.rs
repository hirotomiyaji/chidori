//! UI text: Japanese first, English second (`i18n/ja.json`, `i18n/en.json`, keys kept identical by
//! a test). Placeholders use single braces, `{n}`, as in the spec's string table.

use std::collections::HashMap;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicBool, Ordering};

use chidori_core::settings::Locale;

type Table = HashMap<String, String>;

fn parse(src: &str) -> Table {
    serde_json::from_str(src).expect("translation file is a flat JSON object of strings")
}

static JA: LazyLock<Table> = LazyLock::new(|| parse(include_str!("../i18n/ja.json")));
static EN: LazyLock<Table> = LazyLock::new(|| parse(include_str!("../i18n/en.json")));

static ENGLISH: AtomicBool = AtomicBool::new(false);

pub fn set_locale(locale: Locale) {
    ENGLISH.store(locale == Locale::En, Ordering::Relaxed);
}

pub fn locale() -> Locale {
    if ENGLISH.load(Ordering::Relaxed) { Locale::En } else { Locale::Ja }
}

pub fn is_ja() -> bool {
    locale() == Locale::Ja
}

/// The text for a key; English when the key is missing in Japanese, the key itself when it is
/// missing in both.
pub fn t(key: &str) -> String {
    let table = if ENGLISH.load(Ordering::Relaxed) { &*EN } else { &*JA };
    table.get(key).or_else(|| EN.get(key)).cloned().unwrap_or_else(|| key.to_string())
}

/// The text for a key with its `{name}` placeholders filled.
pub fn tf(key: &str, params: &[(&str, &dyn std::fmt::Display)]) -> String {
    let mut s = t(key);
    for (name, value) in params {
        s = s.replace(&format!("{{{name}}}"), &value.to_string());
    }
    s
}

/// Fills placeholders from a notice's JSON params.
pub fn t_json(key: &str, params: &serde_json::Map<String, serde_json::Value>) -> String {
    let mut s = t(key);
    for (name, value) in params {
        let v = match value {
            serde_json::Value::String(v) => v.clone(),
            other => other.to_string(),
        };
        s = s.replace(&format!("{{{name}}}"), &v);
    }
    s
}

/// Why one app's sound can't be chosen on this OS.
pub fn app_capture_unavailable() -> String {
    t(if cfg!(target_os = "macos") { "appCaptureMacLater" } else { "win11Required" })
}

/// The mic list's entry for the system's default device.
pub fn default_device() -> String {
    t(if cfg!(target_os = "macos") { "defaultDeviceMac" } else { "defaultDevice" })
}

#[cfg(test)]
mod tests {
    #[test]
    fn both_languages_have_the_same_keys() {
        let mut ja: Vec<_> = super::JA.keys().collect();
        let mut en: Vec<_> = super::EN.keys().collect();
        ja.sort();
        en.sort();
        assert_eq!(ja, en);
    }

    #[test]
    fn placeholders_are_filled() {
        super::set_locale(chidori_core::settings::Locale::Ja);
        assert_eq!(super::tf("lag", &[("n", &12)]), "遅れ 12秒");
    }
}
