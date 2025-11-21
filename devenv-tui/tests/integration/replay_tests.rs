use devenv_tui::Model;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;

#[derive(Debug, Deserialize, Clone)]
struct RawTraceEvent {
    #[serde(default)]
    _target: String,
    #[serde(default)]
    fields: HashMap<String, serde_json::Value>,
}

fn extract_string_map(json_map: &HashMap<String, serde_json::Value>) -> HashMap<String, String> {
    json_map
        .iter()
        .filter_map(|(k, v)| {
            v.as_str()
                .map(|s| (k.clone(), s.to_string()))
                .or_else(|| v.as_u64().map(|n| (k.clone(), n.to_string())))
                .or_else(|| v.as_i64().map(|n| (k.clone(), n.to_string())))
                .or_else(|| v.as_bool().map(|b| (k.clone(), b.to_string())))
        })
        .collect()
}

fn load_trace_file(filename: &str) -> Result<Vec<HashMap<String, String>>, std::io::Error> {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("tests/integration/scenarios");
    path.push(filename);

    let file = File::open(&path)?;
    let reader = BufReader::new(file);

    let mut events = Vec::new();

    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }

        if let Ok(raw_event) = serde_json::from_str::<RawTraceEvent>(&line) {
            let fields = extract_string_map(&raw_event.fields);
            events.push(fields);
        }
    }

    Ok(events)
}

#[test]
fn test_load_simple_build_trace() {
    let events = load_trace_file("simple_build.jsonl").expect("Failed to load trace file");

    assert!(!events.is_empty(), "Trace file should contain events");
    assert!(
        events.len() >= 5,
        "Simple build should have at least 5 events"
    );

    let has_build_phase = events.iter().any(|e| {
        e.get("phase").is_some()
            && (e.get("phase") == Some(&"buildPhase".to_string())
                || e.get("phase") == Some(&"configurePhase".to_string()))
    });
    assert!(has_build_phase, "Should have build phase events");

    let has_progress = events.iter().any(|e| {
        e.get("activity_id").is_some() && e.get("done").is_some() && e.get("expected").is_some()
    });
    assert!(has_progress, "Should have progress events");
}

#[test]
fn test_load_parallel_downloads_trace() {
    let events = load_trace_file("parallel_downloads.jsonl").expect("Failed to load trace file");

    assert!(!events.is_empty(), "Trace file should contain events");

    let download_events: Vec<_> = events
        .iter()
        .filter(|e| e.get("bytes_downloaded").is_some())
        .collect();

    assert!(
        download_events.len() >= 3,
        "Should have multiple download progress events"
    );

    let has_multiple_activities = events
        .iter()
        .filter_map(|e| e.get("activity_id"))
        .collect::<std::collections::HashSet<_>>()
        .len()
        > 1;

    assert!(
        has_multiple_activities,
        "Should have multiple concurrent activities"
    );
}

#[test]
fn test_load_failed_build_trace() {
    let events = load_trace_file("failed_build.jsonl").expect("Failed to load trace file");

    assert!(!events.is_empty(), "Trace file should contain events");

    let error_events: Vec<_> = events
        .iter()
        .filter(|e| {
            e.get("line")
                .map(|line| line.contains("error") || line.contains("failed"))
                .unwrap_or(false)
        })
        .collect();

    assert!(
        !error_events.is_empty(),
        "Failed build should have error events"
    );
}

#[test]
fn test_load_evaluation_progress_trace() {
    let events = load_trace_file("evaluation_progress.jsonl").expect("Failed to load trace file");

    assert!(!events.is_empty(), "Trace file should contain events");

    let eval_events: Vec<_> = events
        .iter()
        .filter(|e| e.get("total_files_evaluated").is_some())
        .collect();

    assert!(
        !eval_events.is_empty(),
        "Should have evaluation progress events"
    );

    let has_file_list = events.iter().any(|e| e.get("files").is_some());
    assert!(has_file_list, "Should have file list in events");
}

#[test]
fn test_trace_events_have_timestamps() {
    let events = load_trace_file("simple_build.jsonl").expect("Failed to load trace file");

    let has_message = events.iter().any(|e| e.get("message").is_some());
    assert!(has_message, "Events should have messages");
}

#[test]
fn test_trace_events_have_activity_ids() {
    let events = load_trace_file("simple_build.jsonl").expect("Failed to load trace file");

    let activity_events: Vec<_> = events
        .iter()
        .filter(|e| e.get("activity_id").is_some())
        .collect();

    assert!(
        !activity_events.is_empty(),
        "Should have events with activity_id"
    );

    for event in activity_events {
        let activity_id = event.get("activity_id").unwrap();
        assert!(
            activity_id.parse::<u64>().is_ok(),
            "activity_id should be a valid u64"
        );
    }
}

#[test]
fn test_parse_nix_progress_from_trace() {
    let events = load_trace_file("simple_build.jsonl").expect("Failed to load trace file");

    let progress_events: Vec<_> = events
        .iter()
        .filter(|e| {
            e.get("activity_id").is_some() && e.get("done").is_some() && e.get("expected").is_some()
        })
        .collect();

    assert!(!progress_events.is_empty());

    for event in progress_events {
        let done = event.get("done").unwrap().parse::<u64>();
        let expected = event.get("expected").unwrap().parse::<u64>();

        assert!(done.is_ok(), "done should be a valid u64");
        assert!(expected.is_ok(), "expected should be a valid u64");

        if let (Ok(done), Ok(expected)) = (done, expected) {
            assert!(done <= expected, "done should be <= expected");
        }
    }
}

#[test]
fn test_parse_build_phase_from_trace() {
    let events = load_trace_file("simple_build.jsonl").expect("Failed to load trace file");

    let phase_events: Vec<_> = events.iter().filter(|e| e.get("phase").is_some()).collect();

    assert!(!phase_events.is_empty());

    let known_phases = vec!["configurePhase", "buildPhase", "installPhase", "fixupPhase"];

    for event in phase_events {
        let phase = event.get("phase").unwrap();
        assert!(
            known_phases.contains(&phase.as_str()),
            "Phase '{}' should be a known build phase",
            phase
        );
    }
}

#[test]
fn test_parse_download_progress_from_trace() {
    let events = load_trace_file("parallel_downloads.jsonl").expect("Failed to load trace file");

    let download_events: Vec<_> = events
        .iter()
        .filter(|e| e.get("bytes_downloaded").is_some())
        .collect();

    assert!(!download_events.is_empty());

    for event in download_events {
        let bytes_downloaded = event.get("bytes_downloaded").unwrap().parse::<u64>();
        assert!(
            bytes_downloaded.is_ok(),
            "bytes_downloaded should be a valid u64"
        );

        if let Some(total_bytes_str) = event.get("total_bytes") {
            let total_bytes = total_bytes_str.parse::<u64>();
            assert!(total_bytes.is_ok(), "total_bytes should be a valid u64");

            if let (Ok(downloaded), Ok(total)) = (bytes_downloaded, total_bytes) {
                assert!(
                    downloaded <= total,
                    "bytes_downloaded should be <= total_bytes"
                );
            }
        }
    }
}

#[test]
fn test_trace_files_exist() {
    let files = vec![
        "simple_build.jsonl",
        "parallel_downloads.jsonl",
        "failed_build.jsonl",
        "evaluation_progress.jsonl",
    ];

    for file in files {
        let result = load_trace_file(file);
        assert!(
            result.is_ok(),
            "Trace file '{}' should exist and be readable",
            file
        );
    }
}

#[test]
fn test_all_trace_files_are_valid_json() {
    let files = vec![
        "simple_build.jsonl",
        "parallel_downloads.jsonl",
        "failed_build.jsonl",
        "evaluation_progress.jsonl",
    ];

    for file in files {
        let events = load_trace_file(file).expect(&format!("Failed to load {}", file));
        assert!(
            !events.is_empty(),
            "Trace file '{}' should contain at least one event",
            file
        );
    }
}

#[test]
fn test_model_integration_with_simple_trace() {
    let model = Model::new();

    assert_eq!(model.root_activities.len(), 0);
    assert_eq!(model.activities.len(), 0);
}
