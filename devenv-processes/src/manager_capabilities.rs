//! Process-manager identity and capability negotiation.
//!
//! Current Nix modules declare capabilities for the selected manager. Older
//! module revisions do not expose that data, so the CLI carries a conservative
//! copy for managers that shipped before capability negotiation existed.

use serde::{Deserialize, Serialize};

/// A user-visible operation exposed by devenv's manager integration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ManagerOperation {
    BackgroundStart,
    DevenvAttach,
    WaitReady,
    IndividualControl,
    ColdStartSubset,
}

impl ManagerOperation {
    pub const fn description(self) -> &'static str {
        match self {
            Self::BackgroundStart => "background start",
            Self::DevenvAttach => "devenv attach",
            Self::WaitReady => "readiness waiting",
            Self::IndividualControl => "individual process control",
            Self::ColdStartSubset => "starting a process subset",
        }
    }
}

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
    pub cold_start_subset: bool,
}

impl ManagerCapabilities {
    pub const fn supports(self, operation: ManagerOperation) -> bool {
        match operation {
            ManagerOperation::BackgroundStart => self.background_start,
            ManagerOperation::DevenvAttach => self.devenv_attach,
            ManagerOperation::WaitReady => self.wait_ready,
            ManagerOperation::IndividualControl => self.individual_control,
            ManagerOperation::ColdStartSubset => self.cold_start_subset,
        }
    }
}

/// Terminal contract for launching the manager adapter.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ManagerTerminal {
    #[default]
    None,
    Controlling,
}

/// Mechanism used before final process-scope cleanup.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ManagerStopMethod {
    NativeApi,
    Command,
    #[default]
    ProcessScope,
}

/// Client protocol used for attach, readiness, and individual control.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ManagerClient {
    #[default]
    None,
    NativeApi,
}

/// Runtime adapter settings, separate from user-visible capabilities.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ManagerAdapter {
    pub terminal: ManagerTerminal,
    pub stop: ManagerStopMethod,
    pub client: ManagerClient,
}

/// Where the effective capability declaration came from.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DeclarationSource {
    /// Capabilities evaluated from the selected Nix process-manager module.
    Nix,
    /// Compatibility data embedded in the Rust binary for an older Nix module.
    RustFallback,
    /// An unknown manager without a capability declaration.
    #[default]
    ConservativeDefault,
}

/// The selected manager and the capabilities devenv may rely on.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManagerDescriptor {
    pub id: String,
    pub capabilities: ManagerCapabilities,
    pub adapter: ManagerAdapter,
    pub capabilities_source: DeclarationSource,
    pub adapter_source: DeclarationSource,
}

impl ManagerDescriptor {
    /// Resolve a Nix declaration, falling back only for managers known to this
    /// binary. Unknown managers remain usable in the foreground but opt in to
    /// no optional lifecycle operations.
    pub fn resolve(
        id: impl Into<String>,
        declared: Option<ManagerCapabilities>,
        declared_adapter: Option<ManagerAdapter>,
    ) -> Self {
        let id = id.into();
        let (capabilities, capabilities_source) = match declared {
            Some(capabilities) => (capabilities, DeclarationSource::Nix),
            None => match fallback_capabilities(&id) {
                Some(capabilities) => (capabilities, DeclarationSource::RustFallback),
                None => (
                    ManagerCapabilities::default(),
                    DeclarationSource::ConservativeDefault,
                ),
            },
        };
        let (adapter, adapter_source) = match declared_adapter {
            Some(adapter) => (adapter, DeclarationSource::Nix),
            None => match fallback_adapter(&id) {
                Some(adapter) => (adapter, DeclarationSource::RustFallback),
                None => (
                    ManagerAdapter::default(),
                    DeclarationSource::ConservativeDefault,
                ),
            },
        };
        Self {
            id,
            capabilities,
            adapter,
            capabilities_source,
            adapter_source,
        }
    }

    pub fn require(&self, operation: ManagerOperation) -> miette::Result<()> {
        if self.capabilities.supports(operation) {
            Ok(())
        } else {
            miette::bail!(
                "process manager '{}' does not support {}",
                self.id,
                operation.description()
            )
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
            cold_start_subset: true,
        },
        "process-compose" => ManagerCapabilities {
            background_start: true,
            cold_start_subset: true,
            ..ManagerCapabilities::default()
        },
        "honcho" => ManagerCapabilities {
            background_start: true,
            cold_start_subset: true,
            ..ManagerCapabilities::default()
        },
        "hivemind" => ManagerCapabilities {
            background_start: true,
            ..ManagerCapabilities::default()
        },
        "overmind" => ManagerCapabilities {
            background_start: true,
            cold_start_subset: true,
            ..ManagerCapabilities::default()
        },
        "mprocs" => ManagerCapabilities::default(),
        _ => return None,
    };
    Some(capabilities)
}

/// Compatibility adapter settings for Nix modules predating declarations.
pub fn fallback_adapter(manager: &str) -> Option<ManagerAdapter> {
    let adapter = match manager {
        "native" => ManagerAdapter {
            terminal: ManagerTerminal::None,
            stop: ManagerStopMethod::NativeApi,
            client: ManagerClient::NativeApi,
        },
        "overmind" => ManagerAdapter {
            terminal: ManagerTerminal::None,
            stop: ManagerStopMethod::Command,
            client: ManagerClient::None,
        },
        "mprocs" => ManagerAdapter {
            terminal: ManagerTerminal::Controlling,
            stop: ManagerStopMethod::ProcessScope,
            client: ManagerClient::None,
        },
        "process-compose" | "honcho" | "hivemind" => ManagerAdapter::default(),
        _ => return None,
    };
    Some(adapter)
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
        let adapter = ManagerAdapter {
            terminal: ManagerTerminal::Controlling,
            stop: ManagerStopMethod::Command,
            client: ManagerClient::None,
        };
        let descriptor =
            ManagerDescriptor::resolve("process-compose", Some(declared), Some(adapter));
        assert_eq!(descriptor.capabilities, declared);
        assert_eq!(descriptor.adapter, adapter);
        assert_eq!(descriptor.capabilities_source, DeclarationSource::Nix);
        assert_eq!(descriptor.adapter_source, DeclarationSource::Nix);
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
            let descriptor = ManagerDescriptor::resolve(manager, None, None);
            assert_eq!(
                descriptor.capabilities_source,
                DeclarationSource::RustFallback,
                "{manager}"
            );
            assert_eq!(
                descriptor.adapter_source,
                DeclarationSource::RustFallback,
                "{manager}"
            );
        }
        assert!(fallback_capabilities("honcho").unwrap().background_start);
        assert!(fallback_capabilities("hivemind").unwrap().background_start);
        assert!(!fallback_capabilities("mprocs").unwrap().background_start);
        assert_eq!(
            fallback_adapter("mprocs").unwrap().terminal,
            ManagerTerminal::Controlling
        );
    }

    #[test]
    fn unknown_managers_get_no_implicit_capabilities() {
        let descriptor = ManagerDescriptor::resolve("custom", None, None);
        assert_eq!(
            descriptor.capabilities_source,
            DeclarationSource::ConservativeDefault
        );
        assert_eq!(descriptor.capabilities, ManagerCapabilities::default());
        assert_eq!(descriptor.adapter, ManagerAdapter::default());
        assert_eq!(
            descriptor.adapter_source,
            DeclarationSource::ConservativeDefault
        );
    }

    #[test]
    fn capability_and_adapter_fallbacks_are_independent() {
        let capabilities = ManagerCapabilities {
            wait_ready: true,
            ..ManagerCapabilities::default()
        };
        let declared_capabilities =
            ManagerDescriptor::resolve("overmind", Some(capabilities), None);
        assert_eq!(
            declared_capabilities.capabilities_source,
            DeclarationSource::Nix
        );
        assert_eq!(
            declared_capabilities.adapter_source,
            DeclarationSource::RustFallback
        );
        assert_eq!(declared_capabilities.capabilities, capabilities);
        assert_eq!(
            declared_capabilities.adapter.stop,
            ManagerStopMethod::Command
        );

        let adapter = ManagerAdapter {
            terminal: ManagerTerminal::Controlling,
            stop: ManagerStopMethod::ProcessScope,
            client: ManagerClient::None,
        };
        let declared_adapter = ManagerDescriptor::resolve("native", None, Some(adapter));
        assert_eq!(
            declared_adapter.capabilities_source,
            DeclarationSource::RustFallback
        );
        assert_eq!(declared_adapter.adapter_source, DeclarationSource::Nix);
        assert_eq!(declared_adapter.adapter, adapter);
        assert!(declared_adapter.capabilities.background_start);
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

    #[test]
    fn each_operation_maps_to_exactly_one_capability() {
        const OPERATIONS: [ManagerOperation; 5] = [
            ManagerOperation::BackgroundStart,
            ManagerOperation::DevenvAttach,
            ManagerOperation::WaitReady,
            ManagerOperation::IndividualControl,
            ManagerOperation::ColdStartSubset,
        ];
        let cases = [
            (
                ManagerOperation::BackgroundStart,
                ManagerCapabilities {
                    background_start: true,
                    ..ManagerCapabilities::default()
                },
            ),
            (
                ManagerOperation::DevenvAttach,
                ManagerCapabilities {
                    devenv_attach: true,
                    ..ManagerCapabilities::default()
                },
            ),
            (
                ManagerOperation::WaitReady,
                ManagerCapabilities {
                    wait_ready: true,
                    ..ManagerCapabilities::default()
                },
            ),
            (
                ManagerOperation::IndividualControl,
                ManagerCapabilities {
                    individual_control: true,
                    ..ManagerCapabilities::default()
                },
            ),
            (
                ManagerOperation::ColdStartSubset,
                ManagerCapabilities {
                    cold_start_subset: true,
                    ..ManagerCapabilities::default()
                },
            ),
        ];

        for (expected, capabilities) in cases {
            let supported = OPERATIONS
                .into_iter()
                .filter(|operation| capabilities.supports(*operation))
                .collect::<Vec<_>>();
            assert_eq!(supported, vec![expected], "{expected:?}");
        }
    }

    #[test]
    fn descriptor_errors_name_the_rejected_operation() {
        let descriptor = ManagerDescriptor::resolve(
            "test",
            Some(ManagerCapabilities::default()),
            Some(ManagerAdapter::default()),
        );

        assert_eq!(
            descriptor
                .require(ManagerOperation::WaitReady)
                .expect_err("wait is unsupported")
                .to_string(),
            "process manager 'test' does not support readiness waiting"
        );
    }
}
