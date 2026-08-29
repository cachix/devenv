mod files;

use crate::{TaskBuiltin, types::Output};
use miette::Result;
use std::collections::BTreeMap;
use tokio_util::sync::CancellationToken;

pub const FILES_RECONCILE: &str = "files-reconcile";
pub const FILES_RECONCILE_CAPABILITY: &str = "task-builtin.files-reconcile";

/// Exact protocol versions implemented by this task runner.
#[derive(serde::Serialize)]
pub struct CapabilityVersions {
    pub versions: Vec<u32>,
}

pub fn capabilities() -> BTreeMap<String, CapabilityVersions> {
    BTreeMap::from([(
        FILES_RECONCILE_CAPABILITY.to_string(),
        CapabilityVersions { versions: vec![1] },
    )])
}

/// Execute a supported builtin. `None` means the envelope is unknown and the
/// caller must use the task's ordinary command for backwards compatibility.
pub async fn execute(
    builtin: &TaskBuiltin,
    cancellation: CancellationToken,
) -> Result<Option<Output>> {
    match (builtin.name.as_str(), builtin.version) {
        (FILES_RECONCILE, 1) => files::execute_v1(builtin.input.clone(), cancellation)
            .await
            .map(Some),
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advertises_exact_supported_versions() {
        let capabilities = capabilities();
        assert_eq!(capabilities[FILES_RECONCILE_CAPABILITY].versions, vec![1]);
    }

    #[tokio::test]
    async fn unsupported_builtin_or_version_falls_back() {
        for builtin in [
            TaskBuiltin {
                name: "unknown".to_string(),
                version: 1,
                input: serde_json::Value::Null,
            },
            TaskBuiltin {
                name: FILES_RECONCILE.to_string(),
                version: 2,
                input: serde_json::Value::Null,
            },
        ] {
            assert!(
                execute(&builtin, CancellationToken::new())
                    .await
                    .unwrap()
                    .is_none()
            );
        }
    }

    #[tokio::test]
    async fn malformed_supported_payload_is_an_error() {
        let builtin = TaskBuiltin {
            name: FILES_RECONCILE.to_string(),
            version: 1,
            input: serde_json::json!({"root": 42}),
        };

        let error = execute(&builtin, CancellationToken::new())
            .await
            .unwrap_err();
        assert!(format!("{error:#}").contains("invalid files-reconcile v1 input"));
    }
}
