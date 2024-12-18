use dashmap::DashMap;
use regex::Regex;
use serde_json::Value;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};
use tracing::{debug, info};
use tree_sitter::{Node, Parser, Point, Tree, TreeCursor};
use tree_sitter_nix::language;

#[derive(Clone, Debug)]
pub struct Backend {
    pub client: Client,
    // pub document_map: DashMap<String, String>,
    pub document_map: DashMap<String, (String, Tree)>,
    pub completion_json: Value,
}

impl Backend {
    pub fn new(client: Client, completion_json: Value) -> Self {
        let mut parser = Parser::new();
        parser
            .set_language(tree_sitter_nix::language())
            .expect("Unable to load the nix language file");
        Backend {
            client,
            document_map: DashMap::new(),
            completion_json,
        }
    }

    fn get_completion_items(&self, uri: &str, position: Position) -> Vec<CompletionItem> {
        println!("Test document uri {:?}", uri);
        let (content, tree) = self
            .document_map
            .get(uri)
            .expect("Document not found")
            .clone();
        let root_node = tree.root_node();
        let point = Point::new(position.line as usize, position.character as usize);

        let scope_path = self.get_scope(root_node, point, &content);
        let line_content = content
            .lines()
            .nth(position.line as usize)
            .unwrap_or_default();
        let line_until_cursor = &line_content[..position.character as usize];
        let dot_path = self.get_path(line_until_cursor);

        let re = Regex::new(r".*\W(.*)").unwrap();
        let current_word = re
            .captures(line_until_cursor)
            .and_then(|caps| caps.get(1))
            .map(|m| m.as_str())
            .unwrap_or("");

        let search_path = [scope_path.clone(), dot_path].concat();
        let completions = self.search_json(&search_path, current_word);

        completions
            .into_iter()
            .map(|(item, description)| {
                CompletionItem::new_simple(item, description.unwrap_or_default())
            })
            .collect()
    }

    fn parse_document(&self, content: &str) -> Tree {
        let mut parser = Parser::new();
        let nix_grammar = language();
        parser
            .set_language(nix_grammar)
            .expect("Error loading Nix grammar");
        parser
            .parse(content, None)
            .expect("Failed to parse document")
    }

    fn update_document(&self, uri: &str, content: String) {
        let tree = self.parse_document(&content);
        self.document_map.insert(uri.to_string(), (content, tree));
    }

    fn get_scope(&self, root_node: Node, cursor_position: Point, source: &str) -> Vec<String> {
        debug!("Getting scope for cursor position: {:?}", cursor_position);
        debug!("Source code is: {:?}", source);
        let mut scope = Vec::new();

        if let Some(node) = root_node.descendant_for_point_range(cursor_position, cursor_position) {
            let mut cursor = node.walk();
            self.traverse_up(&mut cursor, &mut scope, source);
        }

        scope.reverse();
        debug!("Final scope: {:?}", scope);
        scope
    }

    fn traverse_up(&self, cursor: &mut TreeCursor, scope: &mut Vec<String>, source: &str) {
        loop {
            let node = cursor.node();
            debug!(
                "Current node kind: {}, text: {:?}",
                node.kind(),
                node.utf8_text(source.as_bytes())
            );

            match node.kind() {
                "attrpath" => {
                    if let Ok(text) = node.utf8_text(source.as_bytes()) {
                        let attrs: Vec<String> = text.split('.').map(String::from).collect();
                        scope.extend(attrs);
                    }
                }
                "binding" => {
                    if let Some(attrpath) = node.child_by_field_name("attrpath") {
                        if let Ok(text) = attrpath.utf8_text(source.as_bytes()) {
                            let attrs: Vec<String> = text.split('.').map(String::from).collect();
                            scope.extend(attrs);
                        }
                    }
                }
                "attrset_expression" => {
                    // We've reached an attribute set, continue traversing up
                }
                _ => {
                    // For other node types, we don't add to the scope
                }
            }

            if !cursor.goto_parent() {
                break;
            }
        }
    }

    fn get_path(&self, line: &str) -> Vec<String> {
        let parts: Vec<&str> = line.split('.').collect();

        let path = parts[..parts.len() - 1]
            .iter()
            .map(|&s| s.trim().to_string())
            .collect();
        return path;
    }

    fn search_json(&self, path: &[String], partial_key: &str) -> Vec<(String, Option<String>)> {
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
                .iter()
                .filter(|(k, _)| k.starts_with(partial_key))
                .map(|(k, v)| {
                    let description = match v {
                        Value::Object(obj) => obj
                            .get("description")
                            .and_then(|d| d.as_str())
                            .map(String::from),
                        _ => None,
                    };
                    (k.clone(), description)
                })
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
                    trigger_characters: Some(vec![".".to_string(), "\n".to_string()]),
                    work_done_progress_options: Default::default(),
                    all_commit_characters: None,
                    ..Default::default()
                }),
                execute_command_provider: Some(ExecuteCommandOptions {
                    commands: vec![],
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

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        self.client
            .log_message(MessageType::INFO, "file opened!")
            .await;
        let uri = params.text_document.uri.to_string();
        let content = params.text_document.text;
        self.update_document(&uri, content);
        self.client
            .log_message(MessageType::INFO, "file opened!")
            .await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri.to_string();
        let content = params.content_changes[0].text.clone();
        self.update_document(&uri, content);
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
        let uri = params.text_document_position.text_document.uri.to_string();
        let position = params.text_document_position.position;

        let completion_items = self.get_completion_items(&uri, position);

        Ok(Some(CompletionResponse::List(CompletionList {
            is_incomplete: false,
            items: completion_items,
        })))
    }
}
