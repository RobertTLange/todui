use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{AppError, Result};

const CONFIG_ENV: &str = "TODO_TUI_CONFIG";
const DB_ENV: &str = "TODO_TUI_DB";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    pub config_path: PathBuf,
    pub db_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Config {
    #[serde(default)]
    pub theme: ThemeConfig,
    #[serde(default)]
    pub pomodoro: PomodoroConfig,
    #[serde(default)]
    pub keys: KeyBindings,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ThemeConfig {
    #[serde(default = "default_theme_mode")]
    pub mode: String,
    #[serde(default = "default_accent")]
    pub accent: String,
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            mode: default_theme_mode(),
            accent: default_accent(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PomodoroConfig {
    #[serde(default = "default_focus_minutes")]
    pub focus_minutes: u16,
    #[serde(default = "default_short_break_minutes")]
    pub short_break_minutes: u16,
    #[serde(default = "default_long_break_minutes")]
    pub long_break_minutes: u16,
    #[serde(default = "default_notify_on_complete")]
    pub notify_on_complete: bool,
}

impl Default for PomodoroConfig {
    fn default() -> Self {
        Self {
            focus_minutes: default_focus_minutes(),
            short_break_minutes: default_short_break_minutes(),
            long_break_minutes: default_long_break_minutes(),
            notify_on_complete: default_notify_on_complete(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KeyBindings {
    #[serde(default = "default_up_keys")]
    pub up: Vec<String>,
    #[serde(default = "default_down_keys")]
    pub down: Vec<String>,
    #[serde(default = "default_toggle_done_keys")]
    pub toggle_done: Vec<String>,
    #[serde(default = "default_history_keys")]
    pub history: Vec<String>,
    #[serde(default = "default_pomodoro_keys")]
    pub pomodoro: Vec<String>,
}

impl Default for KeyBindings {
    fn default() -> Self {
        Self {
            up: default_up_keys(),
            down: default_down_keys(),
            toggle_done: default_toggle_done_keys(),
            history: default_history_keys(),
            pomodoro: default_pomodoro_keys(),
        }
    }
}

pub fn resolve_paths() -> Result<AppPaths> {
    resolve_paths_from(|key| std::env::var_os(key), home_dir())
}

pub fn load(paths: &AppPaths) -> Result<Config> {
    load_from_path(&paths.config_path)
}

fn load_from_path(path: &Path) -> Result<Config> {
    match std::fs::read_to_string(path) {
        Ok(contents) => toml::from_str(&contents).map_err(|source| AppError::ParseConfig {
            path: path.to_path_buf(),
            source,
        }),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
        Err(source) => Err(AppError::ReadConfig {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn resolve_paths_from<F>(get_env: F, home_dir: Option<PathBuf>) -> Result<AppPaths>
where
    F: Fn(&str) -> Option<OsString>,
{
    if let (Some(config_path), Some(db_path)) = (get_env(CONFIG_ENV), get_env(DB_ENV)) {
        return Ok(AppPaths {
            config_path: PathBuf::from(config_path),
            db_path: PathBuf::from(db_path),
        });
    }

    let home_dir = home_dir.ok_or(AppError::HomeDirUnavailable)?;
    let config_path = get_env(CONFIG_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir.join(".config/todui/config.toml"));
    let db_path = get_env(DB_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir.join(".local/share/todui/todui.db"));

    Ok(AppPaths {
        config_path,
        db_path,
    })
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn default_theme_mode() -> String {
    String::from("dark")
}

fn default_accent() -> String {
    String::from("cyan")
}

fn default_focus_minutes() -> u16 {
    25
}

fn default_short_break_minutes() -> u16 {
    5
}

fn default_long_break_minutes() -> u16 {
    15
}

fn default_notify_on_complete() -> bool {
    true
}

fn default_up_keys() -> Vec<String> {
    vec![String::from("up"), String::from("k")]
}

fn default_down_keys() -> Vec<String> {
    vec![String::from("down"), String::from("j")]
}

fn default_toggle_done_keys() -> Vec<String> {
    vec![String::from("space"), String::from("x")]
}

fn default_history_keys() -> Vec<String> {
    vec![String::from("H")]
}

fn default_pomodoro_keys() -> Vec<String> {
    vec![String::from("p")]
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::fs;
    use std::path::PathBuf;

    use super::{Config, load_from_path, resolve_paths_from};

    #[test]
    fn resolves_default_paths_from_home_dir() {
        let paths = resolve_paths_from(|_| None::<OsString>, Some(PathBuf::from("/tmp/rob")))
            .expect("paths should resolve");

        assert_eq!(
            paths.config_path,
            PathBuf::from("/tmp/rob/.config/todui/config.toml")
        );
        assert_eq!(
            paths.db_path,
            PathBuf::from("/tmp/rob/.local/share/todui/todui.db")
        );
    }

    #[test]
    fn env_overrides_are_applied_independently() {
        let paths = resolve_paths_from(
            |key| match key {
                "TODO_TUI_CONFIG" => Some(OsString::from("/tmp/custom/config.toml")),
                "TODO_TUI_DB" => Some(OsString::from("/tmp/custom/todui.db")),
                _ => None,
            },
            Some(PathBuf::from("/tmp/ignored")),
        )
        .expect("paths should resolve");

        assert_eq!(paths.config_path, PathBuf::from("/tmp/custom/config.toml"));
        assert_eq!(paths.db_path, PathBuf::from("/tmp/custom/todui.db"));
    }

    #[test]
    fn missing_config_file_uses_defaults() {
        let directory = tempfile::tempdir().expect("tempdir");
        let config_path = directory.path().join("config.toml");
        let config = load_from_path(&config_path).expect("default config");

        assert_eq!(config, Config::default());
        assert!(config.pomodoro.notify_on_complete);
    }

    #[test]
    fn loads_explicit_pomodoro_completion_notification_flag() {
        let directory = tempfile::tempdir().expect("tempdir");
        let config_path = directory.path().join("config.toml");
        fs::write(&config_path, "[pomodoro]\nnotify_on_complete = false\n").expect("config");

        let config = load_from_path(&config_path).expect("parsed config");

        assert!(!config.pomodoro.notify_on_complete);
    }
}
