//! Real child output, restart, and capture-teardown retention tests.
mod common;

use common::TestContext;
use devenv_processes::{LogRetentionConfig, ProcessConfig, RestartConfig, RestartPolicy};
use std::time::Duration;

async fn wait_for_output(logs: &devenv_processes::logs::ProcessLogs, marker: &str) {
    tokio::time::timeout(Duration::from_secs(15), async {
        while !logs.stdout.tail(100).contains(marker) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("timed out waiting for captured output");
}

#[tokio::test(flavor = "multi_thread")]
async fn automatic_restart_keeps_history_manual_restart_clears_it() {
    let ctx = TestContext::new();
    let counter = ctx.temp_path().join("counter");
    std::fs::write(&counter, "0").unwrap();
    let config = ProcessConfig {
        name: "rolling".into(),
        exec: format!(
            "count=$(cat '{0}'); count=$((count + 1)); echo $count > '{0}'; \
             echo generation-$count; echo error-$count >&2; \
             if [ $count = 1 ]; then exit 1; fi; exec tail -f /dev/null",
            counter.display()
        ),
        logs: LogRetentionConfig {
            max_file_size: 8,
            max_files: 4,
        },
        restart: RestartConfig {
            on: RestartPolicy::OnFailure,
            max: Some(2),
            window: None,
        },
        ..Default::default()
    };
    let runner = ctx.create_manager();
    runner.start_command(&config, None).await.unwrap();
    let logs = runner.process_logs("rolling").unwrap();
    wait_for_output(&logs, "generation-2").await;
    assert_eq!(logs.stdout.tail(10), "generation-1\ngeneration-2");
    assert_eq!(logs.stderr.tail(10), "error-1\nerror-2");

    let (_, mut live) = logs.stdout.subscribe(0);
    runner.restart("rolling").await.unwrap();
    wait_for_output(&logs, "generation-3").await;
    assert_eq!(logs.stdout.tail(10), "generation-3");
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(5), live.recv())
            .await
            .unwrap()
            .unwrap(),
        "generation-3"
    );
    runner.stop_all().await.unwrap();
    assert_eq!(logs.stdout.tail(10), "generation-3");
    for file in std::fs::read_dir(ctx.state_dir.join("logs")).unwrap() {
        assert!(file.unwrap().metadata().unwrap().len() <= 8);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn burst_output_is_bounded_and_shutdown_drains_unterminated_tail() {
    let ctx = TestContext::new();
    let runner = ctx.create_manager();
    let config = ProcessConfig {
        name: "burst".into(),
        exec: "for ((i=0; i<1000; i++)); do echo output-$i; echo error-$i >&2; done; printf final; printf last-error >&2".into(),
        logs: LogRetentionConfig { max_file_size: 32, max_files: 3 },
        restart: RestartConfig { on: RestartPolicy::Never, ..Default::default() },
        ..Default::default()
    };
    runner.start_command(&config, None).await.unwrap();
    let logs = runner.process_logs("burst").unwrap();
    wait_for_output(&logs, "final").await;
    runner.stop_all().await.unwrap();
    assert!(logs.stdout.tail(100).ends_with("output-999\nfinal"));
    assert!(logs.stderr.tail(100).ends_with("error-999\nlast-error"));
    let files: Vec<_> = std::fs::read_dir(ctx.state_dir.join("logs"))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(files.len(), 6);
    assert!(
        files
            .iter()
            .all(|file| file.metadata().unwrap().len() <= 32)
    );
}
