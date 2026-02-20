use capctl::Cap;
use std::str::FromStr;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CapError {
    #[error("unknown capability: {0}")]
    Unknown(String),
    #[error("capability not in allowlist: {0}")]
    NotAllowed(String),
}

/// Information about a capability for display in the TUI.
pub struct CapInfo {
    pub cap: Cap,
    pub name: &'static str,
    pub description: &'static str,
}

/// Curated set of capabilities that devenv is willing to grant.
///
/// This is the hard security boundary. No matter what a project config says,
/// capabilities outside this list will never be granted.
const ALLOWED: &[CapInfo] = &[
    CapInfo {
        cap: Cap::NET_BIND_SERVICE,
        name: "net_bind_service",
        description: "Bind to TCP/UDP ports below 1024",
    },
    CapInfo {
        cap: Cap::NET_RAW,
        name: "net_raw",
        description: "Use raw and packet sockets (e.g. ping, tcpdump)",
    },
    CapInfo {
        cap: Cap::NET_ADMIN,
        name: "net_admin",
        description: "Configure network interfaces and routing",
    },
    CapInfo {
        cap: Cap::IPC_LOCK,
        name: "ipc_lock",
        description: "Lock memory (e.g. PostgreSQL huge pages, mlock)",
    },
    CapInfo {
        cap: Cap::SYS_NICE,
        name: "sys_nice",
        description: "Set real-time scheduling priority",
    },
    CapInfo {
        cap: Cap::SYS_RESOURCE,
        name: "sys_resource",
        description: "Override resource limits (e.g. open file descriptors)",
    },
    CapInfo {
        cap: Cap::SYS_ADMIN,
        name: "sys_admin",
        description: "Perform system administration operations (e.g. mount, namespaces)",
    },
    CapInfo {
        cap: Cap::CHOWN,
        name: "chown",
        description: "Change file ownership",
    },
    CapInfo {
        cap: Cap::DAC_OVERRIDE,
        name: "dac_override",
        description: "Bypass file read, write, and execute permission checks",
    },
    CapInfo {
        cap: Cap::FOWNER,
        name: "fowner",
        description: "Bypass permission checks requiring file owner match",
    },
];

/// Parse a capability name (without `cap_` prefix) into a `Cap`.
pub fn parse_cap(name: &str) -> Result<Cap, CapError> {
    let normalized = name.to_lowercase().replace('-', "_");
    let with_prefix = format!("cap_{normalized}");

    // Cap::from_str accepts "cap_..." in any case.
    Cap::from_str(&with_prefix).map_err(|_| CapError::Unknown(name.to_string()))
}

/// Parse and validate a list of capability names against the allowlist.
pub fn parse_and_validate(names: &[String]) -> Result<Vec<Cap>, CapError> {
    let mut caps = Vec::with_capacity(names.len());
    for name in names {
        let cap = parse_cap(name)?;
        if !is_allowed(cap) {
            return Err(CapError::NotAllowed(name.clone()));
        }
        caps.push(cap);
    }
    Ok(caps)
}

/// Check if a capability is in the allowlist.
pub fn is_allowed(cap: Cap) -> bool {
    ALLOWED.iter().any(|info| info.cap == cap)
}

/// Get human-readable info for a capability (for TUI display).
///
/// Accepts names with or without `cap_`/`CAP_` prefix (e.g., both
/// `"net_bind_service"` and `"CAP_NET_BIND_SERVICE"` work).
pub fn info_for(name: &str) -> Option<&'static CapInfo> {
    let normalized = name.to_lowercase().replace('-', "_");
    let stripped = normalized.strip_prefix("cap_").unwrap_or(&normalized);
    ALLOWED.iter().find(|info| info.name == stripped)
}

/// List all allowed capabilities with descriptions.
pub fn allowed_caps() -> &'static [CapInfo] {
    ALLOWED
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_known_cap() {
        let cap = parse_cap("net_bind_service").unwrap();
        assert_eq!(cap, Cap::NET_BIND_SERVICE);
    }

    #[test]
    fn parse_with_dashes() {
        let cap = parse_cap("net-bind-service").unwrap();
        assert_eq!(cap, Cap::NET_BIND_SERVICE);
    }

    #[test]
    fn reject_unknown() {
        assert!(parse_cap("not_a_real_cap").is_err());
    }

    #[test]
    fn reject_disallowed() {
        let names = vec!["sys_ptrace".to_string()];
        assert!(parse_and_validate(&names).is_err());
    }

    #[test]
    fn accept_sys_admin() {
        let names = vec!["sys_admin".to_string()];
        let caps = parse_and_validate(&names).unwrap();
        assert_eq!(caps.len(), 1);
    }

    #[test]
    fn accept_allowed() {
        let names = vec!["net_bind_service".to_string(), "ipc_lock".to_string()];
        let caps = parse_and_validate(&names).unwrap();
        assert_eq!(caps.len(), 2);
    }

    #[test]
    fn reject_dac_read_search() {
        let names = vec!["dac_read_search".to_string()];
        assert!(parse_and_validate(&names).is_err());
    }

    #[test]
    fn info_for_with_cap_prefix() {
        let info = info_for("CAP_NET_ADMIN").unwrap();
        assert_eq!(info.name, "net_admin");
    }

    #[test]
    fn info_for_without_prefix() {
        let info = info_for("net_admin").unwrap();
        assert_eq!(info.name, "net_admin");
    }

    #[test]
    fn info_for_sys_admin() {
        let info = info_for("CAP_SYS_ADMIN").unwrap();
        assert_eq!(info.name, "sys_admin");
    }

    #[test]
    fn info_for_unknown_returns_none() {
        assert!(info_for("CAP_SYS_PTRACE").is_none());
    }
}
