//! process-compose CLI client for listing processes and waiting for readiness.
//!
//! Uses `process-compose list --output json` against the running manager's
//! control socket. This is independent of the generic [`crate::ExternalManager`]
//! launcher, which does not speak a manager-specific protocol.

use miette::{IntoDiagnostic, Result, bail, miette};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;
use tokio::process::Command;
use tracing::{debug, warn};

use crate::config::ProcessConfig;

/// A process as reported by `process-compose list --output json`.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct ProcessComposeProcess {
    pub name: String,
    pub status: String,
    #[serde(default)]
    pub is_ready: Option<String>,
    #[serde(default)]
    pub exit_code: Option<i32>,
}

/// Query process-compose for the current process list.
pub async fn list(
    process_compose: &Path,
    socket_path: &Path,
) -> Result<Vec<ProcessComposeProcess>> {
    let output = Command::new(process_compose)
        .arg("list")
        .arg("--output")
        .arg("json")
        .env("PC_SOCKET_PATH", socket_path)
        .output()
        .await
        .into_diagnostic()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "Failed to list process-compose processes: {}",
            stderr.trim()
        );
    }

    serde_json::from_slice(&output.stdout).map_err(|e| {
        miette!(
            "Failed to parse process-compose list output: {}\n{}",
            e,
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

/// Format process-compose list output for `devenv processes list`.
pub fn format_list(processes: &[ProcessComposeProcess]) -> String {
    if processes.is_empty() {
        return "No processes found.\n".to_string();
    }

    let mut output = String::new();
    for process in processes {
        output.push_str(&format!(
            "{:<30} {:<15} ready: {}\n",
            process.name,
            process.status,
            process.is_ready.as_deref().unwrap_or("-")
        ));
    }
    output
}

/// Poll process-compose until every process is no longer pending and every
/// process with a devenv readiness probe reports Ready.
pub async fn wait_for_ready(
    process_compose: &Path,
    socket_path: &Path,
    process_configs: &HashMap<String, ProcessConfig>,
    timeout: std::time::Duration,
) -> Result<()> {
    let start = std::time::Instant::now();

    loop {
        let processes = match list(process_compose, socket_path).await {
            Ok(processes) => processes,
            Err(e) if start.elapsed() < timeout => {
                debug!(error = %e, "waiting for process-compose to be ready");
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                continue;
            }
            Err(e) => return Err(e),
        };

        let status = classify(&processes, process_configs);

        if !status.failed.is_empty() {
            warn!(
                failed = %status.failed.join(", "),
                "some process-compose processes have failed"
            );
        }

        if status.pending.is_empty() && status.not_ready.is_empty() {
            return Ok(());
        }

        if start.elapsed() >= timeout {
            let mut waiting_for = Vec::new();
            if !status.pending.is_empty() {
                waiting_for.push(format!("Pending: [{}]", status.pending.join(", ")));
            }
            if !status.not_ready.is_empty() {
                waiting_for.push(format!("Not Ready: [{}]", status.not_ready.join(", ")));
            }
            bail!(
                "Timed out waiting for process-compose processes to be ready: {}",
                waiting_for.join(" ")
            );
        }

        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
struct WaitStatus {
    pending: Vec<String>,
    not_ready: Vec<String>,
    failed: Vec<String>,
}

fn classify(
    processes: &[ProcessComposeProcess],
    process_configs: &HashMap<String, ProcessConfig>,
) -> WaitStatus {
    let mut status = WaitStatus::default();

    for process in processes {
        if process.status == "Pending" {
            status.pending.push(process.name.clone());
        }

        if process.status == "Running"
            && process.is_ready.as_deref() != Some("Ready")
            && process_configs
                .get(&process.name)
                .is_some_and(has_process_compose_probe)
        {
            status.not_ready.push(process.name.clone());
        }

        if process.status == "Exited" && process.exit_code.unwrap_or(0) != 0 {
            status.failed.push(process.name.clone());
        }
    }

    status
}

/// process-compose only gets a readiness_probe when devenv translates `ready.exec`
/// or `ready.http.get`. Other ready modes stay under native supervision.
fn has_process_compose_probe(config: &ProcessConfig) -> bool {
    config.ready.as_ref().is_some_and(|ready| {
        ready.exec.is_some() || ready.http.as_ref().is_some_and(|http| http.get.is_some())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{HttpGetProbe, HttpProbe, ReadyConfig};

    fn process(
        name: &str,
        status: &str,
        is_ready: Option<&str>,
        exit_code: Option<i32>,
    ) -> ProcessComposeProcess {
        ProcessComposeProcess {
            name: name.to_string(),
            status: status.to_string(),
            is_ready: is_ready.map(str::to_string),
            exit_code,
        }
    }

    fn exec_ready(name: &str) -> (String, ProcessConfig) {
        (
            name.to_string(),
            ProcessConfig {
                ready: Some(ReadyConfig {
                    exec: Some("true".into()),
                    ..ReadyConfig::default()
                }),
                ..ProcessConfig::default()
            },
        )
    }

    #[test]
    fn parses_process_compose_list_json() {
        let json = r#"
        [
          {
            "name": "http",
            "status": "Running",
            "is_ready": "Ready",
            "exit_code": 0
          },
          {
            "name": "worker",
            "status": "Pending"
          }
        ]
        "#;
        let processes: Vec<ProcessComposeProcess> = serde_json::from_str(json).unwrap();
        assert_eq!(
            processes,
            vec![
                process("http", "Running", Some("Ready"), Some(0)),
                process("worker", "Pending", None, None),
            ]
        );
    }

    #[test]
    fn classify_waits_for_pending_and_probed_not_ready() {
        let mut configs = HashMap::from([exec_ready("web")]);
        configs.insert("logger".into(), ProcessConfig::default());

        let status = classify(
            &[
                process("web", "Running", Some("Not Ready"), None),
                process("db", "Pending", None, None),
                process("logger", "Running", Some("Not Ready"), None),
                process("old", "Exited", None, Some(1)),
            ],
            &configs,
        );

        assert_eq!(
            status,
            WaitStatus {
                pending: vec!["db".into()],
                not_ready: vec!["web".into()],
                failed: vec!["old".into()],
            }
        );
    }

    #[test]
    fn classify_treats_http_probes_as_process_compose_readiness() {
        let configs = HashMap::from([(
            "api".to_string(),
            ProcessConfig {
                ready: Some(ReadyConfig {
                    http: Some(HttpProbe {
                        get: Some(HttpGetProbe {
                            host: "127.0.0.1".into(),
                            port: 8080,
                            path: "/".into(),
                            scheme: "http".into(),
                        }),
                    }),
                    ..ReadyConfig::default()
                }),
                ..ProcessConfig::default()
            },
        )]);

        let status = classify(
            &[process("api", "Running", Some("Not Ready"), None)],
            &configs,
        );
        assert_eq!(status.not_ready, vec!["api"]);
    }

    #[test]
    fn classify_is_ready_when_probed_processes_report_ready() {
        let configs = HashMap::from([exec_ready("web")]);
        let status = classify(&[process("web", "Running", Some("Ready"), None)], &configs);
        assert_eq!(status, WaitStatus::default());
    }

    #[test]
    fn format_list_includes_ready_column() {
        let output = format_list(&[process("http", "Running", Some("Ready"), Some(0))]);
        assert!(output.contains("http"));
        assert!(output.contains("Running"));
        assert!(output.contains("ready: Ready"));
    }

    #[test]
    fn format_list_empty() {
        assert_eq!(format_list(&[]), "No processes found.\n");
    }
}
