//! Regression tests for rendering `devenv hook <shell>`.

use std::process::Command;

#[test]
fn hook_print_does_not_panic_on_broken_pipe() {
    let bin = env!("CARGO_BIN_EXE_devenv");

    for shell in ["bash", "zsh", "fish", "nu"] {
        // `pipefail` surfaces the writer's exit status instead of `true`'s.
        let output = Command::new("bash")
            .arg("-c")
            .arg(format!("set -o pipefail; {bin:?} hook {shell} | true"))
            .output()
            .expect("spawn bash");

        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!stderr.contains("panicked"), "hook {shell}:\n{stderr}");
        assert!(
            output.status.success(),
            "hook {shell}: {:?}\n{stderr}",
            output.status
        );
    }
}

#[test]
fn hook_forwards_shell_args_to_spawned_shell() {
    let bin = env!("CARGO_BIN_EXE_devenv");

    for shell in ["bash", "zsh", "fish", "nu"] {
        let output = Command::new(bin)
            .args(["hook", shell, "--", "--no-tui"])
            .output()
            .expect("run devenv hook");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        assert!(
            output.status.success(),
            "hook {shell}: {:?}\n{stderr}",
            output.status
        );
        let expected = match shell {
            "nu" => "devenv shell \"--no-tui\"",
            "fish" => "devenv shell '--no-tui'",
            _ => "devenv shell --no-tui",
        };
        assert!(
            stdout.contains(expected),
            "hook {shell} did not forward --no-tui:\n{stdout}"
        );
    }
}

#[test]
fn hook_requires_separator_before_forwarded_args() {
    let output = Command::new(env!("CARGO_BIN_EXE_devenv"))
        .args(["hook", "fish", "--no-tui"])
        .output()
        .expect("run devenv hook");

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("must follow `--`"));
}

#[test]
fn hook_does_not_reparse_forwarded_args() {
    let output = Command::new(env!("CARGO_BIN_EXE_devenv"))
        .args([
            "hook",
            "fish",
            "--",
            "--option-added-by-a-future-version",
            "--help",
            "echo",
        ])
        .output()
        .expect("run devenv hook");

    assert!(
        output.status.success(),
        "forwarded arguments were rejected: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--option-added-by-a-future-version"));
    assert!(stdout.contains("--help"));
    assert!(stdout.contains("echo"));
}
