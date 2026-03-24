use std::fs;
use std::path::PathBuf;

use assert_cmd::Command;
use predicates::prelude::*;
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
}

#[test]
fn session_todo_history_and_export_flow() {
    let env = TestEnv::new();

    env.command()
        .args(["session", "new", "Writing Sprint"])
        .assert()
        .success()
        .stdout(predicate::str::contains("writing-sprint\n"));

    env.command()
        .args([
            "add",
            "Draft design spec",
            "--session",
            "writing-sprint",
            "--note",
            "cover CLI, TUI, DB, pomodoro",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("1\n"));

    env.command()
        .args(["done", "1", "--session", "writing-sprint"])
        .assert()
        .success()
        .stdout(predicate::str::contains("1\tdone\n"));

    env.command()
        .args(["session", "history", "writing-sprint"])
        .assert()
        .success()
        .stdout(predicate::str::contains("r3"))
        .stdout(predicate::str::contains("todo status changed"))
        .stdout(predicate::str::contains("r1"));

    env.command()
        .args([
            "export",
            "md",
            "writing-sprint",
            "--format",
            "gfm",
            "--timestamps",
            "full",
            "--include-notes",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("# Session: writing-sprint"))
        .stdout(predicate::str::contains("- [x] Draft design spec"))
        .stdout(predicate::str::contains(
            "notes: cover CLI, TUI, DB, pomodoro",
        ));
}

#[test]
fn export_to_file_and_no_recent_session_error() {
    let env = TestEnv::new();

    env.command()
        .args(["add", "Orphan todo"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("no recent session"));

    env.command()
        .args(["session", "new", "Writing Sprint"])
        .assert()
        .success();

    env.command()
        .args(["add", "Draft design spec", "--session", "writing-sprint"])
        .assert()
        .success();

    let output_path = env.db_path.parent().expect("parent").join("export.md");
    env.command()
        .args([
            "export",
            "md",
            "writing-sprint",
            "--output",
            output_path.to_str().expect("utf8 path"),
            "--open-only",
        ])
        .assert()
        .success()
        .stdout(predicate::str::is_empty());

    let exported = fs::read_to_string(output_path).expect("read export");
    assert!(exported.contains("Draft design spec"));
}
