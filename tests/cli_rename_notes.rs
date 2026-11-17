use std::path::PathBuf;

use assert_cmd::Command;
use serde_json::Value;
use tempfile::TempDir;

struct TestEnv {
    _temp_dir: TempDir,
    db_path: PathBuf,
    config_path: PathBuf,
}

impl TestEnv {
    fn new() -> Self {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        Self {
            db_path: temp_dir.path().join("todui.db"),
            config_path: temp_dir.path().join("config.toml"),
            _temp_dir: temp_dir,
        }
    }

    fn command(&self) -> Command {
        let mut command = Command::cargo_bin("todui").expect("binary");
        command.env("TODO_TUI_DB", &self.db_path);
        command.env("TODO_TUI_CONFIG", &self.config_path);
        command
    }

    fn run(&self, args: &[&str]) -> String {
        let output = self
            .command()
            .args(args)
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        String::from_utf8(output).expect("utf8")
    }

    fn session(&self, slug: &str) -> Option<Value> {
        let listing: Value =
            serde_json::from_str(&self.run(&["list", "--json"])).expect("valid json");
        listing["sessions"]
            .as_array()
            .expect("sessions")
            .iter()
            .find(|session| session["slug"] == slug)
            .cloned()
    }
}

#[test]
fn rename_keeps_tag_repo_and_todos() {
    let env = TestEnv::new();
    env.run(&[
        "session",
        "new",
        "Old Name",
        "--tag",
        "work",
        "--repo",
        "@acme/widget",
    ]);
    env.run(&["add", "Keep me", "--session", "old-name"]);

    assert_eq!(
        env.run(&["session", "rename", "old-name", "--to", "New Name"]),
        "new-name\n"
    );

    assert!(env.session("old-name").is_none());
    let renamed = env.session("new-name").expect("renamed session");
    assert_eq!(renamed["tag"], "work");
    assert_eq!(renamed["repo"], "acme/widget");
    assert_eq!(renamed["openCount"], 1);
}

#[test]
fn rename_to_an_existing_session_fails() {
    let env = TestEnv::new();
    env.run(&["session", "new", "alpha"]);
    env.run(&["session", "new", "beta"]);
    env.command()
        .args(["session", "rename", "alpha", "--to", "beta"])
        .assert()
        .failure();
}

#[test]
fn notes_round_trip_and_clear() {
    let env = TestEnv::new();
    assert_eq!(env.run(&["notes"]), "");

    env.run(&["notes", "--set", "line one\nline two"]);
    assert_eq!(env.run(&["notes"]), "line one\nline two\n");

    env.run(&["notes", "--clear"]);
    assert_eq!(env.run(&["notes"]), "");
}
