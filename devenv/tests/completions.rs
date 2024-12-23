mod common;
use crate::common::*;
use tower_lsp::lsp_types::{
    CompletionContext, CompletionParams, CompletionTriggerKind, Position, TextDocumentIdentifier,
    TextDocumentPositionParams,
};
#[tokio::test]
async fn test_simple_completions() {
    let mut ctx = TestContext::new("simple");
    ctx.initialize().await;
    let test_content = r#"{ pkgs, lib, config, inputs, ... }:
        {
        languages.
        }"#;
    ctx.notify::<tower_lsp::lsp_types::notification::DidOpenTextDocument>(
        tower_lsp::lsp_types::DidOpenTextDocumentParams {
            text_document: tower_lsp::lsp_types::TextDocumentItem {
                uri: ctx.doc_uri("test.nix"),
                language_id: "nix".to_string(),
                version: 1,
                text: test_content.to_string(),
            },
        },
    )
    .await;
    // Add a small delay to ensure the document is processed
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let completion_response = ctx
        .request::<tower_lsp::lsp_types::request::Completion>(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: ctx.doc_uri("test.nix"),
                },
                position: Position {
                    line: 2,
                    character: 18,
                },
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: Some(CompletionContext {
                trigger_kind: CompletionTriggerKind::TRIGGER_CHARACTER,
                trigger_character: Some(".".to_string()),
            }),
        })
        .await;
    if let Some(tower_lsp::lsp_types::CompletionResponse::List(list)) = completion_response {
        assert!(!list.items.is_empty(), "Should have completion items");
        let item_labels: Vec<String> = list.items.into_iter().map(|item| item.label).collect();

        println!("labels are {:?}", item_labels);
        assert!(
            item_labels.contains(&"python".to_string()),
            "Should suggest python"
        );
    } else {
        panic!("Expected CompletionResponse::List");
    }
}
#[tokio::test]
async fn test_simple_nested_completions() {
    let mut ctx = TestContext::new("simple");
    ctx.initialize().await;
    let test_content = r#"{ pkgs, lib, config, inputs, ... }:
        {
        languages = {
        p
        }"#;
    ctx.notify::<tower_lsp::lsp_types::notification::DidOpenTextDocument>(
        tower_lsp::lsp_types::DidOpenTextDocumentParams {
            text_document: tower_lsp::lsp_types::TextDocumentItem {
                uri: ctx.doc_uri("test.nix"),
                language_id: "nix".to_string(),
                version: 1,
                text: test_content.to_string(),
            },
        },
    )
    .await;
    // Add a small delay to ensure the document is processed
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let completion_response = ctx
        .request::<tower_lsp::lsp_types::request::Completion>(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: ctx.doc_uri("test.nix"),
                },
                position: Position {
                    line: 3,
                    character: 8,
                },
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: Some(CompletionContext {
                trigger_kind: CompletionTriggerKind::TRIGGER_CHARACTER,
                trigger_character: Some(".".to_string()),
            }),
        })
        .await;
    if let Some(tower_lsp::lsp_types::CompletionResponse::List(list)) = completion_response {
        assert!(!list.items.is_empty(), "Should have completion items");
        let item_labels: Vec<String> = list.items.into_iter().map(|item| item.label).collect();
        // println!("Completion list is {:?}", item_labels);
        assert!(
            item_labels.contains(&"python".to_string()),
            "Should suggest python"
        );
        assert!(
            item_labels.contains(&"nodejs".to_string()),
            "Should suggest nodejs"
        );
    } else {
        panic!("Expected CompletionResponse::List");
    }
}
