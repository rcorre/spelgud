mod file;
mod spell;
mod workspace;

use lsp_types::notification::DidChangeTextDocument;
use lsp_types::request::CodeActionRequest;
use lsp_types::request::Completion;
use lsp_types::CodeAction;
use lsp_types::CodeActionKind;
use lsp_types::CodeActionParams;
use lsp_types::CodeActionResponse;
use lsp_types::CompletionParams;
use lsp_types::CompletionResponse;
use lsp_types::DidChangeTextDocumentParams;
use lsp_types::ReferenceParams;
use lsp_types::SaveOptions;
use lsp_types::TextDocumentSyncKind;
use lsp_types::TextEdit;

use lsp_server::{Connection, Message};
use lsp_types::request::References;
use lsp_types::request::{DocumentSymbolRequest, Request};
use lsp_types::{
    notification::{DidOpenTextDocument, DidSaveTextDocument, Notification, PublishDiagnostics},
    DiagnosticServerCapabilities, InitializeParams, ServerCapabilities, TextDocumentSyncCapability,
    TextDocumentSyncOptions, TextDocumentSyncSaveOptions,
};
use lsp_types::{
    DidOpenTextDocumentParams, DidSaveTextDocumentParams, DocumentSymbolParams,
    DocumentSymbolResponse, OneOf,
};
use std::error::Error;

pub type Result<T> = std::result::Result<T, Box<dyn Error>>;

// Handle a request, returning the response to send.
fn handle<Req>(
    workspace: &mut workspace::Workspace,
    req: lsp_server::Request,
    handler: impl Fn(&mut workspace::Workspace, Req::Params) -> Result<Req::Result>,
) -> Result<lsp_server::Message>
where
    Req: lsp_types::request::Request,
{
    let (id, params) = req.extract::<Req::Params>(Req::METHOD)?;
    Ok(Message::Response(match handler(workspace, params) {
        Ok(resp) => lsp_server::Response {
            id,
            result: Some(serde_json::to_value(resp)?),
            error: None,
        },
        Err(err) => lsp_server::Response {
            id,
            result: None,
            error: Some(lsp_server::ResponseError {
                code: lsp_server::ErrorCode::InternalError as i32,
                message: err.to_string(),
                data: None,
            }),
        },
    }))
}

// Handle a notification, optionally returning a notification to send in response.
fn notify<N>(
    workspace: &mut workspace::Workspace,
    not: lsp_server::Notification,
    handler: impl Fn(&mut workspace::Workspace, N::Params) -> Result<Option<lsp_server::Notification>>,
) -> Result<Option<lsp_server::Message>>
where
    N: lsp_types::notification::Notification,
{
    let params = not.extract::<N::Params>(N::METHOD)?;
    Ok(match handler(workspace, params) {
        Ok(Some(resp)) => Some(Message::Notification(resp)),
        Ok(None) => None,
        // If we get an error, we can't respond directly as with a Request.
        // Instead, send a ShowMessage notification with the error.
        Err(err) => Some(Message::Notification(lsp_server::Notification {
            method: lsp_types::notification::ShowMessage::METHOD.into(),
            params: serde_json::to_value(lsp_types::ShowMessageParams {
                typ: lsp_types::MessageType::ERROR,
                message: err.to_string(),
            })?,
        })),
    })
}

fn handle_document_symbols(
    workspace: &mut workspace::Workspace,
    params: DocumentSymbolParams,
) -> Result<Option<DocumentSymbolResponse>> {
    Ok(Some(DocumentSymbolResponse::Flat(
        workspace.symbols(&params.text_document.uri)?,
    )))
}

fn handle_references(
    workspace: &mut workspace::Workspace,
    params: ReferenceParams,
) -> Result<Option<Vec<lsp_types::Location>>> {
    Ok(None)
}

fn handle_completion(
    workspace: &mut workspace::Workspace,
    params: CompletionParams,
) -> Result<Option<CompletionResponse>> {
    let pos = params.text_document_position.position;
    let uri = params.text_document_position.text_document.uri;
    workspace.complete(&uri, pos.line.try_into()?, pos.character.try_into()?)
}

fn handle_code_action(
    workspace: &mut workspace::Workspace,
    params: CodeActionParams,
) -> Result<Option<CodeActionResponse>> {
    eprintln!("Got action {params:?}");
    let uri = params.text_document.uri;
    let mut res = vec![];
    for diag in params.context.diagnostics {
        log::trace!("Generating actions for {diag:?}");
        // If data is None, there are no suggestions
        let Some(data) = diag.data else {
            continue;
        };
        let data: spell::DiagnosticData = serde_json::from_value(data)?;
        res.extend(data.fixes.iter().map(|fix| {
            lsp_types::CodeActionOrCommand::CodeAction(CodeAction {
                title: format!("Change {} to {}", data.original, fix),
                kind: Some(CodeActionKind::QUICKFIX),
                diagnostics: None,
                edit: Some(lsp_types::WorkspaceEdit {
                    changes: Some(
                        [(
                            uri.clone(),
                            vec![TextEdit {
                                range: data.range,
                                new_text: fix.to_owned(),
                            }],
                        )]
                        .iter()
                        .cloned()
                        .collect(),
                    ),
                    ..Default::default()
                }),
                data: None,
                ..Default::default()
            })
        }));
    }
    Ok(Some(res))
}

fn notify_did_open(
    workspace: &mut workspace::Workspace,
    params: DidOpenTextDocumentParams,
) -> Result<Option<lsp_server::Notification>> {
    let uri = params.text_document.uri;
    let diags = workspace.open(uri.clone(), params.text_document.text)?;

    let params = lsp_types::PublishDiagnosticsParams {
        uri,
        diagnostics: diags,
        version: None,
    };

    Ok(Some(lsp_server::Notification {
        method: PublishDiagnostics::METHOD.into(),
        params: serde_json::to_value(&params)?,
    }))
}

fn notify_did_save(
    workspace: &mut workspace::Workspace,
    params: DidSaveTextDocumentParams,
) -> Result<Option<lsp_server::Notification>> {
    let uri = params.text_document.uri;
    let diags = workspace.save(uri.clone())?;

    let params = lsp_types::PublishDiagnosticsParams {
        uri,
        diagnostics: diags,
        version: None,
    };

    Ok(Some(lsp_server::Notification {
        method: PublishDiagnostics::METHOD.into(),
        params: serde_json::to_value(&params)?,
    }))
}

fn notify_did_change(
    workspace: &mut workspace::Workspace,
    params: DidChangeTextDocumentParams,
) -> Result<Option<lsp_server::Notification>> {
    let uri = params.text_document.uri;
    workspace.edit(&uri, params.content_changes)?;
    Ok(None)
}

pub fn run(connection: Connection) -> Result<()> {
    let server_capabilities = serde_json::to_value(&ServerCapabilities {
        // BUG: technically we are supposed to support UTF-16.
        // From what I've seen editors seem to be happy with UTF-8.
        position_encoding: Some(lsp_types::PositionEncodingKind::UTF8),
        document_symbol_provider: Some(OneOf::Left(true)),
        workspace_symbol_provider: Some(OneOf::Left(true)),
        references_provider: Some(OneOf::Left(true)),
        text_document_sync: Some(TextDocumentSyncCapability::Options(
            TextDocumentSyncOptions {
                open_close: Some(true),
                save: Some(TextDocumentSyncSaveOptions::SaveOptions(SaveOptions {
                    include_text: Some(false),
                })),
                change: Some(TextDocumentSyncKind::INCREMENTAL),
                ..Default::default()
            },
        )),
        completion_provider: Some(lsp_types::CompletionOptions {
            trigger_characters: Some(vec!["\"".into()]),
            ..Default::default()
        }),
        diagnostic_provider: Some(DiagnosticServerCapabilities::Options(
            lsp_types::DiagnosticOptions {
                identifier: Some(String::from("spelgud")),
                workspace_diagnostics: true,
                ..Default::default()
            },
        )),
        code_action_provider: Some(lsp_types::CodeActionProviderCapability::Simple(true)),
        ..Default::default()
    })
    .unwrap();

    log::info!("Initializing");
    let init_params = connection.initialize(server_capabilities)?;
    let _params: InitializeParams = serde_json::from_value(init_params).unwrap();

    let mut workspace = workspace::Workspace::new()?;

    for msg in &connection.receiver {
        log::info!("Handling message {msg:?}");
        match msg {
            Message::Request(req) => {
                if connection.handle_shutdown(&req)? {
                    log::info!("Shutting down");
                    return Ok(());
                }
                let resp = match req.method.as_str() {
                    DocumentSymbolRequest::METHOD => Some(handle::<DocumentSymbolRequest>(
                        &mut workspace,
                        req,
                        handle_document_symbols,
                    )),
                    References::METHOD => {
                        Some(handle::<References>(&mut workspace, req, handle_references))
                    }
                    Completion::METHOD => {
                        Some(handle::<Completion>(&mut workspace, req, handle_completion))
                    }
                    CodeActionRequest::METHOD => Some(handle::<CodeActionRequest>(
                        &mut workspace,
                        req,
                        handle_code_action,
                    )),
                    _ => None,
                };
                if let Some(resp) = resp {
                    connection.sender.send(resp?)?;
                }
            }
            Message::Response(_) => {}
            Message::Notification(not) => {
                let resp = match not.method.as_str() {
                    DidOpenTextDocument::METHOD => {
                        notify::<DidOpenTextDocument>(&mut workspace, not, notify_did_open)?
                    }
                    DidSaveTextDocument::METHOD => {
                        notify::<DidSaveTextDocument>(&mut workspace, not, notify_did_save)?
                    }
                    DidChangeTextDocument::METHOD => {
                        notify::<DidChangeTextDocument>(&mut workspace, not, notify_did_change)?
                    }
                    _ => None,
                };
                if let Some(resp) = resp {
                    connection.sender.send(resp)?;
                }
            }
        }
    }
    Ok(())
}
