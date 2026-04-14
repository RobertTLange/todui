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
        .args(["session", "new", "Writing Sprint", "--tag", "work"])
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
        .stdout(predicate::str::contains("- tag: work"))
        .stdout(predicate::str::contains("- [x] Draft design spec"))
        .stdout(predicate::str::contains("created-by: agent"))
        .stdout(predicate::str::contains("completed-by: agent"))
        .stdout(predicate::str::contains(
            "notes: cover CLI, TUI, DB, pomodoro",
        ));
}

#[test]
fn cli_human_override_changes_provenance() {
    let env = TestEnv::new();

    env.command()
        .args(["session", "new", "Writing Sprint"])
        .assert()
        .success();

    env.command()
        .args([
            "add",
            "Interview notes",
            "--session",
            "writing-sprint",
            "--human",
        ])
        .assert()
        .success();

    env.command()
        .args(["done", "1", "--session", "writing-sprint", "--human"])
        .assert()
        .success();

    env.command()
        .args(["export", "md", "writing-sprint"])
        .assert()
        .success()
        .stdout(predicate::str::contains("created-by: human"))
        .stdout(predicate::str::contains("completed-by: human"));
}

#[test]
fn cli_session_tag_updates_and_lists_tag_column() {
    let env = TestEnv::new();

    env.command()
        .args(["session", "new", "Writing Sprint"])
        .assert()
        .success();

    env.command()
        .args([
            "session",
            "tag",
            "writing-sprint",
            "--set",
            "Private Projects",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "writing-sprint\tprivate-projects\n",
        ));

    env.command()
        .args(["session", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "writing-sprint\tprivate-projects\t",
        ));

    env.command()
        .args(["session", "tag", "writing-sprint", "--clear"])
        .assert()
        .success()
        .stdout(predicate::str::contains("writing-sprint\t-\n"));
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

#[test]
fn cli_edit_updates_title_and_notes() {
    let env = TestEnv::new();

    env.command()
        .args(["session", "new", "Writing Sprint"])
        .assert()
        .success();

    env.command()
        .args([
            "add",
            "Draft design spec",
            "--session",
            "writing-sprint",
            "--note",
            "cover CLI and TUI",
        ])
        .assert()
        .success();

    env.command()
        .args([
            "edit",
            "1",
            "--session",
            "writing-sprint",
            "--title",
            "Draft final design spec",
            "--clear-note",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("1\tedited\n"));

    env.command()
        .args([
            "export",
            "md",
            "writing-sprint",
            "--include-notes",
            "--timestamps",
            "none",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Draft final design spec"))
        .stdout(predicate::str::contains("cover CLI and TUI").not());
}

#[test]
fn cli_edit_requires_at_least_one_change_flag() {
    let env = TestEnv::new();

    env.command()
        .args(["session", "new", "Writing Sprint"])
        .assert()
        .success();

    env.command()
        .args(["add", "Draft design spec", "--session", "writing-sprint"])
        .assert()
        .success();

    env.command()
        .args(["edit", "1", "--session", "writing-sprint"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "edit requires --title, --note, --clear-note, --repo, or --clear-repo",
        ));
}

#[test]
fn cli_edit_rejects_conflicting_note_flags() {
    let env = TestEnv::new();

    env.command()
        .args(["edit", "1", "--note", "cover CLI and TUI", "--clear-note"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("cannot be used with"));
}

#[test]
fn cli_delete_removes_todo_and_compacts_export() {
    let env = TestEnv::new();

    env.command()
        .args(["session", "new", "Writing Sprint"])
        .assert()
        .success();

    env.command()
        .args(["add", "Draft design spec", "--session", "writing-sprint"])
        .assert()
        .success()
        .stdout(predicate::str::contains("1\n"));

    env.command()
        .args(["add", "Review bindings", "--session", "writing-sprint"])
        .assert()
        .success()
        .stdout(predicate::str::contains("2\n"));

    env.command()
        .args(["delete", "1", "--session", "writing-sprint"])
        .assert()
        .success()
        .stdout(predicate::str::contains("1\tdeleted\n"));

    env.command()
        .args(["session", "history", "writing-sprint"])
        .assert()
        .success()
        .stdout(predicate::str::contains("todo deleted"));

    env.command()
        .args(["export", "md", "writing-sprint", "--timestamps", "none"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Review bindings"))
        .stdout(predicate::str::contains("Draft design spec").not());
}

#[test]
fn cli_delete_rejects_wrong_session() {
    let env = TestEnv::new();

    env.command()
        .args(["session", "new", "Writing Sprint"])
        .assert()
        .success();
    env.command()
        .args(["session", "new", "Reading Sprint"])
        .assert()
        .success();

    env.command()
        .args(["add", "Draft design spec", "--session", "writing-sprint"])
        .assert()
        .success();

    env.command()
        .args(["delete", "1", "--session", "reading-sprint"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "todo 1 does not belong to session reading-sprint",
        ));
}

#[test]
fn cli_session_delete_removes_recent_session_and_clears_recent_pointer() {
    let env = TestEnv::new();

    env.command()
        .args(["session", "new", "Writing Sprint"])
        .assert()
        .success();

    env.command()
        .args(["session", "delete"])
        .assert()
        .success()
        .stdout(predicate::str::contains("writing-sprint\tdeleted\n"));

    env.command()
        .args(["add", "Orphan todo"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("no recent session"));
}

#[test]
fn cli_repo_search_and_repo_edit_flow() {
    let env = TestEnv::new();

    env.command()
        .args([
            "session",
            "new",
            "Writing Sprint",
            "--repo",
            "https://github.com/ExampleOrg/todui-keymove",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("writing-sprint\n"));

    env.command()
        .args(["add", "Draft spec", "--session", "writing-sprint"])
        .assert()
        .success()
        .stdout(predicate::str::contains("1\n"));

    env.command()
        .args([
            "add",
            "Review CLI",
            "--session",
            "writing-sprint",
            "--repo",
            "@OpenAI/codex",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("2\n"));

    env.command()
        .args(["repo", "exampleorg/todui-keymove"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "1\twriting-sprint\tDraft spec\topen\tagent\t-\texampleorg/todui-keymove\tsession",
        ))
        .stdout(predicate::str::contains("Review CLI").not());

    env.command()
        .args(["repo", "https://github.com/openai/codex"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "2\twriting-sprint\tReview CLI\topen\tagent\t-\topenai/codex\ttodo",
        ));

    env.command()
        .args(["edit", "2", "--session", "writing-sprint", "--clear-repo"])
        .assert()
        .success()
        .stdout(predicate::str::contains("2\tedited\n"));

    env.command()
        .args(["repo", "@exampleorg/todui-keymove"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "1\twriting-sprint\tDraft spec\topen\tagent\t-\texampleorg/todui-keymove\tsession",
        ))
        .stdout(predicate::str::contains(
            "2\twriting-sprint\tReview CLI\topen\tagent\t-\texampleorg/todui-keymove\tsession",
        ));

    env.command()
        .args(["session", "repo", "writing-sprint", "--clear"])
        .assert()
        .success()
        .stdout(predicate::str::contains("writing-sprint\t-\n"));

    env.command()
        .args(["repo", "exampleorg/todui-keymove"])
        .assert()
        .success()
        .stdout(predicate::str::is_empty());
}
