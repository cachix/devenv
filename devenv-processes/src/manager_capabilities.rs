//! Process-manager identity and capability negotiation.
//!
//! Current Nix modules declare capabilities for the selected manager. Older
//! module revisions do not expose that data, so the CLI carries a conservative
//! copy for managers that shipped before capability negotiation existed.

use serde::{Deserialize, Serialize};

/// Operations a configured process manager can support through devenv.
///
/// New fields must default to `false`: an older declaration must never opt in
/// to behavior it did not explicitly advertise.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ManagerCapabilities {
    /// The manager can remain running after the invoking client exits.
    pub background_start: bool,
    /// `devenv processes attach` can attach to this manager.
    pub devenv_attach: bool,
    /// `devenv processes wait` can query readiness from this manager.
    pub wait_ready: bool,
    /// Individual processes can be started, stopped, and restarted in-place.
    pub individual_control: bool,
    /// A cold manager start can launch a named subset of configured processes.
    pub subset_start: bool,
    /// The manager itself requires a terminal or pseudo-terminal.
    pub requires_tty: bool,
    /// The manager has a manager-aware graceful shutdown operation.
    pub manager_aware_stop: bool,
}

/// Where the effective capability declaration came from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CapabilitySource {
    /// Capabilities evaluated from the selected Nix process-manager module.
    Nix,
    /// Compatibility data embedded in the Rust binary for an older Nix module.
    RustFallback,
    /// An unknown manager without a capability declaration.
    ConservativeDefault,
}

/// The selected manager and the capabilities devenv may rely on.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManagerDescriptor {
    pub id: String,
    pub capabilities: ManagerCapabilities,
    pub capability_source: CapabilitySource,
}

impl ManagerDescriptor {
    /// Resolve a Nix declaration, falling back only for managers known to this
    /// binary. Unknown managers remain usable in the foreground but opt in to
    /// no optional lifecycle operations.
    pub fn resolve(id: impl Into<String>, declared: Option<ManagerCapabilities>) -> Self {
        let id = id.into();
        let (capabilities, capability_source) = match declared {
            Some(capabilities) => (capabilities, CapabilitySource::Nix),
            None => match fallback_capabilities(&id) {
                Some(capabilities) => (capabilities, CapabilitySource::RustFallback),
                None => (
                    ManagerCapabilities::default(),
                    CapabilitySource::ConservativeDefault,
                ),
            },
        };
        Self {
            id,
            capabilities,
            capability_source,
        }
    }

    pub fn is_native(&self) -> bool {
        self.id == "native"
    }
}

/// Compatibility declarations for Nix modules released before capabilities
/// were exposed. Keep this in sync with the declarations in
/// `src/modules/process-managers`.
pub fn fallback_capabilities(manager: &str) -> Option<ManagerCapabilities> {
    let capabilities = match manager {
        "native" => ManagerCapabilities {
            background_start: true,
            devenv_attach: true,
            wait_ready: true,
            individual_control: true,
            subset_start: true,
            requires_tty: false,
            manager_aware_stop: true,
        },
        "process-compose" => ManagerCapabilities {
            background_start: true,
            subset_start: true,
            ..ManagerCapabilities::default()
        },
        "honcho" => ManagerCapabilities {
            background_start: true,
            subset_start: true,
            ..ManagerCapabilities::default()
        },
        "hivemind" => ManagerCapabilities {
            background_start: true,
            ..ManagerCapabilities::default()
        },
        "overmind" => ManagerCapabilities {
            background_start: true,
            subset_start: true,
            manager_aware_stop: true,
            ..ManagerCapabilities::default()
        },
        "mprocs" => ManagerCapabilities {
            requires_tty: true,
            ..ManagerCapabilities::default()
        },
        _ => return None,
    };
    Some(capabilities)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nix_declarations_override_embedded_fallbacks() {
        let declared = ManagerCapabilities {
            background_start: false,
            wait_ready: true,
            ..ManagerCapabilities::default()
        };
        let descriptor = ManagerDescriptor::resolve("process-compose", Some(declared));
        assert_eq!(descriptor.capabilities, declared);
        assert_eq!(descriptor.capability_source, CapabilitySource::Nix);
    }

    #[test]
    fn known_managers_have_backwards_compatible_fallbacks() {
        for manager in [
            "native",
            "process-compose",
            "overmind",
            "honcho",
            "hivemind",
            "mprocs",
        ] {
            let descriptor = ManagerDescriptor::resolve(manager, None);
            assert_eq!(
                descriptor.capability_source,
                CapabilitySource::RustFallback,
                "{manager}"
            );
        }
        assert!(fallback_capabilities("honcho").unwrap().background_start);
        assert!(fallback_capabilities("hivemind").unwrap().background_start);
        assert!(!fallback_capabilities("mprocs").unwrap().background_start);
        assert!(fallback_capabilities("mprocs").unwrap().requires_tty);
    }

    #[test]
    fn unknown_managers_get_no_implicit_capabilities() {
        let descriptor = ManagerDescriptor::resolve("custom", None);
        assert_eq!(
            descriptor.capability_source,
            CapabilitySource::ConservativeDefault
        );
        assert_eq!(descriptor.capabilities, ManagerCapabilities::default());
    }

    #[test]
    fn missing_and_future_fields_are_compatible() {
        let old: ManagerCapabilities =
            serde_json::from_str(r#"{"background_start":true}"#).expect("old declaration parses");
        assert!(old.background_start);
        assert!(!old.wait_ready);

        let future: ManagerCapabilities =
            serde_json::from_str(r#"{"background_start":true,"future_capability":true}"#)
                .expect("future declaration parses");
        assert!(future.background_start);
    }
}
