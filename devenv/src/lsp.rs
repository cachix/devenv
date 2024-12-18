use dashmap::DashMap;
use regex::Regex;
use serde_json::Value;
use std::ops::Deref;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};
use tracing::{debug, info};
#[derive(Debug)]
pub struct Backend {
    pub client: Client,
    // document store in memory
    pub document_map: DashMap<String, String>,
    pub completion_json: Value,
}
impl Backend {
    fn parse_line(&self, line: &str) -> (Vec<String>, String) {
        let parts: Vec<&str> = line.split('.').collect();
        let partial_key = parts.last().unwrap_or(&"").to_string();
        let path = parts[..parts.len() - 1]
            .iter()
            .map(|&s| s.to_string())
            .collect();
        (path, partial_key)
    }
    fn search_json(&self, path: &[String], partial_key: &str) -> Vec<String> {
        let mut current = &self.completion_json;
        for key in path {
            if let Some(value) = current.get(key) {
                current = value;
            } else {
                return Vec::new();
            }
        }
        match current {
            Value::Object(map) => map
                .keys()
                .filter(|k| k.starts_with(partial_key))
                .cloned()
                .collect(),
            _ => Vec::new(),
        }
    }
}
#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            server_info: None,
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                completion_provider: Some(CompletionOptions {
                    resolve_provider: Some(false),
                    trigger_characters: Some(vec![".".to_string()]),
                    work_done_progress_options: Default::default(),
                    all_commit_characters: None,
                    ..Default::default()
                }),
                execute_command_provider: Some(ExecuteCommandOptions {
                    commands: vec!["dummy.do_something".to_string()],
                    work_done_progress_options: Default::default(),
                }),
                workspace: Some(WorkspaceServerCapabilities {
                    workspace_folders: Some(WorkspaceFoldersServerCapabilities {
                        supported: Some(true),
                        change_notifications: Some(OneOf::Left(true)),
                    }),
                    file_operations: None,
                }),
                ..ServerCapabilities::default()
            },
            ..Default::default()
        })
    }
    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "devenv lsp is now initialized!")
            .await;
    }
    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }
    async fn did_change_workspace_folders(&self, _: DidChangeWorkspaceFoldersParams) {
        self.client
            .log_message(MessageType::INFO, "workspace folders changed!")
            .await;
    }
    async fn did_change_configuration(&self, _: DidChangeConfigurationParams) {
        self.client
            .log_message(MessageType::INFO, "configuration changed!")
            .await;
    }
    async fn did_change_watched_files(&self, _: DidChangeWatchedFilesParams) {
        self.client
            .log_message(MessageType::INFO, "watched files have changed!")
            .await;
    }
    async fn execute_command(&self, _: ExecuteCommandParams) -> Result<Option<Value>> {
        self.client
            .log_message(MessageType::INFO, "command executed!")
            .await;
        match self.client.apply_edit(WorkspaceEdit::default()).await {
            Ok(res) if res.applied => self.client.log_message(MessageType::INFO, "applied").await,
            Ok(_) => self.client.log_message(MessageType::INFO, "rejected").await,
            Err(err) => self.client.log_message(MessageType::ERROR, err).await,
        }
        Ok(None)
    }
    async fn did_open(&self, _: DidOpenTextDocumentParams) {
        self.client
            .log_message(MessageType::INFO, "file opened!")
            .await;
        info!("textDocument/DidOpen");
    }
    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        // info!("textDocument/DidChange, params: {:?}", params);
        self.document_map.insert(
            params.text_document.uri.to_string(),
            params.content_changes[0].text.clone(),
        );
        self.client
            .log_message(MessageType::INFO, "file changed!")
            .await;
    }
    async fn did_save(&self, _: DidSaveTextDocumentParams) {
        info!("textDocument/DidSave");
        self.client
            .log_message(MessageType::INFO, "file saved!")
            .await;
    }
    async fn did_close(&self, _: DidCloseTextDocumentParams) {
        info!("textDocument/DidClose");
        self.client
            .log_message(MessageType::INFO, "file closed!")
            .await;
    }
    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        info!("textDocument/Completion");
        let uri = params.text_document_position.text_document.uri;
        let file_content = match self.document_map.get(uri.as_str()) {
            Some(content) => {
                debug!("Text document content via DashMap: {:?}", content.deref());
                content.clone()
            }
            None => {
                info!("No content found for the given URI");
                String::new()
            }
        };
        let position = params.text_document_position.position;
        let line = position.line as usize;
        let character = position.character as usize;
        let line_content = file_content.lines().nth(line).unwrap_or_default();
        let line_until_cursor = &line_content[..character];
        // handling regex for getting the current word
        let re = Regex::new(r".*\W(.*)").unwrap(); // Define the regex pattern
        let current_word = re
            .captures(line_until_cursor)
            .and_then(|caps| caps.get(1))
            .map(|m| m.as_str())
            .unwrap_or("");
        debug!("Current line content {:?}", line_content);
        debug!("Line until cursor: {:?}", line_until_cursor);
        debug!("Current word {:?}", current_word);
        // Parse the line to get the current path and partial key
        let (path, partial_key) = self.parse_line(current_word);
        // Search for completions in the JSON
        let completions = self.search_json(&path, &partial_key);
        info!("Probable completion items {:?}", completions);
        // covert completions to CompletionItems format
        let completion_items: Vec<_> = completions
            .iter()
            .map(|item| CompletionItem::new_simple(item.to_string(), "".to_string()))
            .collect();
        Ok(Some(CompletionResponse::List(CompletionList {
            is_incomplete: false,
            items: completion_items,
        })))
    }
}
