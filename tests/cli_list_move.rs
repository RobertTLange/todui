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

    fn list_json(&self, extra: &[&str]) -> Value {
        let mut args = vec!["list", "--json"];
        args.extend_from_slice(extra);
        serde_json::from_str(&self.run(&args)).expect("valid json")
    }

    fn titles(&self, session: &str) -> Vec<String> {
        self.list_json(&["--session", session])["todos"]
            .as_array()
            .expect("todos")
            .iter()
            .map(|todo| todo["title"].as_str().expect("title").to_string())
            .collect()
    }
}

fn seed(env: &TestEnv) {
    env.run(&[
        "session",
        "new",
        "Work",
        "--tag",
        "work",
        "--repo",
        "@acme/api",
    ]);
    env.run(&["session", "new", "Home"]);
    for title in ["first", "second", "third"] {
        env.run(&["add", title, "--session", "work", "--human"]);
    }
    env.run(&["add", "groceries", "--session", "home", "--note", "milk"]);
    env.run(&["done", "1", "--session", "work", "--human"]);
}

#[test]
fn list_json_reports_sessions_and_todos_with_ids() {
    let env = TestEnv::new();
    seed(&env);

    let listing = env.list_json(&[]);
    let sessions = listing["sessions"].as_array().expect("sessions");
    let work = sessions
        .iter()
        .find(|s| s["slug"] == "work")
        .expect("work session");
    assert_eq!(work["tag"], "work");
    assert_eq!(work["repo"], "acme/api");
    assert_eq!(work["openCount"], 2);
    assert_eq!(work["doneCount"], 1);
    assert!(work["lastOpenedAt"].is_i64());
    assert!(work["revision"].is_u64());

    let todos = listing["todos"].as_array().expect("todos");
    assert_eq!(todos.len(), 4);
    let first = todos.iter().find(|t| t["id"] == 1).expect("todo 1");
    assert_eq!(first["session"], "work");
    assert_eq!(first["status"], "done");
    assert_eq!(first["position"], 1);
    assert_eq!(first["createdBy"], "human");
    assert_eq!(first["completedBy"], "human");
    assert!(first["completedAt"].is_i64());
    let groceries = todos
        .iter()
        .find(|t| t["title"] == "groceries")
        .expect("groceries");
    assert_eq!(groceries["notes"], "milk");
    assert_eq!(groceries["createdBy"], "agent");
    assert!(groceries["completedAt"].is_null());
}

#[test]
fn list_json_filters_by_session_and_open_status() {
    let env = TestEnv::new();
    seed(&env);

    let work = env.list_json(&["--session", "work"]);
    assert!(
        work["todos"]
            .as_array()
            .unwrap()
            .iter()
            .all(|t| t["session"] == "work")
    );
    assert_eq!(
        work["sessions"].as_array().unwrap().len(),
        2,
        "sessions stay complete for pickers"
    );

    let open = env.list_json(&["--session", "work", "--open-only"]);
    let titles: Vec<_> = open["todos"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["title"].clone())
        .collect();
    assert_eq!(titles, vec!["second", "third"]);

    env.command()
        .args(["list", "--json", "--session", "nope"])
        .assert()
        .failure();
}

#[test]
fn list_without_json_prints_tab_separated_rows() {
    let env = TestEnv::new();
    seed(&env);
    let output = env.run(&["list", "--session", "work"]);
    assert_eq!(output.lines().next(), Some("1\twork\tdone\t1\tfirst"));
}

#[test]
fn listing_does_not_change_the_last_opened_session() {
    let env = TestEnv::new();
    seed(&env);
    let before = env.list_json(&[]);
    env.list_json(&["--session", "home"]);
    assert_eq!(env.list_json(&[])["sessions"], before["sessions"]);
}

#[test]
fn move_reorders_within_the_session_and_clamps() {
    let env = TestEnv::new();
    seed(&env);

    assert_eq!(
        env.run(&["move", "3", "--to", "1", "--session", "work"]),
        "3\t1\n"
    );
    assert_eq!(env.titles("work"), vec!["third", "first", "second"]);

    env.run(&["move", "3", "--to", "3", "--session", "work"]);
    assert_eq!(env.titles("work"), vec!["first", "second", "third"]);

    assert_eq!(
        env.run(&["move", "1", "--to", "99", "--session", "work"]),
        "1\t3\n"
    );
    assert_eq!(env.titles("work"), vec!["second", "third", "first"]);
    env.run(&["move", "1", "--to", "0", "--session", "work"]);
    assert_eq!(env.titles("work"), vec!["first", "second", "third"]);

    let positions: Vec<_> = env.list_json(&["--session", "work"])["todos"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["position"].as_i64().unwrap())
        .collect();
    assert_eq!(positions, vec![1, 2, 3]);

    env.command()
        .args(["move", "1", "--to", "2", "--session", "home"])
        .assert()
        .failure();
}

#[test]
fn move_records_a_revision() {
    let env = TestEnv::new();
    seed(&env);
    let before = env.list_json(&["--session", "work"])["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["slug"] == "work")
        .unwrap()["revision"]
        .as_u64()
        .unwrap();
    env.run(&["move", "2", "--to", "1", "--session", "work"]);
    let history = env.run(&["session", "history", "work"]);
    assert!(history.contains("todo moved"));
    let after = env.list_json(&[])["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["slug"] == "work")
        .unwrap()["revision"]
        .as_u64()
        .unwrap();
    assert_eq!(after, before + 1);
}
