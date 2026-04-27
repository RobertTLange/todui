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
    pub database: DatabaseConfig,
    #[serde(default)]
    pub theme: ThemeConfig,
    #[serde(default)]
    pub pomodoro: PomodoroConfig,
    #[serde(default)]
    pub keys: KeyBindings,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct DatabaseConfig {
    #[serde(default)]
    pub path: Option<PathBuf>,
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
    resolve_paths_with_overrides(None, None)
}

pub fn resolve_paths_with_overrides(
    config_path_override: Option<PathBuf>,
    db_path_override: Option<PathBuf>,
) -> Result<AppPaths> {
    resolve_paths_from(
        config_path_override,
        db_path_override,
        |key| std::env::var_os(key),
        home_dir(),
    )
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

fn resolve_paths_from<F>(
    config_path_override: Option<PathBuf>,
    db_path_override: Option<PathBuf>,
    get_env: F,
    home_dir: Option<PathBuf>,
) -> Result<AppPaths>
where
    F: Fn(&str) -> Option<OsString>,
{
    let config_path = if let Some(path) =
        config_path_override.or_else(|| get_env(CONFIG_ENV).map(PathBuf::from))
    {
        path
    } else {
        home_dir
            .as_ref()
            .ok_or(AppError::HomeDirUnavailable)?
            .join(".todui/config.toml")
    };
    let config = load_from_path(&config_path)?;
    let db_path = if let Some(path) = db_path_override
        .or_else(|| get_env(DB_ENV).map(PathBuf::from))
        .or(config.database.path)
    {
        path
    } else {
        home_dir
            .ok_or(AppError::HomeDirUnavailable)?
            .join(".local/share/todui/todui.db")
    };

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
        let paths = resolve_paths_from(
            None,
            None,
            |_| None::<OsString>,
            Some(PathBuf::from("/tmp/rob")),
        )
        .expect("paths should resolve");

        assert_eq!(
            paths.config_path,
            PathBuf::from("/tmp/rob/.todui/config.toml")
        );
        assert_eq!(
            paths.db_path,
            PathBuf::from("/tmp/rob/.local/share/todui/todui.db")
        );
    }

    #[test]
    fn env_overrides_are_applied_independently() {
        let paths = resolve_paths_from(
            None,
            None,
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

    #[test]
    fn loads_database_path_from_config() {
        let directory = tempfile::tempdir().expect("tempdir");
        let config_path = directory.path().join("config.toml");
        fs::write(
            &config_path,
            "[database]\npath = \"/tmp/configured/todui.db\"\n",
        )
        .expect("config");

        let config = load_from_path(&config_path).expect("parsed config");

        assert_eq!(
            config.database.path,
            Some(PathBuf::from("/tmp/configured/todui.db"))
        );
    }

    #[test]
    fn config_database_path_overrides_default_db_path() {
        let directory = tempfile::tempdir().expect("tempdir");
        let config_path = directory.path().join("config.toml");
        fs::write(
            &config_path,
            "[database]\npath = \"/tmp/from-config/todui.db\"\n",
        )
        .expect("config");

        let paths = resolve_paths_from(
            None,
            None,
            |key| match key {
                "TODO_TUI_CONFIG" => Some(config_path.clone().into_os_string()),
                _ => None,
            },
            Some(PathBuf::from("/tmp/ignored-home")),
        )
        .expect("paths should resolve");

        assert_eq!(paths.config_path, config_path);
        assert_eq!(paths.db_path, PathBuf::from("/tmp/from-config/todui.db"));
    }

    #[test]
    fn cli_db_override_wins_over_env_and_config() {
        let directory = tempfile::tempdir().expect("tempdir");
        let config_path = directory.path().join("config.toml");
        fs::write(
            &config_path,
            "[database]\npath = \"/tmp/from-config/todui.db\"\n",
        )
        .expect("config");

        let paths = resolve_paths_from(
            None,
            Some(PathBuf::from("/tmp/from-cli/todui.db")),
            |key| match key {
                "TODO_TUI_CONFIG" => Some(config_path.clone().into_os_string()),
                "TODO_TUI_DB" => Some(OsString::from("/tmp/from-env/todui.db")),
                _ => None,
            },
            Some(PathBuf::from("/tmp/ignored-home")),
        )
        .expect("paths should resolve");

        assert_eq!(paths.db_path, PathBuf::from("/tmp/from-cli/todui.db"));
    }

    #[test]
    fn explicit_config_and_db_paths_do_not_require_home_dir() {
        let directory = tempfile::tempdir().expect("tempdir");
        let config_path = directory.path().join("config.toml");
        fs::write(&config_path, "[theme]\nmode = \"dark\"\n").expect("config");

        let paths = resolve_paths_from(
            Some(config_path.clone()),
            Some(PathBuf::from("/tmp/from-cli/todui.db")),
            |_| None::<OsString>,
            None,
        )
        .expect("paths should resolve");

        assert_eq!(paths.config_path, config_path);
        assert_eq!(paths.db_path, PathBuf::from("/tmp/from-cli/todui.db"));
    }

    #[test]
    fn config_path_override_wins_over_env_config_path() {
        let directory = tempfile::tempdir().expect("tempdir");
        let env_config_path = directory.path().join("env.toml");
        let cli_config_path = directory.path().join("cli.toml");
        fs::write(
            &env_config_path,
            "[database]\npath = \"/tmp/from-env-config/todui.db\"\n",
        )
        .expect("env config");
        fs::write(
            &cli_config_path,
            "[database]\npath = \"/tmp/from-cli-config/todui.db\"\n",
        )
        .expect("cli config");

        let paths = resolve_paths_from(
            Some(cli_config_path.clone()),
            None,
            |key| match key {
                "TODO_TUI_CONFIG" => Some(env_config_path.clone().into_os_string()),
                _ => None,
            },
            Some(PathBuf::from("/tmp/ignored-home")),
        )
        .expect("paths should resolve");

        assert_eq!(paths.config_path, cli_config_path);
        assert_eq!(
            paths.db_path,
            PathBuf::from("/tmp/from-cli-config/todui.db")
        );
    }
}
