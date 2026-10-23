//! User settings, stored as JSON in the encrypted library (`settings` table).

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// `en`, `he` or `ar`; `None` follows the system.
    pub language: Option<String>,
    pub theme: String,
    /// Off by default (docs/OPEN_QUESTIONS.md Q12).
    pub lookup_online: bool,
    pub tmdb_key: Option<String>,
    pub metadata_language: String,
    pub audio_languages: Vec<String>,
    pub subtitle_languages: Vec<String>,
    pub hwdec: bool,
    pub autoplay_next: bool,
    /// 0 = never.
    pub idle_lock_minutes: u32,
    pub lock_on_sleep: bool,
    pub subtitle_scale: f64,
    pub subtitle_pos: f64,
    pub subtitle_color: String,
    pub subtitle_background: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            language: None,
            theme: "dark".into(),
            lookup_online: false,
            tmdb_key: None,
            metadata_language: "en-US".into(),
            audio_languages: Vec::new(),
            subtitle_languages: Vec::new(),
            hwdec: true,
            autoplay_next: true,
            idle_lock_minutes: 15,
            lock_on_sleep: true,
            subtitle_scale: 1.0,
            subtitle_pos: 100.0,
            subtitle_color: "#FFFFFF".into(),
            subtitle_background: true,
        }
    }
}

impl Settings {
    /// Reads stored settings; unknown or missing fields fall back to defaults.
    pub fn from_value(v: Value) -> Settings {
        serde_json::from_value(v).unwrap_or_default()
    }

    /// Applies a partial update (keys as the UI sends them).
    pub fn merged(&self, patch: Value) -> Settings {
        let mut cur = serde_json::to_value(self).unwrap_or(Value::Null);
        if let (Value::Object(cur), Value::Object(patch)) = (&mut cur, patch) {
            for (k, v) in patch {
                cur.insert(k, v);
            }
        }
        let mut s = Settings::from_value(cur);
        s.subtitle_scale = s.subtitle_scale.clamp(0.5, 3.0);
        s.subtitle_pos = s.subtitle_pos.clamp(0.0, 150.0);
        s.tmdb_key = s.tmdb_key.filter(|k| !k.trim().is_empty());
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patches_and_defaults() {
        let s = Settings::from_value(serde_json::json!({ "language": "he", "bogus": 1 }));
        assert_eq!(s.language.as_deref(), Some("he"));
        assert!(!s.lookup_online);
        let s = s.merged(serde_json::json!({ "lookupOnline": true, "subtitleScale": 9.0, "tmdbKey": " " }));
        assert!(s.lookup_online);
        assert_eq!(s.subtitle_scale, 3.0);
        assert_eq!(s.tmdb_key, None);
        assert_eq!(s.language.as_deref(), Some("he"));
    }
}
