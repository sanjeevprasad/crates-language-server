use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::fs;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

type CachedDependency = HashMap<String, Dependency>;

#[derive(Serialize, Deserialize)]
struct CrateVersion {
    num: String,
    yanked: bool,
}

#[derive(Serialize, Deserialize)]
struct CrateVersions {
    versions: Option<Vec<CrateVersion>>,
    errors: Option<Vec<Value>>,
}

struct Dependency {
    last_updated_at: u128,
    latest: String,
}
struct Backend {
    client: Client,
    cached_dependencies: Arc<Mutex<CachedDependency>>,
}

// impl Backend {
//     async fn read_cached_dependencies(&self) {
//         let home = std::env::var("HOME").expect("HOME dir expected");
//         let content = match fs::read_to_string(file_path).await {
//             Ok(text) => text,
//             Err(err) => {
//                 eprintln!("error opening file {err}");
//                 return Ok(None);
//             }
//         };
//         let lines = src.split("\n").collect::<Vec<_>>();
//         let value = match serde_json::from_str::<CachedDependency>(&src) {
//             Ok(v) => v,
//             Err(_err) => return Ok(None),
//         };
//     }
// }

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            server_info: None,
            capabilities: ServerCapabilities {
                // definition_provider: Some(OneOf::Left(true)),
                inlay_hint_provider: Some(OneOf::Right(InlayHintServerCapabilities::Options(
                    InlayHintOptions {
                        work_done_progress_options: Default::default(),
                        resolve_provider: None,
                    },
                ))),
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::INCREMENTAL,
                )),
                // completion_provider: Some(CompletionOptions {
                //     resolve_provider: Some(false),
                //     trigger_characters: Some(vec![".".to_string()]),
                //     work_done_progress_options: Default::default(),
                //     all_commit_characters: None,
                //     ..Default::default()
                // }),
                // execute_command_provider: Some(ExecuteCommandOptions {
                //     commands: vec!["dummy.do_something".to_string()],
                //     work_done_progress_options: Default::default(),
                // }),
                // workspace: Some(WorkspaceServerCapabilities {
                //     workspace_folders: Some(WorkspaceFoldersServerCapabilities {
                //         supported: Some(true),
                //         change_notifications: Some(OneOf::Left(true)),
                //     }),
                //     file_operations: None,
                // }),
                ..ServerCapabilities::default()
            },
            ..Default::default()
        })
    }

    async fn inlay_hint(&self, params: InlayHintParams) -> Result<Option<Vec<InlayHint>>> {
        eprintln!("inlay_hint {params:?}");
        let mut hints = Vec::new();
        let file_path = params.text_document.uri.path();
        let src = match fs::read_to_string(file_path).await {
            Ok(text) => text,
            Err(err) => {
                eprintln!("error opening file {err}");
                return Ok(None);
            }
        };
        let lines = src.split("\n").collect::<Vec<_>>();
        let value = match toml::from_str::<serde_json::Value>(&src) {
            Ok(v) => v,
            Err(_err) => return Ok(None),
        };
        let dependencies = match value["dependencies"].as_object() {
            Some(dep) => dep,
            None => return Ok(None),
        };
        struct Dep {
            krate: String,
            curr_version: String,
            position: Position,
        }
        let mut deps = Vec::new();
        for (krate, value) in dependencies.iter() {
            let version = if value.is_object() {
                value["version"].as_str().unwrap_or("invalid")
            } else if value.is_string() {
                value.as_str().unwrap_or("invalid")
            } else {
                eprintln!("failed to parse crate {krate}");
                continue;
            };
            let mut i = 0;
            for l in &lines {
                i += 1;
                let dep_line = l.trim_start();
                if dep_line.starts_with(krate) {
                    if dep_line.len() > krate.len() {
                        let crate_end_char = &dep_line[krate.len()..krate.len() + 1];
                        if crate_end_char == "-" || crate_end_char == "_" {
                            continue;
                        }
                        deps.push(Dep {
                            krate: krate.to_owned(),
                            position: Position {
                                line: i - 1,
                                character: l.len() as u32,
                            },
                            curr_version: version.to_owned(),
                        });
                    }
                }
            }
        }
        let client = reqwest::ClientBuilder::default()
            .user_agent("crate-lsp")
            .build()
            .unwrap();
        let client = Arc::new(client);
        let mut tasks = Vec::new();
        const HOUR_MS: u128 = Duration::from_secs(3600).as_millis();
        for dep in &deps {
            let krate = dep.krate.to_owned();
            let cached_dependencies = self.cached_dependencies.clone();
            if let Some(cache) = cached_dependencies.lock().await.get(&krate) {
                match now_millis() - cache.last_updated_at < HOUR_MS {
                    true => continue,
                    false => eprintln!("cache-busting {krate}"),
                }
            }
            let client = client.clone();
            let task: JoinHandle<
                std::result::Result<(), Box<dyn std::error::Error + Send + Sync>>,
            > = tokio::spawn(async move {
                let url = format!("https://crates.io/api/v1/crates/{krate}/versions");
                let res = client.get(url).send().await?;
                let value = res.json::<CrateVersions>().await?;
                if let Some(errors) = value.errors {
                    for error in errors {
                        eprintln!("error {error}");
                    }
                };
                if let Some(versions) = value.versions {
                    if let Some(version) = versions.first() {
                        cached_dependencies.lock().await.insert(
                            krate.to_owned(),
                            Dependency {
                                last_updated_at: now_millis(),
                                latest: version.num.to_owned(),
                            },
                        );
                    }
                }
                Ok(())
            });
            tasks.push(task);
        }
        for t in tasks {
            if let Err(err) = t.await {
                eprintln!("Error {err}");
            }
        }
        let cached_dependencies = self.cached_dependencies.lock().await;
        for dep in deps {
            let label = match cached_dependencies.get(&dep.krate) {
                Some(cache) => match cache.latest == dep.curr_version || dep.curr_version == "*" {
                    true => format!(" ✔ latest: {}", cache.latest),
                    false => format!(" 𐄂 available: {}", cache.latest),
                },
                None => " error".to_owned(),
            };
            let hint = InlayHint {
                position: dep.position,
                label: InlayHintLabel::String(label),
                padding_left: Some(true),
                padding_right: Some(true),
                kind: Some(InlayHintKind::TYPE),
                text_edits: None,
                tooltip: None,
                data: None,
            };
            hints.push(hint);
        }
        Ok(Some(hints))
    }
    async fn initialized(&self, _: InitializedParams) {
        eprintln!("\n###################### crates-lsp initialized ####################");
    }

    async fn did_change_workspace_folders(&self, _: DidChangeWorkspaceFoldersParams) {
        // eprintln!("workspace folders changed!");
    }

    async fn did_change_configuration(&self, _: DidChangeConfigurationParams) {
        // eprintln!("configuration changed!");
    }

    async fn did_change_watched_files(&self, _: DidChangeWatchedFilesParams) {
        // eprintln!("watched files have changed!");
    }

    async fn execute_command(&self, _: ExecuteCommandParams) -> Result<Option<Value>> {
        eprintln!("command executed!");
        match self.client.apply_edit(WorkspaceEdit::default()).await {
            Ok(res) if res.applied => eprintln!("applied"),
            Ok(_) => eprintln!("rejected"),
            Err(err) => eprintln!("{err:}"),
        }

        Ok(None)
    }

    async fn did_open(&self, _: DidOpenTextDocumentParams) {
        eprintln!("file opened!");
    }

    async fn did_change(&self, _: DidChangeTextDocumentParams) {
        eprintln!("file changed!");
    }

    async fn did_save(&self, _: DidSaveTextDocumentParams) {
        eprintln!("file saved!");
    }

    async fn did_close(&self, _: DidCloseTextDocumentParams) {
        eprintln!("file closed!");
    }

    async fn completion(&self, _: CompletionParams) -> Result<Option<CompletionResponse>> {
        Ok(Some(CompletionResponse::Array(vec![
            CompletionItem::new_simple("Hello".to_string(), "Some detail".to_string()),
            CompletionItem::new_simple("Bye".to_string(), "More detail".to_string()),
        ])))
    }
    async fn shutdown(&self) -> Result<()> {
        eprintln!("######### crates-lsp shutdown ########");
        Ok(())
    }
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Failed to get epock")
        .as_millis()
}

#[tokio::main]
async fn main() {
    let (stdin, stdout) = (tokio::io::stdin(), tokio::io::stdout());
    let (service, socket) = LspService::new(|client| Backend {
        client,
        cached_dependencies: Arc::new(Mutex::new(HashMap::new())),
    });
    Server::new(stdin, stdout, socket).serve(service).await;
}
