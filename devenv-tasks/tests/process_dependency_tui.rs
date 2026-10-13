//! Feed real scheduler events into the TUI so process/task ID mismatches cannot
//! be hidden by a hand-built hierarchy fixture.
use devenv_core::VerbosityLevel;
use devenv_tasks::{Config, RunMode, TaskConfig, TaskType, Tasks};
use devenv_tui::{ActivityModel, ActivityVariant, UiState};
use std::collections::HashMap;
use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

#[tokio::test]
async fn failed_prerequisite_is_visible_under_each_process_with_stopped_filter() {
    let (mut events, handle) = devenv_activity::init();
    let _guard = handle.install();
    // Keep this as one test: the activity transport is process-global.
    for shared in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let script = temp.path().join("setup");
        std::fs::write(
            &script,
            "#!/bin/sh\necho 'specific setup failure' >&2\nexit 1\n",
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let names = if shared {
            vec!["alpha", "beta"]
        } else {
            vec!["alpha"]
        };
        let mut configs = vec![TaskConfig {
            name: "test:setup".into(),
            command: Some(script.to_string_lossy().into_owned()),
            ..Default::default()
        }];
        for name in &names {
            configs.push(TaskConfig {
                name: format!("devenv:processes:{name}"),
                r#type: TaskType::Process,
                command: Some("true".into()),
                after: vec!["test:setup".into()],
                ..Default::default()
            });
        }
        let config = Config {
            tasks: configs,
            roots: names
                .iter()
                .map(|name| format!("devenv:processes:{name}"))
                .collect(),
            run_mode: RunMode::All,
            runtime_dir: temp.path().join("runtime"),
            cache_dir: temp.path().join("cache"),
            sudo_context: None,
            env: HashMap::new(),
            bash: String::new(),
            ignore_process_deps: false,
            exit_on_idle: Some(false),
            supervisor: devenv_processes::SupervisionMode::Native,
            capability_broker: None,
        };
        let tasks = Tasks::builder(
            config,
            VerbosityLevel::Normal,
            tokio_shutdown::Shutdown::new(),
        )
        .build()
        .await
        .unwrap();
        tokio::time::timeout(Duration::from_secs(10), tasks.run(true))
            .await
            .unwrap();
        let mut model = ActivityModel::default();
        while let Ok(event) = events.try_recv() {
            model.apply_activity_event(event);
        }
        let mut ui = UiState::new();
        ui.hide_stopped_processes = true;
        let display = model.get_display_activities(&ui);
        let failures: Vec<_> = display
            .iter()
            .filter(|d| d.activity.name == "test:setup")
            .collect();
        assert_eq!(
            failures.len(),
            names.len(),
            "failed shared tasks must be visible under every process"
        );
        for failure in failures {
            assert!(
                model
                    .get_build_logs(failure.activity.id)
                    .unwrap()
                    .iter()
                    .any(|line| line.contains("specific setup failure"))
            );
        }
        for name in names {
            assert!(
                display.iter().any(|d| d.activity.name == name
                    && matches!(d.activity.variant, ActivityVariant::Process(_))),
                "process {name} and its dependency failure must remain visible"
            );
        }
        drop(tasks);
        while events.try_recv().is_ok() {}
    }
}
