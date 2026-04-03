use std::{path::PathBuf, sync::Arc, thread};

use gpui::Window;
use irpc::{channel::oneshot, rpc::Handler};

#[derive(Debug, Clone)]
pub struct QuicRpcUiCommand {
    pub resolved_path: PathBuf,
    pub content: String,
    pub linenum_1_based: u32,
}

pub fn spawn_quic_rpc_server(
    app_paths: crate::path_resolver::AppPaths,
    file_workflow: crate::file_update_handler::SinglelineCreateFileWorkflow,
    ui_tx: smol::channel::Sender<QuicRpcUiCommand>,
) {
    thread::spawn(move || {
        let bind_addr = crate::quic_rpc_protocol::quic_server_socket_addr();
        crate::app::trace_debug(format!(
            "quic_rpc server thread start addr={bind_addr} host={} port={}",
            crate::quic_rpc_protocol::QUIC_RPC_HOST,
            crate::quic_rpc_protocol::QUIC_RPC_PORT
        ));

        let runtime = match tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(error) => {
                crate::app::trace_debug(format!("quic_rpc runtime init failed error={error}"));
                return;
            }
        };

        runtime.block_on(async move {
            let (endpoint, server_cert_der) = match irpc::util::make_server_endpoint(bind_addr) {
                Ok(value) => value,
                Err(error) => {
                    crate::app::trace_debug(format!(
                        "quic_rpc make_server_endpoint failed addr={bind_addr} error={error}"
                    ));
                    return;
                }
            };
            crate::app::trace_debug(format!(
                "quic_rpc server endpoint ready addr={bind_addr} cert_der_len={}",
                server_cert_der.len()
            ));

            let user_document_dir = app_paths.user_document_dir.clone();
            let handler: Handler<crate::quic_rpc_protocol::PinFileRpcService> = Arc::new({
                let file_workflow = file_workflow.clone();
                let ui_tx = ui_tx.clone();
                move |request, _rx_stream, tx_stream| {
                    let file_workflow = file_workflow.clone();
                    let ui_tx = ui_tx.clone();
                    let user_document_dir = user_document_dir.clone();
                    Box::pin(async move {
                        let response = handle_pin_file_request(
                            request,
                            user_document_dir,
                            file_workflow,
                            ui_tx,
                        )
                        .await;
                        oneshot::Sender::from(tx_stream).send(response).await
                    })
                }
            });

            irpc::rpc::listen(endpoint, handler).await;
            crate::app::trace_debug("quic_rpc listen returned");
        });
    });
}

async fn handle_pin_file_request(
    request: crate::quic_rpc_protocol::PinFileRpcService,
    user_document_dir: PathBuf,
    file_workflow: crate::file_update_handler::SinglelineCreateFileWorkflow,
    ui_tx: smol::channel::Sender<QuicRpcUiCommand>,
) -> crate::quic_rpc_protocol::PinFileRpcResponse {
    let crate::quic_rpc_protocol::PinFileRpcService::PinFile(payload) = request;

    crate::app::trace_debug(format!(
        "quic_rpc request recv file_path='{}' linenum={} platform='{}'",
        crate::app::compact_text(&payload.file_path),
        payload.linenum,
        payload.platform
    ));

    let Some(platform) = crate::quic_rpc_protocol::normalize_platform_tag(&payload.platform) else {
        let response = crate::quic_rpc_protocol::PinFileRpcResponse::invalid_request(
            "platform must be one of windows/linux/macos (case insensitive)",
        );
        crate::app::trace_debug(format!(
            "quic_rpc request reject invalid platform='{}'",
            payload.platform
        ));
        return response;
    };
    crate::app::trace_debug(format!("quic_rpc request platform normalized={platform}"));

    let resolved_path = match crate::quic_rpc_protocol::resolve_request_file_path(
        user_document_dir.as_path(),
        payload.file_path.as_str(),
    ) {
        Ok(path) => path,
        Err(error) => {
            crate::app::trace_debug(format!(
                "quic_rpc request reject path='{}' error={error}",
                crate::app::compact_text(&payload.file_path)
            ));
            return crate::quic_rpc_protocol::PinFileRpcResponse::invalid_request(error);
        }
    };

    let pin_result =
        match file_workflow.try_pin_file_via_rpc(resolved_path.clone(), payload.linenum) {
            Ok(result) => result,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                crate::app::trace_debug(format!(
                    "quic_rpc pin file not found path={} error={error}",
                    resolved_path.display()
                ));
                return crate::quic_rpc_protocol::PinFileRpcResponse::file_not_found(
                    resolved_path.as_path(),
                );
            }
            Err(error) => {
                crate::app::trace_debug(format!(
                    "quic_rpc pin internal failure path={} error={error}",
                    resolved_path.display()
                ));
                return crate::quic_rpc_protocol::PinFileRpcResponse::internal_error(format!(
                    "pin workflow failed: {error}"
                ));
            }
        };

    let command = QuicRpcUiCommand {
        resolved_path: pin_result.path.clone(),
        content: pin_result.content,
        linenum_1_based: pin_result.linenum,
    };
    if let Err(error) = ui_tx.send(command).await {
        crate::app::trace_debug(format!(
            "quic_rpc ui bridge send failed path={} error={error}",
            pin_result.path.display()
        ));
        return crate::quic_rpc_protocol::PinFileRpcResponse::internal_error(
            "ui bridge unavailable",
        );
    }

    crate::app::trace_debug(format!(
        "quic_rpc pin accepted path={} linenum={}",
        pin_result.path.display(),
        pin_result.linenum
    ));
    crate::quic_rpc_protocol::PinFileRpcResponse::ok(pin_result.path.as_path())
}

impl crate::app::Papyru2App {
    pub(crate) fn apply_quic_rpc_pin_command(
        &mut self,
        command: QuicRpcUiCommand,
        window: &mut Window,
        app: &mut gpui::App,
    ) {
        let target_path = command.resolved_path.clone();
        let cursor_line = command.linenum_1_based.saturating_sub(1);
        let requested_line = command.linenum_1_based;
        crate::app::trace_debug(format!(
            "quic_rpc ui apply start path={} linenum={} cursor_line={}",
            target_path.display(),
            requested_line,
            cursor_line
        ));

        self.file_workflow
            .set_edit_from_open_file(target_path.clone());
        let autosave_path = Some(target_path.clone());
        self.singleline.update(app, |singleline, _| {
            singleline.set_current_editing_file_path(autosave_path.clone());
        });
        self.editor.update(app, |editor, _| {
            editor.set_current_editing_file_path(autosave_path.clone());
        });
        self.editor_autosave.on_edit_path_changed(autosave_path);

        if let Some(stem) =
            crate::singleline_input::singleline_stem_from_file_tree_selection(target_path.as_path())
        {
            self.singleline.update(app, |singleline, cx| {
                singleline.apply_text_value_only(stem.clone(), window, cx);
            });
        }

        self.editor.update(app, |editor, cx| {
            editor.open_content_from_rpc(
                target_path.clone(),
                command.content.clone(),
                cursor_line,
                0,
                window,
                cx,
            );
        });

        self.file_tree.update(app, |file_tree, cx| {
            file_tree.clear_selection_for_req_ftr17_case3(cx);
        });

        self.rpc_highlight_active = true;
        self.rpc_highlight_line_1_based = Some(requested_line);
        crate::app::trace_debug(format!(
            "quic_rpc ui apply done path={} highlight_line={} tree_selection_cleared=true",
            target_path.display(),
            requested_line
        ));
    }

    pub(crate) fn clear_rpc_highlight_on_editor_interaction(&mut self) {
        if !self.rpc_highlight_active {
            return;
        }
        let line = self.rpc_highlight_line_1_based.take().unwrap_or(0);
        self.rpc_highlight_active = false;
        crate::app::trace_debug(format!(
            "quic_rpc highlight cleared by editor interaction line={line}"
        ));
    }
}
