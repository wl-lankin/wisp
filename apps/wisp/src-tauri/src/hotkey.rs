use std::str::FromStr;

use tauri::AppHandle;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

pub fn parse(combo: &str) -> Result<Shortcut, String> {
    let combo = combo.trim();
    if combo.is_empty() {
        return Err("The shortcut is empty".into());
    }
    Shortcut::from_str(combo).map_err(|e| format!("\"{combo}\" is not a valid shortcut ({e})"))
}

/// Replaces the registered catch shortcut. On failure the previous one stays active.
pub fn apply(app: &AppHandle, previous: Option<&str>, next: &str) -> Result<(), String> {
    let next_shortcut = parse(next)?;
    let shortcuts = app.global_shortcut();
    let previous_shortcut = previous.and_then(|p| parse(p).ok());
    if let Some(prev) = previous_shortcut {
        if prev == next_shortcut && shortcuts.is_registered(prev) {
            return Ok(());
        }
        let _ = shortcuts.unregister(prev);
    }
    if let Err(e) = shortcuts.register(next_shortcut) {
        if let Some(prev) = previous_shortcut {
            let _ = shortcuts.register(prev);
        }
        return Err(format!("{next} could not be registered, another app may use it ({e})"));
    }
    Ok(())
}

pub fn plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, _shortcut, event| {
            if event.state() == ShortcutState::Pressed {
                let app = app.clone();
                tauri::async_runtime::spawn(async move { crate::catch_from_hotkey(&app).await });
            }
        })
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_default_and_rejects_garbage() {
        assert!(parse(crate::settings::DEFAULT_HOTKEY).is_ok());
        assert!(parse("Ctrl+Shift+Space").is_ok());
        assert!(parse("Alt+F9").is_ok());
        assert!(parse("Ctrl+5").is_ok());
        assert!(parse("Shift+Super+Space").is_ok());
        assert!(parse("Ctrl+").is_err());
        assert!(parse("Banana+V").is_err());
        assert!(parse("").is_err());
    }
}
