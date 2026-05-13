//! Regression: `devenv hook <shell>` must not panic when its stdout reader
//! closes the pipe early (e.g. `devenv hook fish | source`).

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
