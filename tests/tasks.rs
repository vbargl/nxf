//! Tests for src/tasks.rs: planning and running tasks, end to end.

mod common;

use common::{
    append, assert_contains, assert_lacks, commit, events, fake_systems, fixture, hostname,
    write_script,
};
use nxf::output::{Style, RESET};
use nxf::tasks;
use serde_json::{json, Value};

fn words(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| word.to_string()).collect()
}

fn plan(given: &[&str]) -> Result<tasks::Plan, String> {
    tasks::plan(&words(given), &words(&["a", "b"]))
}

/// Tasks are defined in the order they run in.
#[test]
fn discovery() {
    let defined = tasks::defined();
    let names: Vec<&str> = defined.iter().map(|task| task.name).collect();
    assert_eq!(
        names,
        ["update", "build", "test", "boot", "switch", "upgrade"]
    );
    let takers: Vec<&str> = defined
        .iter()
        .filter(|task| task.takes_inputs)
        .map(|task| task.name)
        .collect();
    assert_eq!(takers, ["update", "upgrade"]);
    assert!(defined.iter().all(|task| !task.description.is_empty()));
}

/// Plan runs tasks in definition order, once.
#[test]
fn plan_order() {
    let plan = plan(&["test", "update", "test"]).unwrap();
    assert_eq!(plan.tasks, ["update", "test"]);
    assert!(plan.inputs.is_empty());
}

/// Shortcuts expand into tasks that keep their own places in the order.
#[test]
fn plan_shortcut() {
    assert_eq!(
        plan(&["upgrade", "build", "test"]).unwrap().tasks,
        ["update", "build", "test", "switch"]
    );
    assert_eq!(
        plan(&["switch", "upgrade", "update"]).unwrap().tasks,
        ["update", "switch"]
    );
    let defined = tasks::defined();
    let upgrade = defined.iter().find(|task| task.name == "upgrade").unwrap();
    assert_eq!(upgrade.expands, ["update", "switch"]);
    assert!(upgrade.takes_inputs && upgrade.shortcut);
}

/// Plan routes inputs to update.
#[test]
fn plan_inputs() {
    let plan = plan(&["test", "b", "update", "a", "b"]).unwrap();
    assert_eq!(plan.tasks, ["update", "test"]);
    assert_eq!(plan.inputs, ["b", "a"]);
}

/// Plan rejects unknown words.
#[test]
fn plan_unknown() {
    assert_contains(
        &plan(&["update", "status"]).unwrap_err(),
        "Unknown task or input: status",
    );
}

/// Plan rejects inputs without update.
#[test]
fn plan_inputs_need_update() {
    assert_contains(
        &plan(&["a", "test"]).unwrap_err(),
        "need the update or upgrade task",
    );
}

/// Update changes only the named input, builds with it and commits the lock.
#[test]
fn update_named_input() {
    let fx = fixture();
    let (a, b) = (fx.locked("a"), fx.locked("b"));
    fx.bump(&["a", "b"]);
    let result = fx.run(&["update", "a"]);
    result.ok();
    assert_ne!(fx.locked("a"), a);
    assert_eq!(fx.locked("b"), b);
    assert_eq!(
        common::git(&fx.checkout, &["log", "-1", "--format=%s"]),
        "flake.lock: update a"
    );
    assert_eq!(fx.porcelain(), "");
    let out = &result.stdout;
    // One row for the input: its state, its name in the label column, the change.
    assert_contains(
        out,
        &format!(
            "\n  ✓ a         {} → {}\n    Commit    ",
            &a[..12],
            fx.short("a")
        ),
    );
    assert_lacks(out, "b         ");
    assert_lacks(out, "@nix ");
    assert_lacks(out, "warning: updating lock file");
    assert_lacks(out, "Checking input a");
}

/// An unchanged update says so and creates no commit.
#[test]
fn update_unchanged() {
    let fx = fixture();
    let before = fx.head();
    let result = fx.run(&["update"]);
    result.ok();
    assert_contains(&result.stdout, "✓ All selected inputs are up to date");
    assert_contains(&result.stdout, "No lockfile commit was needed.");
    assert_contains(
        &result.stdout,
        &format!(
            "Checking input updates\n  · a         {}   unchanged\n  · b         {}   unchanged\n\n✓ All",
            fx.short("a"),
            fx.short("b")
        ),
    );
    assert_eq!(fx.head(), before);
}

/// On a terminal the input in progress is shown and then replaced by its
/// one row, so the finished screen has one row for each input.
#[test]
fn input_results_terminal() {
    let mut fx = fixture();
    fx.unset("NO_COLOR");
    fx.set("TERM", "xterm");
    let result = fx.terminal(100, &["update"]);
    result.ok();
    assert_contains(&result.stdout, "… Checking input a");
    let mut parser = vt100::Parser::new(60, 100, 0);
    parser.process(result.stdout.as_bytes());
    let screen = parser.screen().contents();
    let rows: Vec<&str> = screen.lines().map(str::trim_end).collect();
    assert!(!screen.contains("Checking input a"), "{screen}");
    for input in ["a", "b"] {
        let expected = format!("  · {input}         {}   unchanged", fx.short(input));
        assert_eq!(
            rows.iter().filter(|row| **row == expected).count(),
            1,
            "{screen}"
        );
    }
}

/// Newly declared inputs are locked and committed with the first update.
#[test]
fn new_input() {
    let fx = fixture();
    let file = fx.checkout.join("flake.nix");
    let text = std::fs::read_to_string(&file).unwrap().replace(
        "outputs = _",
        &format!(
            "inputs.c = {{ url = \"git+file://{}/a\"; flake = false; }}; outputs = _",
            fx.dir.display()
        ),
    );
    std::fs::write(&file, text).unwrap();
    commit(&fx.checkout, "add c");
    fx.run(&["update"]).ok();
    let lock: Value =
        serde_json::from_str(&std::fs::read_to_string(fx.checkout.join("flake.lock")).unwrap())
            .unwrap();
    assert!(lock["nodes"].get("c").is_some());
    assert_eq!(fx.porcelain(), "");
}

/// Tasks run in definition order whatever the given order.
#[test]
fn order() {
    let fx = fixture().with("NXF_TEST_BUILD", &fake_systems().one);
    fx.bump(&["a"]);
    fx.run(&["test", "update"]).ok();
    // update built before committing; test then saw the commit, so update ran first.
    assert_eq!(fx.nh_actions(), ["build", "build", "test"]);
    let last = fx.nh_calls().pop().unwrap();
    assert!(last.ends_with(&format!("@ {}", fx.short_head())), "{last}");
}

/// Upgrade updates named inputs, then switches.
#[test]
fn upgrade() {
    let fx = fixture().with("NXF_TEST_BUILD", &fake_systems().one);
    let b = fx.locked("b");
    fx.bump(&["a", "b"]);
    let result = fx.run(&["upgrade", "a"]);
    result.ok();
    assert_eq!(fx.locked("b"), b);
    assert_eq!(fx.nh_actions(), ["build", "build", "switch"]);
    assert_contains(&result.stdout, fx.checkout.to_str().unwrap());
    assert_contains(&result.stdout, "✓ Upgrade applied");
    let phases: Vec<&str> = result
        .stdout
        .lines()
        .filter(|line| line.starts_with('['))
        .collect();
    assert_eq!(
        phases,
        [
            "[1/3] Checking input updates",
            "[2/3] Building configuration",
            "[3/3] Activating configuration"
        ]
    );
}

/// Every activation action reports uncertainty and keeps its successful build.
#[test]
fn activation_failures() {
    let system = &fake_systems().one;
    for action in ["test", "boot", "switch"] {
        let fx = fixture()
            .with("NXF_TEST_BUILD", system)
            .with("NXF_TEST_NH_FAIL_ACTION", action);
        let result = fx.run(&[action]);
        assert_ne!(result.code, 0);
        assert_eq!(fx.nh_calls().len(), 2);
        assert_contains(&result.stdout, "  ✓ Built\n");
        assert!(!result.stdout.lines().any(|line| line.starts_with("✓ ")));
        assert_lacks(&result.stderr, "Task failed");
        assert_contains(
            &result.stderr,
            if action == "boot" {
                "verify boot entries before rebooting"
            } else {
                "Some changes may already be active."
            },
        );
        assert_eq!(
            &std::fs::canonicalize(fx.systems().join(fx.tree())).unwrap(),
            system
        );
    }
}

/// An upgrade failure distinguishes committed updates from an unstarted activation.
#[test]
fn upgrade_build_failure() {
    let fx = fixture()
        .with("NXF_TEST_BUILD", &fake_systems().one)
        .with("NXF_TEST_NH_FAIL_ACTION", "build");
    let old = fx.locked("a");
    fx.bump(&["a"]);
    let result = fx.run(&["upgrade", "a"]);
    assert_ne!(result.code, 0);
    assert_eq!(fx.locked("a"), old);
    assert_eq!(fx.porcelain(), "");
    assert_contains(&result.stderr, "Upgrade stopped: input update failed");
    assert_contains(&result.stderr, "The lockfile was restored");
    assert_contains(&result.stderr, "    nxf update a");
    assert_lacks(&result.stdout, "Activating configuration");
    assert_lacks(&result.stdout, "Upgrade applied");
    // The cause comes first and names its input.
    assert_contains(&result.stderr, "failed\n  a: Command exited with status 3.");
}

/// A no-change upgrade still reports the activation it performed.
#[test]
fn upgrade_unchanged() {
    let system = &fake_systems().one;
    let fx = fixture()
        .with("NXF_TEST_BUILD", system)
        .with("NXF_SYSTEM", system);
    let result = fx.run(&["upgrade"]);
    result.ok();
    assert_contains(&result.stdout, "   unchanged\n");
    assert_contains(&result.stdout, "  ✓ Built, system unchanged");
    assert_contains(&result.stdout, "activation was still performed");
    assert_contains(&result.stdout, "✓ Upgrade applied");
}

/// Raw diagnostics stay hidden on success and cannot corrupt the system path.
#[test]
fn diagnostics() {
    let system = &fake_systems().one;
    let fx = fixture()
        .with("NXF_TEST_BUILD", system)
        .with("NXF_TEST_NH_STDOUT", "build progress")
        .with("NXF_TEST_NH_STDERR", "build diagnostic");
    let result = fx.run(&["build"]);
    result.ok();
    assert_lacks(&result.stdout, "build progress");
    assert_lacks(&result.stdout, "build diagnostic");
    assert_contains(
        &result.stdout,
        &format!("    System    {}", system.display()),
    );
    assert_contains(&result.stdout, "✓ Configuration built");
}

/// Carriage-return chatter and builder logs cannot leak into concise progress.
#[test]
fn concise_progress() {
    let noise = format!(
        "remote: Counting objects: 1%\rremote: Counting objects: 100%\n{}",
        events(&[
            json!({"action": "start", "type": 104, "id": 10}),
            json!({"action": "result", "type": 105, "id": 10, "fields": [3, 8, 1, 0]}),
            json!({"action": "result", "type": 105, "id": 10, "fields": [3, 8, 1, 0]}),
            json!({"action": "start", "type": 105, "id": 11, "fields": ["/nix/store/00000000000000000000000000000000-example-package.drv", "", 1, 1]}),
            json!({"action": "result", "type": 101, "id": 11, "fields": ["raw compiler output"]}),
            json!({"action": "msg", "level": 0, "msg": "error: example compilation failed"}),
        ])
    );
    let mut fx = fixture().with("NXF_TEST_BUILD", &fake_systems().one);
    fx.env.insert("NXF_TEST_NH_STDOUT".to_string(), noise);
    fx.set("NXF_TEST_NH_STDERR", "raw nh diagnostic");
    let result = fx.run(&["build"]);
    result.ok();
    assert_contains(&result.stdout, "    3 of 8 derivations built");
    assert_contains(&result.stdout, "    Building example-package");
    assert_eq!(result.stdout.matches("3 of 8 derivations built").count(), 1);
    for raw in [
        "remote:",
        "@nix ",
        "raw compiler output",
        "raw nh diagnostic",
        "error: example compilation failed",
        "\r",
    ] {
        assert_lacks(&result.stdout, raw);
    }
    fx.set("NXF_TEST_NH_FAIL_ACTION", "build");
    let failed = fx.run(&["build"]);
    assert_ne!(failed.code, 0);
    assert_contains(&failed.stderr, "error: example compilation failed");
    assert_contains(&failed.stderr, "raw compiler output");
    assert_lacks(&failed.stderr, "@nix ");
    assert_lacks(&failed.stderr, "\r");
}

/// Styled output retains hierarchy, while NO_COLOR disables all styles.
#[test]
fn terminal_color() {
    let mut fx = fixture().with("NXF_TEST_BUILD", &fake_systems().one);
    fx.unset("NO_COLOR");
    fx.set("CLICOLOR_FORCE", "1");
    let styled = |text: &str, style: Style| format!("{}{text}{RESET}", style.code());
    let result = fx.run(&["upgrade"]);
    result.ok();
    assert_contains(
        &result.stdout,
        &styled("[1/3] Checking input updates", Style::CyanBold),
    );
    assert_contains(
        &result.stdout,
        &styled("✓ Upgrade applied", Style::GreenBold),
    );
    assert_contains(
        &result.stdout,
        &format!("    {}System", Style::Dimmed.code()),
    );
    assert_contains(
        &result.stdout,
        &format!("  {} Built", styled("✓", Style::Green)),
    );
    fx.set("NXF_TEST_NH_FAIL_ACTION", "switch");
    let failed = fx.run(&["switch"]);
    assert_ne!(failed.code, 0);
    assert_contains(
        &failed.stderr,
        &styled("✗ Activation did not finish successfully", Style::RedBold),
    );
    assert_contains(
        &failed.stderr,
        &format!("    {}", styled("nxf switch", Style::Bold)),
    );
    assert_lacks(&failed.stderr, "Task failed");
    fx.unset("NXF_TEST_NH_FAIL_ACTION");
    fx.set("NO_COLOR", "");
    let plain = fx.run(&["upgrade"]);
    plain.ok();
    assert_eq!(plain.stdout, nxf::text::strip_ansi(&plain.stdout));
    assert_contains(&plain.stdout, "\n\n[2/3] Building configuration\n");
}

/// Every nh task announces its purpose and reports its resulting system.
#[test]
fn progress() {
    let system = &fake_systems().one;
    let fx = fixture().with("NXF_TEST_BUILD", system);
    for (task, outcome) in [
        ("build", "✓ Configuration built"),
        ("test", "✓ Configuration active until reboot"),
        ("boot", "✓ Next boot configured"),
        ("switch", "✓ Configuration activated"),
    ] {
        let result = fx.run(&[task]);
        result.ok();
        assert_contains(&result.stdout, "Building configuration\n  ✓ Built\n");
        assert_contains(&result.stdout, &format!("{} · ", hostname()));
        assert_contains(
            &result.stdout,
            &format!("    System    {}", system.display()),
        );
        assert_contains(&result.stdout, outcome);
        let calls = fx.nh_calls();
        let ran = if task == "build" { 1 } else { 2 };
        let calls = &calls[calls.len() - ran..];
        assert_contains(&calls[0], &format!("os build {}#", fx.checkout.display()));
        assert_contains(&calls[0], "--log-format internal-json");
        assert_contains(&calls[0], "--print-build-logs");
        if task != "build" {
            assert_contains(&calls[1], &format!("os {task} {} ", system.display()));
        }
    }
}

/// A failing task stops later tasks.
#[test]
fn failure_stops() {
    let fx = fixture()
        .with("NXF_TEST_NH_STATUS", "3")
        .with("NXF_TEST_BUILD", &fake_systems().one);
    let result = fx.run(&["build", "test"]);
    assert_ne!(result.code, 0);
    assert_eq!(fx.nh_actions(), ["build"]);
    assert_contains(&result.stdout, "Building configuration");
    assert_contains(&result.stderr, "✗ Configuration build failed");
    assert_contains(&result.stderr, "Activation was not started.");
    assert_lacks(&result.stdout, "✓ Built");
    assert_lacks(&result.stdout, "Activating configuration");
}

/// Unknown tasks run nothing.
#[test]
fn unknown_task() {
    let fx = fixture();
    let result = fx.run(&["test", "status"]);
    assert_ne!(result.code, 0);
    assert_contains(&result.stderr, "Unknown task or input: status");
    assert!(fx.nh_calls().is_empty());
}

/// Dry run prints tasks with their inputs.
#[test]
fn dry_run() {
    let fx = fixture();
    let result = fx.run(&["--dry-run", "switch", "b", "update"]);
    result.ok();
    assert_eq!(
        result.stdout.lines().collect::<Vec<_>>(),
        ["update b", "switch"]
    );
    assert!(fx.nh_calls().is_empty());
}

/// Builds record the committed tree.
#[test]
fn build_record() {
    let system = &fake_systems().one;
    let fx = fixture().with("NXF_TEST_BUILD", system);
    fx.run(&["build"]).ok();
    assert_eq!(
        &std::fs::canonicalize(fx.systems().join(fx.tree())).unwrap(),
        system
    );
}

/// Builds with uncommitted changes are not recorded.
#[test]
fn dirty_build() {
    let fx = fixture().with("NXF_TEST_BUILD", &fake_systems().one);
    append(&fx.checkout.join("flake.nix"), "# dirty\n");
    let result = fx.run(&["build"]);
    result.ok();
    assert_contains(&result.stderr, "uncommitted changes");
    let recorded = std::fs::read_dir(fx.systems()).map_or(0, |entries| entries.count());
    assert_eq!(recorded, 0);
}

/// Update restores a lockfile left uncommitted, which was never built.
#[test]
fn update_restores_leftover_lock() {
    let fx = fixture();
    let file = fx.checkout.join("flake.lock");
    let original = std::fs::read_to_string(&file).unwrap();
    std::fs::write(&file, original.replace("\"version\": 7", "\"version\": 7 ")).unwrap();
    assert_ne!(std::fs::read_to_string(&file).unwrap(), original);
    let result = fx.run(&["update"]);
    result.ok();
    assert_contains(&result.stderr, "they were restored");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), original);
}

/// A failed lockfile commit restores the lockfile, and nothing runs after it.
#[test]
fn update_restores_uncommitted_lock() {
    let fx = fixture().with("NXF_TEST_BUILD", &fake_systems().one);
    fx.bump(&["a"]);
    write_script(
        &fx.checkout.join(".git/hooks/pre-commit"),
        "#!/bin/sh\nexit 1\n",
    );
    let before = fx.locked("a");
    let result = fx.run(&["update", "a", "test"]);
    assert_ne!(result.code, 0);
    assert_contains(&result.stderr, "restored");
    assert_contains(&result.stderr, "✗ Input update failed");
    assert_lacks(&result.stdout, "Commit    ");
    assert_eq!(fx.locked("a"), before);
    assert_eq!(fx.porcelain(), "");
    assert_eq!(fx.nh_actions(), ["build"]);
}

/// With --json each task that ran reports its outcome, and nothing else is
/// printed; a failure is reported with its diagnostics and stops later tasks.
#[test]
fn json() {
    let mut fx = fixture().with("NXF_TEST_BUILD", &fake_systems().one);
    let unchanged = fx.run(&["--json", "update", "a", "build"]);
    unchanged.ok();
    assert_eq!(unchanged.stderr, "");
    let results: Value = serde_json::from_str(&unchanged.stdout).unwrap();
    assert_eq!(
        results,
        json!([
            {"task": "update", "inputs": ["a"], "outcome": "unchanged", "detail": ""},
            {"task": "build", "inputs": [], "outcome": "done", "detail": ""},
        ])
    );
    fx.bump(&["a"]);
    let old = fx.locked("a");
    fx.set("NXF_TEST_NH_FAIL_ACTION", "build");
    let failed = fx.run(&["--json", "update", "a", "switch"]);
    assert_eq!(failed.code, 1);
    let results: Value = serde_json::from_str(&failed.stdout).unwrap();
    let results = results.as_array().unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["outcome"], "failed");
    assert_contains(
        results[0]["detail"].as_str().unwrap(),
        "Input update failed",
    );
    assert_eq!(fx.locked("a"), old);
}
