use devenv_tui::{DataEvent, Model, OperationId};
use std::collections::HashMap;
use tokio::sync::mpsc;

#[tokio::test]
async fn test_event_channel_communication() {
    let (tx, mut rx) = mpsc::unbounded_channel();

    let event = DataEvent::RegisterOperation {
        operation_id: OperationId::new("test-op"),
        operation_name: "Test Operation".to_string(),
        parent: None,
        fields: HashMap::new(),
    };

    tx.send(event.clone()).unwrap();

    let received = rx.recv().await;
    assert!(received.is_some());
}

#[tokio::test]
async fn test_multiple_events_through_channel() {
    let (tx, mut rx) = mpsc::unbounded_channel();

    for i in 0..10 {
        let event = DataEvent::RegisterOperation {
            operation_id: OperationId::new(format!("op-{}", i)),
            operation_name: format!("Operation {}", i),
            parent: None,
            fields: HashMap::new(),
        };
        tx.send(event).unwrap();
    }

    let mut count = 0;
    while let Ok(event) = rx.try_recv() {
        count += 1;
        match event {
            DataEvent::RegisterOperation { operation_id, .. } => {
                assert!(operation_id.0.starts_with("op-"));
            }
            _ => panic!("Unexpected event type"),
        }
    }

    assert_eq!(count, 10);
}

#[tokio::test]
async fn test_event_processing_maintains_order() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut model = Model::new();

    for i in 0..5 {
        let event = DataEvent::RegisterOperation {
            operation_id: OperationId::new(format!("op-{}", i)),
            operation_name: format!("Operation {}", i),
            parent: None,
            fields: HashMap::new(),
        };
        tx.send(event).unwrap();
    }

    let mut processed = Vec::new();
    while let Ok(event) = rx.try_recv() {
        match &event {
            DataEvent::RegisterOperation { operation_id, .. } => {
                processed.push(operation_id.0.clone());
            }
            _ => {}
        }
        event.apply(&mut model);
    }

    assert_eq!(model.operations.len(), 5);

    for i in 0..5 {
        let expected = format!("op-{}", i);
        assert_eq!(processed[i], expected);
        assert!(model.operations.contains_key(&OperationId::new(&expected)));
    }
}

#[test]
fn test_operation_id_equality() {
    let id1 = OperationId::new("test");
    let id2 = OperationId::new("test");
    let id3 = OperationId::new("different");

    assert_eq!(id1, id2);
    assert_ne!(id1, id3);
}

#[test]
fn test_operation_id_display() {
    let id = OperationId::new("test-operation");
    assert_eq!(format!("{}", id), "test-operation");
}

#[test]
fn test_operation_id_hash() {
    use std::collections::HashSet;

    let mut set = HashSet::new();
    set.insert(OperationId::new("op1"));
    set.insert(OperationId::new("op2"));
    set.insert(OperationId::new("op1"));

    assert_eq!(set.len(), 2);
}
