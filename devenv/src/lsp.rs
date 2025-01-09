use lsp_textdocument::FullTextDocument;
use regex::Regex;
use serde::de;
use serde_json::Value;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};
use tracing::{debug, info};
use tree_sitter::{Node, Parser, Point, Tree};

pub struct Backend {
    client: Client,
    curr_doc: std::sync::Arc<tokio::sync::Mutex<Option<FullTextDocument>>>,
    tree: std::sync::Arc<tokio::sync::Mutex<Option<Tree>>>,
    completion_json: Value,
    parser: std::sync::Arc<tokio::sync::Mutex<Parser>>,
    root_level_json_completion: Vec<String>, // New field
}

impl Backend {
    pub fn new(client: Client, completion_json: Value) -> Self {
        let mut parser = Parser::new();
        let json_search_result =
            Backend::search_json_static(&completion_json, &["".to_string()], "");
        let root_level_json_completion = json_search_result
            .iter()
            .map(|(k, _)| k.clone())
            .collect::<Vec<String>>();
        parser
            .set_language(tree_sitter_nix::language())
            .expect("Unable to load the nix language file");

        Backend {
            client,
            curr_doc: std::sync::Arc::new(tokio::sync::Mutex::new(None)),
            tree: std::sync::Arc::new(tokio::sync::Mutex::new(None)),
            completion_json,
            parser: std::sync::Arc::new(tokio::sync::Mutex::new(parser)),
            root_level_json_completion: root_level_json_completion,
        }
    }

    fn search_json_static(
        completion_json: &Value,
        path: &[String],
        partial_key: &str,
    ) -> Vec<(String, Option<String>)> {
        let mut current = completion_json;

        for key in path {
            match current.get(key) {
                Some(value) => current = value,
                None => {
                    current = completion_json;
                    break;
                }
            }
        }

        match current {
            Value::Object(map) => map
                .iter()
                .filter(|(k, _)| {
                    k.to_lowercase().contains(&partial_key.to_lowercase()) || partial_key.is_empty()
                })
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

    fn print_ast(&self, node: Node, source: &str, depth: usize) {
        let indent = "  ".repeat(depth);
        println!(
            "{}{} [{}:{}] - [{}:{}]  -> {}",
            indent,
            node.kind(),
            node.start_position().row,
            node.start_position().column,
            node.end_position().row,
            node.end_position().column,
            node.utf8_text(source.as_bytes()).unwrap_or_default()
        );

        debug!(
            "{}{} [{}:{}] - [{}:{}]  -> {}",
            indent,
            node.kind(),
            node.start_position().row,
            node.start_position().column,
            node.end_position().row,
            node.end_position().column,
            node.utf8_text(source.as_bytes()).unwrap_or_default()
        );

        // Recursively print children
        let mut cursor = node.walk();
        if cursor.goto_first_child() {
            loop {
                self.print_ast(cursor.node(), source, depth + 1);
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
        }
    }

    async fn get_completion_list(
        &self,
        uri: &str,
        position: Position,
        params: &CompletionParams,
    ) -> Vec<CompletionItem> {
        let curr_doc = self.curr_doc.lock().await;
        let tree = self.tree.lock().await;

        let doc = match &*curr_doc {
            Some(doc) => doc,
            None => return vec![],
        };
        let tree = match &*tree {
            Some(tree) => tree,
            None => return vec![],
        };

        let content = doc.get_content(None);
        let root_node = tree.root_node();
        let point = Point::new(position.line as usize, position.character as usize);

        let scope_path = self.get_scope(root_node, point, &content);
        debug!("Scope path: {:?}", scope_path);
        println!("Scope path: {:?}", scope_path);
        let line_content = content
            .lines()
            .nth(position.line as usize)
            .unwrap_or_default();
        let line_until_cursor = &line_content[..position.character as usize];
        let dot_path = self.get_path(line_until_cursor);

        if let Some(context) = &params.context {
            if let Some(trigger_char) = &context.trigger_character {
                if trigger_char == "=" {
                    let search_path = [scope_path.clone(), dot_path].concat();
                    let default_value = self.get_default_value(&search_path);
                    if default_value.is_empty() {
                        return vec![];
                    }
                    return vec![CompletionItem {
                        label: format!("= {}", default_value),
                        kind: Some(CompletionItemKind::VALUE),
                        insert_text: Some(format!("= {}", default_value)),
                        ..Default::default()
                    }];
                }
            }
        }

        let re = Regex::new(r".*\W(.*)").unwrap();
        let current_word = re
            .captures(line_until_cursor)
            .and_then(|caps| caps.get(1))
            .map(|m| m.as_str())
            .unwrap_or("");

        let search_path = [scope_path.clone(), dot_path].concat();
        debug!("Current word: {:?}", current_word);
        debug!("Search path: {:?}", search_path);
        println!("Search path: {:?}", search_path);
        let completions = self.search_json(&search_path, current_word);

        completions
            .into_iter()
            .map(|(item, description)| {
                CompletionItem::new_simple(item, description.unwrap_or_default())
            })
            .collect()
    }

    // Add this helper method
    fn get_default_value(&self, path: &[String]) -> String {
        let mut current = &self.completion_json;

        for key in path {
            match current.get(key) {
                Some(value) => current = value,
                None => return String::new(),
            }
        }

        match current {
            Value::Object(map) => {
                if let Some(Value::String(default)) = map.get("default") {
                    default.clone()
                } else {
                    String::new()
                }
            }
            _ => String::new(),
        }
    }

    fn extract_scope_from_sibling(&self, current_node: Node, source: &str) -> Vec<String> {
        let mut scope = Vec::new();
        let mut current_node = current_node;

        while let Some(sibling) = current_node.prev_named_sibling() {
            let sibling_value = sibling.utf8_text(source.as_bytes()).unwrap_or_default();
            let sibling_kind = sibling.kind();
            println!(
                "Sibling kind: {:?} | value: {:?}",
                sibling_kind, sibling_value
            );

            if sibling_kind == "attrpath" {
                if self
                    .root_level_json_completion
                    .contains(&&sibling_value.to_string())
                {
                    scope.push(sibling_value.to_string());
                    break;
                }
                scope.push(sibling_value.to_string());
            }

            current_node = sibling
        }

        // scope.reverse();
        scope
    }

    fn get_scope(&self, root_node: Node, cursor_position: Point, source: &str) -> Vec<String> {
        // self.print_ast(root_node, source, 0);
        debug!("Getting scope for cursor position: {:?}", cursor_position);

        let node = match root_node.descendant_for_point_range(cursor_position, cursor_position) {
            Some(node) => node,
            None => return Vec::new(),
        };

        // Try different node types in order of priority
        let scope = self
            .try_error_node(node, cursor_position, root_node, source)
            .or_else(|| self.try_formals_node(node, source))
            .or_else(|| self.try_attrset_node(node, source))
            .or_else(|| self.try_attrpath_node(node, source))
            .unwrap_or_default();

        debug!("Final scope: {:?}", scope);
        scope
    }

    fn try_error_node(
        &self,
        node: Node,
        cursor_position: Point,
        root_node: Node,
        source: &str,
    ) -> Option<Vec<String>> {
        if node.kind() != "ERROR" {
            return None;
        }
        debug!("Inside the ERROR current_node kind");
        let prev_point = Point {
            row: cursor_position.row,
            column: cursor_position.column.saturating_sub(1),
        };
        root_node
            .descendant_for_point_range(prev_point, prev_point)
            .map(|new_node| {
                debug!("new node kind: {:?}", new_node.kind());
                if new_node.kind() == "=" {
                    let mut scope = self.try_formals_node(node, source);
                    debug!("Scope: {:?}", scope);
                }
                let mut scope = self.extract_scope_from_sibling(new_node, source);
                debug!("Scope: {:?}", scope);
                if scope.is_empty() {
                    debug!("Scope is empty, trying parent");
                    if let Some(parent_node) = new_node.parent() {
                        debug!(
                            "Parent node kind: {:?}, with value {:?}",
                            parent_node.kind(),
                            parent_node.utf8_text(source.as_bytes()).unwrap_or_default()
                        );
                        scope = self
                            .try_formals_node(parent_node, source)
                            .unwrap_or_default();
                    }
                }
                scope.reverse();
                scope
            })
    }

    fn try_formals_node(&self, node: Node, source: &str) -> Option<Vec<String>> {
        let mut current_node = node;
        while current_node.kind() != "formals" {
            current_node = current_node.parent()?;
        }

        if current_node.kind() == "formals" {
            let mut scope = self.extract_scope_from_sibling(current_node, source);
            scope.reverse();
            Some(scope)
        } else {
            None
        }
    }

    fn try_attrset_node(&self, node: Node, source: &str) -> Option<Vec<String>> {
        debug!("Trying attrset_expression node");
        let mut current_node = node;
        while current_node.kind() != "attrset_expression" {
            current_node = current_node.parent()?;
        }

        if current_node.kind() == "attrset_expression" {
            let mut scope = self.extract_scope_from_sibling(current_node, source);
            scope.reverse();
            Some(scope)
        } else {
            None
        }
    }

    fn try_attrpath_node(&self, node: Node, source: &str) -> Option<Vec<String>> {
        debug!("Trying attrpath node");
        let mut current_node = node;
        while current_node.kind() != "attrpath" {
            current_node = current_node.parent()?;
        }

        if current_node.kind() == "attrpath" {
            let mut scope = self.extract_scope_from_sibling(current_node, source);
            scope.reverse();
            Some(scope)
        } else {
            None
        }
    }

    fn get_path(&self, line: &str) -> Vec<String> {
        let mut path = Vec::new();

        // Handle empty or whitespace-only lines
        if line.trim().is_empty() {
            return path;
        }

        // Split by dots and handle the special case where we're typing after a dot
        let parts: Vec<&str> = line.split('.').collect();
        if parts.len() > 1 {
            // Take all complete parts before the cursor
            for part in &parts[..parts.len() - 1] {
                let trimmed = part.trim();
                if !trimmed.is_empty() {
                    path.push(trimmed.to_string());
                }
            }
        }

        path
    }

    fn search_json(&self, path: &[String], partial_key: &str) -> Vec<(String, Option<String>)> {
        let mut current = &self.completion_json;

        // First try the exact path
        for key in path {
            match current.get(key) {
                Some(value) => current = value,
                None => {
                    // If exact path fails, try searching at root level
                    current = &self.completion_json;
                    break;
                }
            }
        }

        match current {
            Value::Object(map) => map
                .iter()
                .filter(|(k, _)| {
                    k.to_lowercase().contains(&partial_key.to_lowercase()) || partial_key.is_empty()
                })
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
                    trigger_characters: Some(vec![".".to_string(), "=".to_string()]),
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

        let completion_items = self.get_completion_list(&uri, position, &params).await;

        Ok(Some(CompletionResponse::List(CompletionList {
            is_incomplete: true,
            items: completion_items,
        })))
    }
}
