use devenv_processes::manager_capabilities::{ManagerCapabilities, fallback_capabilities};

#[test]
fn compatibility_capabilities_match_known_manager_contracts() {
    let cases = [
        (
            "native",
            ManagerCapabilities {
                background_start: true,
                devenv_attach: true,
                wait_ready: true,
                individual_control: true,
                subset_start: true,
                requires_tty: false,
                manager_aware_stop: true,
            },
        ),
        (
            "process-compose",
            ManagerCapabilities {
                background_start: true,
                subset_start: true,
                ..ManagerCapabilities::default()
            },
        ),
        (
            "overmind",
            ManagerCapabilities {
                background_start: true,
                subset_start: true,
                manager_aware_stop: true,
                ..ManagerCapabilities::default()
            },
        ),
        (
            "honcho",
            ManagerCapabilities {
                background_start: true,
                subset_start: true,
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
        (
            "mprocs",
            ManagerCapabilities {
                requires_tty: true,
                ..ManagerCapabilities::default()
            },
        ),
    ];

    for (manager, expected) in cases {
        assert_eq!(fallback_capabilities(manager), Some(expected), "{manager}");
    }
}
