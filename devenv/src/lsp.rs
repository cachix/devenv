use super::{cli, cnix, config, lsp, tasks, utils};
use lsp_textdocument::FullTextDocument;
use regex::Regex;
use serde_json::Value;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};
use tracing::{debug, info};
use tracing_subscriber::field::debug;
use tree_sitter::{Node, Parser, Point, Query, QueryCursor, Tree, TreeCursor};

pub struct Backend {
    client: Client,
    curr_doc: std::sync::Arc<tokio::sync::Mutex<Option<FullTextDocument>>>,
    tree: std::sync::Arc<tokio::sync::Mutex<Option<Tree>>>,
    completion_json: Value,
    parser: std::sync::Arc<tokio::sync::Mutex<Parser>>,
}

impl Backend {
    pub fn new(client: Client, completion_json: Value) -> Self {
        let mut parser = Parser::new();
        parser
            .set_language(tree_sitter_nix::language())
            .expect("Unable to load the nix language file");
        Backend {
            client,
            curr_doc: std::sync::Arc::new(tokio::sync::Mutex::new(None)),
            tree: std::sync::Arc::new(tokio::sync::Mutex::new(None)),
            completion_json,
            parser: std::sync::Arc::new(tokio::sync::Mutex::new(parser)),
        }
    }

    fn text_doc_change_to_tree_sitter_edit(
        change: &TextDocumentContentChangeEvent,
        doc: &FullTextDocument,
    ) -> std::result::Result<tree_sitter::InputEdit, &'static str> {
        let range = change.range.as_ref().ok_or("Invalid edit range")?;
        let start = range.start;
        let end = range.end;

        let start_byte = doc.offset_at(start) as usize;
        let old_end_byte = doc.offset_at(end) as usize;
        let new_end_byte = start_byte + change.text.len();

        let new_end_pos = doc.position_at(new_end_byte as u32);

        Ok(tree_sitter::InputEdit {
            start_byte,
            old_end_byte,
            new_end_byte,
            start_position: Point {
                row: start.line as usize,
                column: start.character as usize,
            },
            old_end_position: Point {
                row: end.line as usize,
                column: end.character as usize,
            },
            new_end_position: Point {
                row: new_end_pos.line as usize,
                column: new_end_pos.character as usize,
            },
        })
    }

    async fn get_completion_list(&self, uri: &str, position: Position) -> Vec<CompletionItem> {
        let curr_doc = self.curr_doc.lock().await;
        let tree = self.tree.lock().await;

        debug!("Current document: {:?}", curr_doc);

        let doc = match &*curr_doc {
            Some(doc) => doc,
            None => return vec![],
        };
        let tree = match &*tree {
            Some(tree) => tree,
            None => return vec![],
        };

        debug!("Current tree: {:?}", tree);

        let content = doc.get_content(None);
        let root_node = tree.root_node();
        let point = Point::new(position.line as usize, position.character as usize);

        let scope_path = self.get_scope(root_node, point, &content);
        debug!("Scope path: {:?}", scope_path);
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
        debug!("Current word: {:?}", current_word);
        debug!("Search path: {:?}", search_path);
        let completions = self.search_json(&search_path, current_word);

        completions
            .into_iter()
            .map(|(item, description)| {
                CompletionItem::new_simple(item, description.unwrap_or_default())
            })
            .collect()
    }

    fn get_scope(&self, root_node: Node, cursor_position: Point, source: &str) -> Vec<String> {
        debug!("Getting scope for cursor position: {:?}", cursor_position);
        debug!("Source code is: {:?}", source);
        // self.print_ast(node, source, depth);
        let mut scope = Vec::new();

        if let Some(node) = root_node.descendant_for_point_range(cursor_position, cursor_position) {
            debug!("Node kind of current cursor position: {}", node.kind());
            debug!("Parent: {}", node.parent().map(|n| n.kind()).unwrap_or(""));
            let mut current_node = node;
            while let Some(sibling) = current_node.prev_named_sibling() {
                if sibling.kind() == "formals" {
                    break;
                }
                scope.push(
                    sibling
                        .utf8_text(source.as_bytes())
                        .unwrap_or_default()
                        .to_string(),
                );
                debug!(
                    "Previous named sibling: kind {}, value {:?}",
                    sibling.kind(),
                    sibling.utf8_text(source.as_bytes())
                );
                current_node = sibling;
            }

            debug!("Prev named siblings: {:?}", scope);
        }
        debug!("Final scope: {:?}", scope);
        scope.reverse();
        scope
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
            server_info: Some(ServerInfo {
                name: String::from("devenv-lsp"),
                version: Some(String::from(env!("CARGO_PKG_VERSION"))),
            }),
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::INCREMENTAL,
                )),
                completion_provider: Some(CompletionOptions {
                    resolve_provider: Some(false),
                    trigger_characters: Some(vec![".".to_string()]),
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
        let mut curr_doc = self.curr_doc.lock().await;
        let mut tree = self.tree.lock().await;
        let mut parser = self.parser.lock().await;

        *curr_doc = Some(lsp_textdocument::FullTextDocument::new(
            params.text_document.language_id.clone(),
            params.text_document.version,
            params.text_document.text.clone(),
        ));
        *tree = parser.parse(params.text_document.text, None);

        self.client
            .log_message(MessageType::INFO, "file opened!")
            .await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let mut curr_doc = self.curr_doc.lock().await;
        let mut tree = self.tree.lock().await;

        if let Some(ref mut doc) = *curr_doc {
            doc.update(&params.content_changes, params.text_document.version);
            let mut parser = self.parser.lock().await;
            for change in params.content_changes.iter() {
                if let Some(ref mut curr_tree) = *tree {
                    match Self::text_doc_change_to_tree_sitter_edit(change, doc) {
                        Ok(edit) => {
                            debug!("Applying edit: {:?}", edit);
                            curr_tree.edit(&edit);
                            // Reparse after edit
                            if let Some(new_tree) =
                                parser.parse(doc.get_content(None), Some(curr_tree))
                            {
                                *curr_tree = new_tree;
                            }
                        }
                        Err(err) => {
                            self.client
                                .log_message(
                                    MessageType::ERROR,
                                    format!("Failed to edit tree: {}", err),
                                )
                                .await;
                        }
                    }
                }
            }
        }
        debug!("Changed Document is {:?}", curr_doc);
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
        debug!("Triggering completions");
        let uri = params.text_document_position.text_document.uri.to_string();
        let position = params.text_document_position.position;

        let completion_items = self.get_completion_list(&uri, position).await;

        Ok(Some(CompletionResponse::List(CompletionList {
            is_incomplete: false,
            items: completion_items,
        })))
    }
}
