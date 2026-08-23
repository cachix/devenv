use devenv_processes::manager_capabilities::{
    ManagerAdapter, ManagerCapabilities, ManagerClient, ManagerStopMethod, ManagerTerminal,
    fallback_adapter, fallback_capabilities,
};

#[test]
fn compatibility_contracts_match_known_managers() {
    let cases = [
        (
            "native",
            ManagerCapabilities {
                background_start: true,
                devenv_attach: true,
                wait_ready: true,
                individual_control: true,
                cold_start_subset: true,
            },
        ),
        (
            "process-compose",
            ManagerCapabilities {
                background_start: true,
                cold_start_subset: true,
                ..ManagerCapabilities::default()
            },
        ),
        (
            "overmind",
            ManagerCapabilities {
                background_start: true,
                cold_start_subset: true,
                ..ManagerCapabilities::default()
            },
        ),
        (
            "honcho",
            ManagerCapabilities {
                background_start: true,
                cold_start_subset: true,
                ..ManagerCapabilities::default()
            },
        ),
        (
            "hivemind",
            ManagerCapabilities {
                background_start: true,
                ..ManagerCapabilities::default()
            },
        ),
        ("mprocs", ManagerCapabilities::default()),
    ];

    for (manager, expected) in cases {
        assert_eq!(fallback_capabilities(manager), Some(expected), "{manager}");
    }

    let adapter_cases = [
        (
            "native",
            ManagerAdapter {
                terminal: ManagerTerminal::None,
                stop: ManagerStopMethod::NativeApi,
                client: ManagerClient::NativeApi,
            },
        ),
        ("process-compose", ManagerAdapter::default()),
        (
            "overmind",
            ManagerAdapter {
                terminal: ManagerTerminal::None,
                stop: ManagerStopMethod::Command,
                client: ManagerClient::None,
            },
        ),
        ("honcho", ManagerAdapter::default()),
        ("hivemind", ManagerAdapter::default()),
        (
            "mprocs",
            ManagerAdapter {
                terminal: ManagerTerminal::Controlling,
                stop: ManagerStopMethod::ProcessScope,
                client: ManagerClient::None,
            },
        ),
    ];

    for (manager, expected) in adapter_cases {
        assert_eq!(fallback_adapter(manager), Some(expected), "{manager}");
    }
}
