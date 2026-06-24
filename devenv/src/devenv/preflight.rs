//! Pre-flight commands.
//!
//! Runs user-defined commands during devenv startup, before subsystems
//! that read `env::var()` (cachix, substituter netrc, …) initialize.
//! Stdout in `KEY=value` form is merged into the process environment.

use std::collections::BTreeMap;

use devenv_activity::{Activity, ActivityLevel, start};
use devenv_nix_backend::NixCBackend;
use miette::{IntoDiagnostic, Result, WrapErr};
use tracing::{debug, warn};

#[derive(serde::Deserialize)]
struct PreFlight {
    command: String,
}

pub async fn run(cnix: &NixCBackend) -> Result<()> {
    let activity =
        start!(Activity::evaluate("Reading config.devenv.preFlight").level(ActivityLevel::Debug));
    let json = match cnix.eval_attr("config.devenv.preFlight", &activity).await {
        Ok(j) => j,
        Err(_) => return Ok(()),
    };
    let entries: BTreeMap<String, PreFlight> = serde_json::from_str(&json)
        .into_diagnostic()
        .wrap_err("Failed to deserialize config.devenv.preFlight")?;

    if entries.is_empty() {
        return Ok(());
    }

    for (name, pf) in &entries {
        debug!(name = %name, "running preFlight command");
        let output = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(&pf.command)
            .output()
            .await
            .into_diagnostic()
            .wrap_err_with(|| format!("preFlight `{name}` failed to spawn"))?;

        if !output.status.success() {
            warn!(
                name = %name,
                status = %output.status,
                stderr = %String::from_utf8_lossy(&output.stderr).trim(),
                "preFlight command failed",
            );
            continue;
        }

        for line in String::from_utf8_lossy(&output.stdout).lines() {
            let Some((key, val)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim();
            let val = val.trim();
            if key.is_empty() {
                continue;
            }
            // SAFETY: pre-flight runs before subsystems that read env::var().
            // The point of this hook is to mutate process env.
            unsafe { std::env::set_var(key, val) };
        }
    }

    Ok(())
}
