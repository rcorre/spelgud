use core::panic;
use lsp_server::{Connection, Message};
use lsp_types::notification::{
    DidChangeTextDocument, DidOpenTextDocument, DidSaveTextDocument, PublishDiagnostics,
};
use lsp_types::request::{Completion, DocumentSymbolRequest, Shutdown};
use lsp_types::{notification::Initialized, request::Initialize, InitializedParams};
use lsp_types::{
    CompletionParams, Diagnostic, DiagnosticSeverity, DidChangeTextDocumentParams,
    DidOpenTextDocumentParams, DidSaveTextDocumentParams, DocumentSymbolParams,
    DocumentSymbolResponse, GotoDefinitionParams, InitializeParams, Location, PartialResultParams,
    Position, PublishDiagnosticsParams, Range, SymbolInformation, SymbolKind,
    TextDocumentContentChangeEvent, TextDocumentIdentifier, TextDocumentItem,
    TextDocumentPositionParams, Url, WorkDoneProgressParams,
};
use pretty_assertions::assert_eq;
use spelgud::Result;
use std::error::Error;

fn base_uri() -> Url {
    Url::from_file_path(std::fs::canonicalize("./testdata/example.txt").unwrap()).unwrap()
}

fn diag(uri: Url, target: &str, message: &str) -> Diagnostic {
    Diagnostic {
        range: locate(uri, target).range,
        message: message.into(),
        severity: Some(DiagnosticSeverity::ERROR),
        source: Some("spelgud".into()),
        ..Default::default()
    }
}

fn sym(uri: Url, name: &str, text: &str) -> SymbolInformation {
    let kind = text
        .split_once(" ")
        .unwrap_or_else(|| panic!("Invalid symbol {text}"))
        .0;
    // deprecated field is deprecated, but cannot be omitted
    #[allow(deprecated)]
    SymbolInformation {
        name: name.into(),
        kind: match kind {
            "enum" => SymbolKind::ENUM,
            "message" => SymbolKind::STRUCT,
            _ => panic!("Invalid symbol {text}"),
        },
        tags: None,
        deprecated: None,
        location: locate(uri, text),
        container_name: None,
    }
}

// Generate TextDocumentPositionParams for the given string and offset.
fn position(uri: Url, text: &str, column: u32) -> TextDocumentPositionParams {
    let filetext = std::fs::read_to_string(uri.to_file_path().unwrap()).unwrap();
    let (lineno, line) = filetext
        .lines()
        .enumerate()
        .skip_while(|(_, l)| !l.contains(text))
        .next()
        .unwrap_or_else(|| panic!("{text} not found in {uri}"));

    let character = line.find(text).unwrap_or(0);
    TextDocumentPositionParams {
        text_document: TextDocumentIdentifier { uri: base_uri() },
        position: Position {
            line: lineno.try_into().unwrap(),
            character: column + u32::try_from(character).unwrap(),
        },
    }
}

// Generate a GotoDefinition request for a line containing `text`,
// with the cursor offset from the start of the search string by `offset`
fn goto(uri: Url, text: &str, column: u32) -> GotoDefinitionParams {
    GotoDefinitionParams {
        work_done_progress_params: lsp_types::WorkDoneProgressParams {
            work_done_token: None,
        },
        partial_result_params: lsp_types::PartialResultParams {
            partial_result_token: None,
        },
        text_document_position_params: position(uri, text, column),
    }
}

// Given "some |search| string", locate "some search string" in the document
// and return the Location of "search".
fn locate(uri: Url, text: &str) -> Location {
    let start_off = text.find("|").unwrap();
    let end_off = text.rfind("|").unwrap() - 1;
    let text = text.replace("|", "");
    let filetext = std::fs::read_to_string(uri.to_file_path().unwrap()).unwrap();
    let (start_line, start_col) = filetext
        .lines()
        .enumerate()
        .find_map(|(i, l)| match l.find(text.as_str()) {
            Some(col) => Some((i, col)),
            None => None,
        })
        .unwrap_or_else(|| panic!("{text} not found in {uri}"));

    Location {
        uri,
        range: Range {
            start: Position {
                line: start_line.try_into().unwrap(),
                character: (start_col + start_off).try_into().unwrap(),
            },
            end: Position {
                line: start_line.try_into().unwrap(),
                character: (start_col + end_off).try_into().unwrap(),
            },
        },
    }
}

fn completion_params(uri: Url, position: Position) -> CompletionParams {
    CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position,
        },
        work_done_progress_params: WorkDoneProgressParams {
            work_done_token: None,
        },
        partial_result_params: PartialResultParams {
            partial_result_token: None,
        },
        context: None,
    }
}

fn assert_elements_equal<T, K, F>(mut a: Vec<T>, mut b: Vec<T>, key: F)
where
    T: Clone + std::fmt::Debug + std::cmp::PartialEq,
    K: Ord,
    F: Clone + FnMut(&T) -> K,
{
    a.sort_by_key(key.clone());
    b.sort_by_key(key);

    assert_eq!(a, b);
}

struct TestClient {
    conn: Connection,
    thread: Option<std::thread::JoinHandle<()>>,
    id: i32,
}

impl TestClient {
    fn new() -> Result<TestClient> {
        Self::new_with_root("testdata")
    }

    fn new_with_root(path: impl AsRef<std::path::Path>) -> Result<TestClient> {
        let (client, server) = Connection::memory();
        let thread = std::thread::spawn(|| {
            spelgud::run(server).unwrap();
        });
        let mut client = TestClient {
            conn: client,
            thread: Some(thread),
            id: 0,
        };

        client.request::<Initialize>(InitializeParams {
            root_uri: Some(Url::from_file_path(std::fs::canonicalize(path).unwrap()).unwrap()),
            ..Default::default()
        })?;
        client.notify::<Initialized>(InitializedParams {})?;

        Ok(client)
    }

    fn recv<T>(&self) -> std::result::Result<T::Params, Box<dyn Error>>
    where
        T: lsp_types::notification::Notification,
    {
        match self
            .conn
            .receiver
            .recv_timeout(std::time::Duration::from_secs(5))?
        {
            Message::Request(r) => Err(format!("Expected notification, got: {r:?}"))?,
            Message::Response(r) => Err(format!("Expected notification, got: {r:?}"))?,
            Message::Notification(resp) => {
                assert_eq!(resp.method, T::METHOD, "Unexpected response {resp:?}");
                Ok(serde_json::from_value(resp.params)?)
            }
        }
    }

    fn request<T>(&mut self, params: T::Params) -> spelgud::Result<T::Result>
    where
        T: lsp_types::request::Request,
        T::Params: serde::de::DeserializeOwned,
    {
        let req = Message::Request(lsp_server::Request {
            id: self.id.into(),
            method: T::METHOD.to_string(),
            params: serde_json::to_value(params)?,
        });
        eprintln!("Sending {:?}", req);
        self.id += 1;
        self.conn.sender.send(req)?;
        eprintln!("Waiting");
        match self
            .conn
            .receiver
            .recv_timeout(std::time::Duration::from_secs(5))?
        {
            Message::Request(r) => Err(format!("Expected response, got: {r:?}"))?,
            Message::Notification(r) => Err(format!("Expected response, got: {r:?}"))?,
            Message::Response(resp) if resp.error.is_some() => {
                Err(format!("Got error response {:?}", resp))?
            }
            Message::Response(resp) => Ok(serde_json::from_value(
                resp.result.ok_or("Missing result from response")?,
            )?),
        }
    }

    fn notify<T>(&self, params: T::Params) -> spelgud::Result<()>
    where
        T: lsp_types::notification::Notification,
        T::Params: serde::de::DeserializeOwned,
    {
        self.conn
            .sender
            .send(Message::Notification(lsp_server::Notification {
                method: T::METHOD.to_string(),
                params: serde_json::to_value(params)?,
            }))?;
        Ok(())
    }

    fn open(&self, uri: Url) -> spelgud::Result<PublishDiagnosticsParams> {
        let text = std::fs::read_to_string(uri.path())?;
        self.notify::<DidOpenTextDocument>(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri,
                language_id: "".into(),
                version: 0,
                text,
            },
        })?;
        self.recv::<PublishDiagnostics>()
    }
}

impl Drop for TestClient {
    fn drop(&mut self) {
        self.request::<Shutdown>(()).unwrap();
        self.notify::<lsp_types::notification::Exit>(()).unwrap();
        self.thread.take().unwrap().join().unwrap();
    }
}

#[test]
fn test_start_stop() -> spelgud::Result<()> {
    TestClient::new()?;
    Ok(())
}

#[test]
fn test_open() -> spelgud::Result<()> {
    let client = TestClient::new()?;

    assert_eq!(
        client.open(base_uri())?,
        PublishDiagnosticsParams {
            uri: base_uri(),
            diagnostics: vec![],
            version: None,
        }
    );
    Ok(())
}

#[test]
fn test_diagnostics_on_open() -> spelgud::Result<()> {
    let client = TestClient::new()?;

    let diags = client.open(base_uri())?;
    assert_eq!(diags.uri, base_uri());
    assert_elements_equal(diags.diagnostics, vec![], |s| s.message.clone());
    Ok(())
}

#[test]
fn test_diagnostics_on_save() -> spelgud::Result<()> {
    let tmp = tempfile::tempdir()?;
    let path = tmp.path().join("example.proto");
    let uri = Url::from_file_path(&path).unwrap();
    let client = TestClient::new_with_root(&tmp)?;

    let text = r#"
syntax = "proto3";
package main;
message Foo{}
"#;
    std::fs::write(&path, text)?;

    let diags = client.open(uri.clone())?;
    assert_eq!(
        diags,
        PublishDiagnosticsParams {
            uri: uri.clone(),
            diagnostics: vec![],
            version: None,
        }
    );

    // modify the file, check that we pick up the change
    let text = r#"
syntax = "proto3";
package main;
message Foo{Flob flob = 1;}
"#;
    std::fs::write(&path, text)?;

    let start = lsp_types::Position {
        line: 3,
        character: "message Foo{ ".len() as u32,
    };
    client.notify::<DidChangeTextDocument>(DidChangeTextDocumentParams {
        text_document: lsp_types::VersionedTextDocumentIdentifier {
            uri: uri.clone(),
            version: 0,
        },
        content_changes: vec![TextDocumentContentChangeEvent {
            text: "Flob flob = 1;".into(),
            range: Some(lsp_types::Range { start, end: start }),
            range_length: None,
        }],
    })?;

    client.notify::<DidSaveTextDocument>(DidSaveTextDocumentParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
        text: None,
    })?;
    let diags = client.recv::<PublishDiagnostics>()?;
    assert_eq!(
        diags,
        PublishDiagnosticsParams {
            uri: uri.clone(),
            diagnostics: vec![],
            version: None,
        }
    );

    Ok(())
}

#[test]
fn test_document_symbols() -> spelgud::Result<()> {
    let mut client = TestClient::new()?;
    client.open(base_uri())?;

    let Some(DocumentSymbolResponse::Flat(actual)) =
        client.request::<DocumentSymbolRequest>(DocumentSymbolParams {
            text_document: TextDocumentIdentifier {
                uri: base_uri().clone(),
            },
            work_done_progress_params: lsp_types::WorkDoneProgressParams {
                work_done_token: None,
            },
            partial_result_params: lsp_types::PartialResultParams {
                partial_result_token: None,
            },
        })?
    else {
        panic!("Expected DocumentSymbolResponse::Flat")
    };
    assert_elements_equal(actual, vec![], |s| s.name.clone());
    Ok(())
}

#[test]
fn test_references() -> spelgud::Result<()> {
    let mut client = TestClient::new()?;
    client.open(base_uri())?;

    // TODO
    return Ok(());

    assert_eq!(
        client.request::<lsp_types::request::References>(lsp_types::ReferenceParams {
            text_document_position: position(base_uri(), "message Foo", 9),
            work_done_progress_params: lsp_types::WorkDoneProgressParams {
                work_done_token: None,
            },
            partial_result_params: lsp_types::PartialResultParams {
                partial_result_token: None
            },
            context: lsp_types::ReferenceContext {
                include_declaration: false,
            },
        })?,
        Some(vec![])
    );

    Ok(())
}

#[test]
fn test_complete() -> spelgud::Result<()> {
    let mut client = TestClient::new()?;
    client.open(base_uri())?;

    let resp = client.request::<Completion>(completion_params(
        base_uri(),
        Position {
            line: 2,
            character: 0,
        },
    ))?;

    // TODO
    return Ok(());

    let Some(lsp_types::CompletionResponse::Array(actual)) = resp else {
        panic!("Unexpected completion response {resp:?}");
    };

    assert_elements_equal(actual, vec![], |s| s.label.clone());

    Ok(())
}
