//! Integration tests for foreground idle behavior.

mod common;

use common::*;
use devenv_processes::{
    OnIdle, ProcessConfig, ProcessPhase, RestartConfig, RestartPolicy, WatchConfig,
};
use std::time::Duration;
use tokio::time::timeout;

const TEST_TIMEOUT: Duration = Duration::from_secs(10);

#[tokio::test(flavor = "multi_thread")]
async fn exit_on_idle_returns_after_a_watched_native_oneshot_exits() {
    timeout(TEST_TIMEOUT, async {
        let ctx = TestContext::new();
        let watched_file = ctx.temp_path().join("input.txt");
        tokio::fs::write(&watched_file, "initial").await.unwrap();

        let config = ProcessConfig {
            name: "watched-oneshot".to_string(),
            exec: "exit 0".to_string(),
            restart: RestartConfig {
                on: RestartPolicy::Never,
                ..Default::default()
            },
            watch: WatchConfig {
                paths: vec![watched_file],
                ..Default::default()
            },
            ..Default::default()
        };

        let manager = ctx.create_manager();
        manager.start_command(&config, None).await.unwrap();
        manager
            .run_foreground(
                tokio_util::sync::CancellationToken::new(),
                None,
                OnIdle::Exit,
            )
            .await
            .unwrap();

        assert_eq!(
            manager.get_phase("watched-oneshot").await,
            Some(ProcessPhase::Exited)
        );
        manager.stop_all().await.unwrap();
    })
    .await
    .expect("OnIdle::Exit should not wait for a parked file watcher");
}
