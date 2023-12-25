use std::error::Error;

use lsp_server::{Connection, ExtractError, Message, Request, RequestId, Response};
use lsp_types::request::*;
use lsp_types::{
    CompletionOptions, GotoDefinitionResponse, InitializeParams, InlayHint, InlayHintKind,
    InlayHintLabel, InlayHintLabelPart, InlayHintOptions, InlayHintServerCapabilities, OneOf,
    Position, Range, ServerCapabilities,
};

fn main() -> Result<(), Box<dyn Error + Sync + Send>> {
    eprintln!("##################################### starting cargo-language-server #########################################");
    let (connection, io_threads) = Connection::stdio();
    let server_capabilities = serde_json::to_value(&ServerCapabilities {
        // definition_provider: Some(OneOf::Left(true)),
        inlay_hint_provider: Some(OneOf::Right(InlayHintServerCapabilities::Options(
            InlayHintOptions {
                work_done_progress_options: Default::default(),
                resolve_provider: Some(true),
            },
        ))),
        completion_provider: Some(CompletionOptions {
            ..Default::default()
        }),
        ..Default::default()
    })?;
    let params = connection.initialize(server_capabilities)?;
    let _params: InitializeParams = serde_json::from_value(params)?;
    for msg in &connection.receiver {
        match msg {
            Message::Request(req) => {
                if connection.handle_shutdown(&req)? {
                    return Ok(());
                }
                let req = match parse::<InlayHintRequest>(req) {
                    Ok((id, params)) => {
                        eprintln!("INLAY_HINT #{id}: {params:?}");
                        let result = InlayHint {
                            position: Position {
                                line: 1,
                                character: 1,
                            },
                            label: InlayHintLabel::LabelParts(vec![InlayHintLabelPart {
                                value: "working".to_owned(),
                                tooltip: None,
                                location: Some(lsp_types::Location {
                                    uri: params.text_document.uri.clone(),
                                    range: Range {
                                        start: Position::new(0, 4),
                                        end: Position::new(0, 5),
                                    },
                                }),
                                command: None,
                            }]),
                            padding_left: Some(true),
                            padding_right: Some(true),
                            kind: Some(InlayHintKind::TYPE),
                            text_edits: None,
                            tooltip: None,
                            data: None,
                        };
                        let response = Response {
                            id,
                            result: Some(serde_json::to_value(&result)?),
                            error: None,
                        };
                        connection.sender.send(Message::Response(response))?;
                        continue;
                    }
                    Err(err @ ExtractError::JsonError { .. }) => panic!("{err:?}"),
                    Err(ExtractError::MethodMismatch(req)) => req,
                };
                let req = match parse::<GotoDefinition>(req) {
                    Ok((id, params)) => {
                        eprintln!("GOTO_DEFININTION #{id}: {params:?}");
                        let result = GotoDefinitionResponse::Array(Vec::new());
                        let resp = Response {
                            id,
                            result: Some(serde_json::to_value(&result)?),
                            error: None,
                        };
                        connection.sender.send(Message::Response(resp))?;
                        continue;
                    }
                    Err(err @ ExtractError::JsonError { .. }) => panic!("{err:?}"),
                    Err(ExtractError::MethodMismatch(req)) => req,
                };
                eprintln!("UNHANDLED {:?}", req);
            }
            Message::Response(resp) => {
                eprintln!("got response: {resp:?}");
            }
            Message::Notification(_not) => {
                // eprintln!("got notification: {_not:?}");
            }
        }
    }
    io_threads.join()?;
    eprintln!("shutting down cargo-language-server");
    Ok(())
}

fn parse<R>(req: Request) -> Result<(RequestId, R::Params), ExtractError<Request>>
where
    R: lsp_types::request::Request,
    R::Params: serde::de::DeserializeOwned,
{
    req.extract(R::METHOD)
}
