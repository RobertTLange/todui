use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn top_level_help_describes_main_commands() {
    Command::cargo_bin("todui")
        .expect("binary")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Manage session-based todo lists from the CLI or full-screen TUI.",
        ))
        .stdout(predicate::str::contains(
            "List sessions: `todui session list`",
        ))
        .stdout(predicate::str::contains(
            "Add a todo with kwargs: `todui add \"Draft spec\" --session writing-sprint --note \"cover CLI\"`",
        ))
        .stdout(predicate::str::contains(
            "Inspect todo titles and notes from the CLI: `todui export md writing-sprint --include-notes`",
        ))
        .stdout(predicate::str::contains(
            "There is no dedicated `todo show <id>` command; use `export md` for CLI inspection or `resume` for the TUI.",
        ))
        .stdout(predicate::str::contains(
            "Manage sessions, tags, and revision history",
        ))
        .stdout(predicate::str::contains("Add a todo to a session"))
        .stdout(predicate::str::contains("Open a session in the TUI"))
        .stdout(predicate::str::contains(
            "Export a session snapshot as markdown",
        ));
}

#[test]
fn session_help_describes_subcommands() {
    Command::cargo_bin("todui")
        .expect("binary")
        .args(["session", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Use `todui session list` to discover available session slugs before calling `add`, `resume`, or `export md`.",
        ))
        .stdout(predicate::str::contains("Create a new session"))
        .stdout(predicate::str::contains(
            "Delete a session and all of its data",
        ))
        .stdout(predicate::str::contains(
            "List sessions with tags, timestamps, and revision numbers",
        ))
        .stdout(predicate::str::contains(
            "Print the session revision history",
        ))
        .stdout(predicate::str::contains(
            "Set or clear the grouping tag for a session",
        ));
}

#[test]
fn session_list_help_explains_output_shape() {
    Command::cargo_bin("todui")
        .expect("binary")
        .args(["session", "list", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Print one session per line in a tab-separated format for scripts and agents.",
        ))
        .stdout(predicate::str::contains(
            "<slug>\\t<display-name>\\t<tag-or->\\t<last-opened-local-time>\\tr<current-revision>",
        ));
}

#[test]
fn add_help_explains_stdout_and_kwargs() {
    Command::cargo_bin("todui")
        .expect("binary")
        .args(["add", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "On success, stdout contains the new todo id as a single integer.",
        ))
        .stdout(predicate::str::contains(
            "todui add \"Review keybindings\" --session writing-sprint --note \"Ghostty + mouse\"",
        ));
}

#[test]
fn export_markdown_help_explains_defaults_and_flags() {
    Command::cargo_bin("todui")
        .expect("binary")
        .args(["export", "md", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Use this command when an agent needs todo titles or note bodies from the CLI.",
        ))
        .stdout(predicate::str::contains(
            "Session slug. Defaults to the most recently opened session",
        ))
        .stdout(predicate::str::contains(
            "Export a specific revision instead of the live head",
        ))
        .stdout(predicate::str::contains(
            "Write output to a file instead of stdout",
        ))
        .stdout(predicate::str::contains("Include todo notes in the export"))
        .stdout(predicate::str::contains(
            "Include only open todos in the export",
        ))
        .stdout(predicate::str::contains(
            "todui export md writing-sprint --include-notes",
        ));
}
