use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const DEFAULT_HOTKEY: &str = "Ctrl+Alt+V";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    System,
    Dark,
    Light,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    pub device_name: Option<String>,
    pub save_dir: Option<PathBuf>,
    pub hotkey: String,
    pub autostart: bool,
    pub theme: Theme,
    pub onboarded: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            device_name: None,
            save_dir: None,
            hotkey: DEFAULT_HOTKEY.to_string(),
            autostart: true,
            theme: Theme::System,
            onboarded: false,
        }
    }
}

impl AppSettings {
    pub fn load(path: &Path) -> Self {
        match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
                tracing::warn!("settings file unreadable, using defaults: {e}");
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let tmp = path.with_extension("json.tmp");
        let bytes = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, path).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sensible() {
        let s = AppSettings::default();
        assert_eq!(s.hotkey, DEFAULT_HOTKEY);
        assert!(s.autostart);
        assert_eq!(s.theme, Theme::System);
        assert!(!s.onboarded);
        assert_eq!(s.device_name, None);
    }

    #[test]
    fn save_and_load_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let s = AppSettings {
            device_name: Some("Wolfgang's PC".into()),
            save_dir: Some(dir.path().join("in")),
            hotkey: "Ctrl+Shift+Space".into(),
            autostart: false,
            theme: Theme::Light,
            onboarded: true,
        };
        s.save(&path).unwrap();
        assert_eq!(AppSettings::load(&path), s);
    }

    #[test]
    fn missing_corrupt_or_partial_files_fall_back_to_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        assert_eq!(AppSettings::load(&path), AppSettings::default());
        std::fs::write(&path, "{not json").unwrap();
        assert_eq!(AppSettings::load(&path), AppSettings::default());
        std::fs::write(&path, r#"{"onboarded":true,"futureField":1}"#).unwrap();
        let s = AppSettings::load(&path);
        assert!(s.onboarded);
        assert_eq!(s.hotkey, DEFAULT_HOTKEY);
    }
}
