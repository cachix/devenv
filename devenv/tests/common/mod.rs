#![allow(dead_code)]

use core::panic;
use devenv::lsp::Backend;
use fs_extra::dir::CopyOptions;
use std::fmt::Debug;
use std::fs;
use std::io::Write;
use std::path::Path;
use temp_dir::TempDir;

use tokio::io::{duplex, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, DuplexStream};
use tower_lsp::lsp_types::notification::Notification;
use tower_lsp::lsp_types::{InitializedParams, Url, WorkspaceFolder};
use tower_lsp::{jsonrpc, lsp_types, lsp_types::request::Request, LspService, Server};

fn encode_message(content_type: Option<&str>, message: &str) -> String {
    let content_type = content_type
        .map(|ty| format!("\r\nContent-Type: {ty}"))
        .unwrap_or_default();
    format!(
        "Content-Length: {}{}\r\n\r\n{}",
        message.len(),
        content_type,
        message
    )
}

pub struct TestContext {
    pub request_tx: DuplexStream,
    pub response_rx: BufReader<DuplexStream>,
    pub _server: tokio::task::JoinHandle<()>,
    pub request_id: i64,
    pub workspace: TempDir,
}

impl TestContext {
    pub fn new(base: &str) -> Self {
        let (request_tx, req_server) = duplex(1024);
        let (resp_server, response_rx) = duplex(1024);
        let response_rx = BufReader::new(response_rx);
        // create a demo completion json file
        let completion_json = serde_json::json!({ "languages": {
                "python": { "description": "Python language" },
                "nodejs": { "description": "Node.js runtime" }
            },
            "services": {
                "nginx": { "description": "Web server" },
                "redis": { "description": "Cache server" }
            }
        });

        let (service, socket) =
            LspService::build(|client| Backend::new(client, completion_json.clone())).finish();
        let server = tokio::spawn(Server::new(req_server, resp_server, socket).serve(service));
        //
        // create a temporary workspace an init it with our test inputs
        let workspace = TempDir::new().unwrap();
        for item in fs::read_dir(Path::new("tests").join("workspace").join(base)).unwrap() {
            eprintln!("copying {item:?}");
            fs_extra::copy_items(
                &[item.unwrap().path()],
                workspace.path(),
                &CopyOptions::new(),
            )
            .unwrap();
        }

        Self {
            request_tx,
            response_rx,
            _server: server,
            request_id: 0,
            workspace,
        }
    }

    pub fn doc_uri(&self, path: &str) -> Url {
        Url::from_file_path(self.workspace.path().join(path)).unwrap()
    }

    pub async fn recv<R: std::fmt::Debug + serde::de::DeserializeOwned>(&mut self) -> R {
        loop {
            // first line is the content length header
            let mut clh = String::new();
            self.response_rx.read_line(&mut clh).await.unwrap();
            if !clh.starts_with("Content-Length") {
                panic!("missing content length header");
            }
            let length = clh
                .trim_start_matches("Content-Length: ")
                .trim()
                .parse::<usize>()
                .unwrap();
            // next line is just a blank line
            self.response_rx.read_line(&mut clh).await.unwrap();
            // then the message, of the size given by the content length header
            let mut content = vec![0; length];
            self.response_rx.read_exact(&mut content).await.unwrap();
            let content = String::from_utf8(content).unwrap();
            eprintln!("received: {content}");
            std::io::stderr().flush().unwrap();
            // skip log messages
            if content.contains("window/logMessage") {
                continue;
            }
            let response = serde_json::from_str::<jsonrpc::Request>(&content).unwrap();
            let (_method, _id, params) = response.into_parts();
            return serde_json::from_value(params.unwrap()).unwrap();
        }
    }

    pub async fn response<R: std::fmt::Debug + serde::de::DeserializeOwned>(&mut self) -> R {
        loop {
            // first line is the content length header
            let mut clh = String::new();
            self.response_rx.read_line(&mut clh).await.unwrap();
            if !clh.starts_with("Content-Length") {
                panic!("missing content length header");
            }
            let length = clh
                .trim_start_matches("Content-Length: ")
                .trim()
                .parse::<usize>()
                .unwrap();
            // next line is just a blank line
            self.response_rx.read_line(&mut clh).await.unwrap();
            // then the message, of the size given by the content length header
            let mut content = vec![0; length];
            self.response_rx.read_exact(&mut content).await.unwrap();
            let content = String::from_utf8(content).unwrap();
            eprintln!("received: {content}");
            std::io::stderr().flush().unwrap();
            // skip log messages
            if content.contains("window/logMessage") {
                continue;
            }
            let response = serde_json::from_str::<jsonrpc::Response>(&content).unwrap();
            let (_id, result) = response.into_parts();
            return serde_json::from_value(result.unwrap()).unwrap();
        }
    }

    pub async fn send(&mut self, request: &jsonrpc::Request) {
        let content = serde_json::to_string(request).unwrap();
        eprintln!("\nsending: {content}");
        std::io::stderr().flush().unwrap();
        self.request_tx
            .write_all(encode_message(None, &content).as_bytes())
            .await
            .unwrap();
    }

    pub async fn notify<N: Notification>(&mut self, params: N::Params) {
        let notification = jsonrpc::Request::build(N::METHOD)
            .params(serde_json::to_value(params).unwrap())
            .finish();
        self.send(&notification).await;
    }

    pub async fn request<R: Request>(&mut self, params: R::Params) -> R::Result
    where
        R::Result: Debug,
    {
        let request = jsonrpc::Request::build(R::METHOD)
            .id(self.request_id)
            .params(serde_json::to_value(params).unwrap())
            .finish();
        self.request_id += 1;
        self.send(&request).await;
        self.response().await
    }

    pub async fn initialize(&mut self) {
        // a real set of initialize param from helix. We just have to change the workspace configuration
        let initialize = r#"{
        "capabilities": {
          "general": {
            "positionEncodings": [
              "utf-8",
              "utf-32",
              "utf-16"
            ]
          },
          "textDocument": {
            "codeAction": {
              "codeActionLiteralSupport": {
                "codeActionKind": {
                  "valueSet": [
                    "",
                    "quickfix",
                    "refactor",
                    "refactor.extract",
                    "refactor.inline",
                    "refactor.rewrite",
                    "source",
                    "source.organizeImports"
                  ]
                }
              },
              "dataSupport": true,
              "disabledSupport": true,
              "isPreferredSupport": true,
              "resolveSupport": {
                "properties": [
                  "edit",
                  "command"
                ]
              }
            },
            "completion": {
              "completionItem": {
                "deprecatedSupport": true,
                "insertReplaceSupport": true,
                "resolveSupport": {
                  "properties": [
                    "documentation",
                    "detail",
                    "additionalTextEdits"
                  ]
                },
                "snippetSupport": true,
                "tagSupport": {
                  "valueSet": [
                    1
                  ]
                }
              },
              "completionItemKind": {}
            },
            "hover": {
              "contentFormat": [
                "markdown"
              ]
            },
            "inlayHint": {
              "dynamicRegistration": false
            },
            "publishDiagnostics": {
              "tagSupport": {
                "valueSet": [
                  1,
                  2
                ]
              },
              "versionSupport": true
            },
            "rename": {
              "dynamicRegistration": false,
              "honorsChangeAnnotations": false,
              "prepareSupport": true
            },
            "signatureHelp": {
              "signatureInformation": {
                "activeParameterSupport": true,
                "documentationFormat": [
                  "markdown"
                ],
                "parameterInformation": {
                  "labelOffsetSupport": true
                }
              }
            }
          },
          "window": {
            "workDoneProgress": true
          },
          "workspace": {
            "applyEdit": true,
            "configuration": true,
            "didChangeConfiguration": {
              "dynamicRegistration": false
            },
            "didChangeWatchedFiles": {
              "dynamicRegistration": true,
              "relativePatternSupport": false
            },
            "executeCommand": {
              "dynamicRegistration": false
            },
            "fileOperations": {
              "didRename": true,
              "willRename": true
            },
            "inlayHint": {
              "refreshSupport": false
            },
            "symbol": {
              "dynamicRegistration": false
            },
            "workspaceEdit": {
              "documentChanges": true,
              "failureHandling": "abort",
              "normalizesLineEndings": false,
              "resourceOperations": [
                "create",
                "rename",
                "delete"
              ]
            },
            "workspaceFolders": true
          }
        },
        "clientInfo": {
          "name": "helix",
          "version": "24.3 (109f53fb)"
        },
        "processId": 28774,
        "rootPath": "/Users/glehmann/src/earthlyls",
        "rootUri": "file:///Users/glehmann/src/earthlyls",
        "workspaceFolders": [
          {
            "name": "sdk",
            "uri": "file:///Users/glehmann/src/earthlyls"
          }
        ]
      }"#;
        let mut initialize: <lsp_types::request::Initialize as Request>::Params =
            serde_json::from_str(initialize).unwrap();
        let workspace_url = Url::from_file_path(self.workspace.path()).unwrap();
        // initialize.root_path = Some(self.workspace.path().to_string_lossy().to_string());
        initialize.root_uri = Some(workspace_url.clone());
        initialize.workspace_folders = Some(vec![WorkspaceFolder {
            name: "tmp".to_owned(),
            uri: workspace_url.clone(),
        }]);
        self.request::<lsp_types::request::Initialize>(initialize)
            .await;
        self.notify::<lsp_types::notification::Initialized>(InitializedParams {})
            .await;
    }
}
