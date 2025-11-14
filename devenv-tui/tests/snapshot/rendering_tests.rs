use devenv_tui::{Activity, ActivityVariant, NixActivityState, TaskDisplayStatus};

use crate::test_utils::builders::ActivityBuilder;
use crate::test_utils::fixtures::{
    completed_download_activity, failed_build_activity, simple_build_activity, task_activity_running,
};

fn format_activity_summary(activity: &Activity) -> String {
    let state_str = match &activity.state {
        NixActivityState::Active => "Active".to_string(),
        NixActivityState::Completed { success, duration } => {
            if *success {
                format!("Completed ({}s)", duration.as_secs())
            } else {
                format!("Failed ({}s)", duration.as_secs())
            }
        }
    };

    let variant_str = match &activity.variant {
        ActivityVariant::Build(build) => {
            format!("Build [phase: {:?}]", build.phase)
        }
        ActivityVariant::Download(download) => {
            format!(
                "Download [{}/{}]",
                download.size_current.map(|s| s.to_string()).unwrap_or_else(|| "?".to_string()),
                download.size_total.map(|s| s.to_string()).unwrap_or_else(|| "?".to_string())
            )
        }
        ActivityVariant::Query(_) => "Query".to_string(),
        ActivityVariant::Task(task) => {
            format!("Task [{:?}]", task.status)
        }
        ActivityVariant::Evaluating => "Evaluating".to_string(),
        ActivityVariant::FetchTree => "FetchTree".to_string(),
        ActivityVariant::UserOperation => "UserOperation".to_string(),
        ActivityVariant::Unknown => "Unknown".to_string(),
    };

    format!(
        "Activity {{ id: {}, name: \"{}\", short_name: \"{}\", state: {}, variant: {} }}",
        activity.id, activity.name, activity.short_name, state_str, variant_str
    )
}

#[test]
fn test_simple_build_activity_snapshot() {
    let activity = simple_build_activity();
    let summary = format_activity_summary(&activity);

    insta::assert_snapshot!(summary, @r###"Activity { id: 1, name: "Building example-package", short_name: "example-package", state: Active, variant: Build [phase: Some("buildPhase")] }"###);
}

#[test]
fn test_completed_download_activity_snapshot() {
    let activity = completed_download_activity();
    let summary = format_activity_summary(&activity);

    insta::assert_snapshot!(summary, @"Activity { id: 2, name: \"Downloading nixpkgs\", short_name: \"nixpkgs\", state: Completed (5s), variant: Download [10485760/10485760] }");
}

#[test]
fn test_failed_build_activity_snapshot() {
    let activity = failed_build_activity();
    let summary = format_activity_summary(&activity);

    insta::assert_snapshot!(summary, @"Activity { id: 3, name: \"Building failed-package\", short_name: \"failed-package\", state: Failed (30s), variant: Build [phase: Some(\"buildPhase\")] }");
}

#[test]
fn test_task_activity_snapshot() {
    let activity = ActivityBuilder::new(100)
        .name("Running test suite")
        .short_name("test")
        .task_activity(TaskDisplayStatus::Running)
        .build();

    let summary = format_activity_summary(&activity);

    insta::assert_snapshot!(summary, @"Activity { id: 100, name: \"Running test suite\", short_name: \"test\", state: Active, variant: Task [Running] }");
}

#[test]
fn test_evaluating_activity_snapshot() {
    let activity = ActivityBuilder::new(200)
        .name("Evaluating Nix expression")
        .short_name("eval")
        .evaluating_activity()
        .build();

    let summary = format_activity_summary(&activity);

    insta::assert_snapshot!(summary, @"Activity { id: 200, name: \"Evaluating Nix expression\", short_name: \"eval\", state: Active, variant: Evaluating }");
}

#[test]
fn test_query_activity_snapshot() {
    let activity = ActivityBuilder::new(300)
        .name("Querying substituter")
        .short_name("query")
        .query_activity()
        .build();

    let summary = format_activity_summary(&activity);

    insta::assert_snapshot!(summary, @"Activity { id: 300, name: \"Querying substituter\", short_name: \"query\", state: Active, variant: Query }");
}

#[test]
fn test_build_activity_with_different_phases() {
    let phases = vec!["configurePhase", "buildPhase", "installPhase", "fixupPhase"];

    for (i, phase) in phases.iter().enumerate() {
        let activity = ActivityBuilder::new(i as u64)
            .name(format!("Building package in {}", phase))
            .build_activity_with_phase(*phase)
            .build();

        let summary = format_activity_summary(&activity);
        insta::assert_snapshot!(format!("build_phase_{}", phase), summary);
    }
}

#[test]
fn test_download_progress_stages() {
    let stages = vec![
        (Some(0), Some(1000)),
        (Some(250), Some(1000)),
        (Some(500), Some(1000)),
        (Some(750), Some(1000)),
        (Some(1000), Some(1000)),
    ];

    for (i, (current, total)) in stages.iter().enumerate() {
        let activity = ActivityBuilder::new(i as u64)
            .name("Downloading package")
            .download_activity(*current, *total)
            .build();

        let summary = format_activity_summary(&activity);
        insta::assert_snapshot!(format!("download_progress_{}", i), summary);
    }
}

#[test]
fn test_task_status_lifecycle() {
    let statuses = vec![
        TaskDisplayStatus::Pending,
        TaskDisplayStatus::Running,
        TaskDisplayStatus::Success,
        TaskDisplayStatus::Failed,
        TaskDisplayStatus::Skipped,
        TaskDisplayStatus::Cancelled,
    ];

    for (i, status) in statuses.iter().enumerate() {
        let activity = ActivityBuilder::new(i as u64)
            .name("Task lifecycle")
            .task_activity(status.clone())
            .build();

        let summary = format_activity_summary(&activity);
        insta::assert_snapshot!(format!("task_status_{:?}", status), summary);
    }
}

#[test]
fn test_activity_with_detail() {
    let activity = ActivityBuilder::new(1)
        .name("Building package")
        .detail("/nix/store/abc123-package.drv")
        .build_activity()
        .build();

    let detail_str = activity.detail.as_ref().map(|d| d.as_str()).unwrap_or("None");

    insta::assert_snapshot!(detail_str, @"/nix/store/abc123-package.drv");
}

#[test]
fn test_activity_with_progress() {
    let activity = ActivityBuilder::new(1)
        .name("Building package")
        .build_activity()
        .progress(50, 100, "items")
        .build();

    let progress_str = if let Some(ref progress) = activity.progress {
        format!(
            "Progress: {}/{} {} ({}%)",
            progress.current.unwrap_or(0),
            progress.total.unwrap_or(0),
            progress.unit.as_ref().unwrap_or(&"".to_string()),
            progress.percent.unwrap_or(0.0)
        )
    } else {
        "No progress".to_string()
    };

    insta::assert_snapshot!(progress_str, @"Progress: 50/100 items (50%)");
}

#[test]
fn test_download_activity_with_substituter() {
    let activity = ActivityBuilder::new(1)
        .name("Downloading with substituter")
        .download_activity_with_substituter(Some(500), Some(1000), "https://cache.nixos.org")
        .build();

    let summary = format_activity_summary(&activity);
    insta::assert_snapshot!(summary);
}

#[test]
fn test_query_activity_with_substituter() {
    let activity = ActivityBuilder::new(1)
        .name("Querying with substituter")
        .query_activity_with_substituter("https://cache.nixos.org")
        .build();

    let summary = format_activity_summary(&activity);
    insta::assert_snapshot!(summary);
}

#[test]
fn test_task_activity_with_duration() {
    let activity = ActivityBuilder::new(1)
        .name("Task with duration")
        .task_activity_with_duration(TaskDisplayStatus::Success, 42)
        .build();

    let summary = format_activity_summary(&activity);
    insta::assert_snapshot!(summary);
}

#[test]
fn test_fetch_tree_activity_snapshot() {
    let activity = ActivityBuilder::new(1)
        .name("Fetching tree")
        .short_name("fetch-tree")
        .fetch_tree_activity()
        .build();

    let summary = format_activity_summary(&activity);
    insta::assert_snapshot!(summary);
}

#[test]
fn test_user_operation_activity_snapshot() {
    let activity = ActivityBuilder::new(1)
        .name("User operation")
        .short_name("user-op")
        .user_operation_activity()
        .build();

    let summary = format_activity_summary(&activity);
    insta::assert_snapshot!(summary);
}

#[test]
fn test_task_activity_running_snapshot() {
    let activity = task_activity_running();
    let summary = format_activity_summary(&activity);

    insta::assert_snapshot!(summary);
}

fn format_hierarchy_prefix(depth: usize, has_spinner: bool) -> String {
    let indent = if depth > 0 {
        format!("{}└── ", "  ".repeat(depth - 1))
    } else {
        String::new()
    };

    let spinner = if has_spinner && depth == 0 { "⠋ " } else { "" };

    format!("{}{}", indent, spinner)
}

#[test]
fn test_hierarchy_prefix_depth_0() {
    let prefix = format_hierarchy_prefix(0, true);
    insta::assert_snapshot!(prefix, @"⠋ ");
}

#[test]
fn test_hierarchy_prefix_depth_1() {
    let prefix = format_hierarchy_prefix(1, false);
    insta::assert_snapshot!(prefix, @"└── ");
}

#[test]
fn test_hierarchy_prefix_depth_2() {
    let prefix = format_hierarchy_prefix(2, false);
    insta::assert_snapshot!(prefix, @"  └── ");
}

#[test]
fn test_hierarchy_prefix_depth_3() {
    let prefix = format_hierarchy_prefix(3, false);
    insta::assert_snapshot!(prefix, @"    └── ");
}
