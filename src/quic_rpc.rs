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
        crate::log::trace_debug(format!(
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
                crate::log::trace_debug(format!("quic_rpc runtime init failed error={error}"));
                return;
            }
        };

        runtime.block_on(async move {
            let (endpoint, server_cert_der) = match irpc::util::make_server_endpoint(bind_addr) {
                Ok(value) => value,
                Err(error) => {
                    crate::log::trace_debug(format!(
                        "quic_rpc make_server_endpoint failed addr={bind_addr} error={error}"
                    ));
                    return;
                }
            };
            crate::log::trace_debug(format!(
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
            crate::log::trace_debug("quic_rpc listen returned");
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

    crate::log::trace_debug(format!(
        "quic_rpc request recv file_path='{}' linenum={} platform='{}'",
        crate::app::compact_text(&payload.file_path),
        payload.linenum,
        payload.platform
    ));

    let Some(platform) = crate::quic_rpc_protocol::normalize_platform_tag(&payload.platform) else {
        let response = crate::quic_rpc_protocol::PinFileRpcResponse::invalid_request(
            "platform must be one of windows/linux/macos (case insensitive)",
        );
        crate::log::trace_debug(format!(
            "quic_rpc request reject invalid platform='{}'",
            payload.platform
        ));
        return response;
    };
    crate::log::trace_debug(format!("quic_rpc request platform normalized={platform}"));

    let resolved_path = match crate::quic_rpc_protocol::resolve_request_file_path(
        user_document_dir.as_path(),
        payload.file_path.as_str(),
    ) {
        Ok(path) => path,
        Err(error) => {
            crate::log::trace_debug(format!(
                "quic_rpc request reject path='{}' error={error}",
                crate::app::compact_text(&payload.file_path)
            ));
            return crate::quic_rpc_protocol::PinFileRpcResponse::invalid_request(error);
        }
    };

    let pin_result = match file_workflow.try_pin_file_via_rpc(
        user_document_dir,
        resolved_path.clone(),
        payload.linenum,
    ) {
        Ok(result) => result,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            crate::log::trace_debug(format!(
                "quic_rpc pin file not found path={} error={error}",
                resolved_path.display()
            ));
            return crate::quic_rpc_protocol::PinFileRpcResponse::file_not_found(
                resolved_path.as_path(),
            );
        }
        Err(error) => {
            crate::log::trace_debug(format!(
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
        crate::log::trace_debug(format!(
            "quic_rpc ui bridge send failed path={} error={error}",
            pin_result.path.display()
        ));
        return crate::quic_rpc_protocol::PinFileRpcResponse::internal_error(
            "ui bridge unavailable",
        );
    }

    crate::log::trace_debug(format!(
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
        crate::log::trace_debug(format!(
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
        crate::log::trace_debug(format!(
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
        crate::log::trace_debug(format!(
            "quic_rpc highlight cleared by editor interaction line={line}"
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::{QuicRpcUiCommand, handle_pin_file_request};
    use chrono::{Datelike, Duration, Local};
    use std::{
        fs,
        path::{Path, PathBuf},
        time::UNIX_EPOCH,
    };

    fn new_temp_root(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        let stamp = std::time::SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        path.push(format!(
            "papyru2_quic_rpc_{name}_{}_{}",
            std::process::id(),
            stamp
        ));
        fs::create_dir_all(&path).expect("create temp root");
        path
    }

    fn remove_temp_root(path: &Path) {
        let _ = fs::remove_dir_all(path);
    }

    fn dated_directory(root: &Path, year: i32, month: u32, day: u32) -> PathBuf {
        root.join(format!("{year:04}"))
            .join(format!("{month:02}"))
            .join(format!("{day:02}"))
    }

    fn relative_rpc_path(year: i32, month: u32, day: u32, file_name: &str) -> String {
        format!("{year:04}\\{month:02}\\{day:02}\\{file_name}")
    }

    fn new_workflow() -> (
        crate::file_update_handler::FileWorkflowEventDispatcher,
        crate::file_update_handler::SinglelineCreateFileWorkflow,
    ) {
        let dispatcher = crate::file_update_handler::FileWorkflowEventDispatcher::new();
        let workflow = crate::file_update_handler::SinglelineCreateFileWorkflow::with_dispatcher(
            dispatcher.clone(),
        );
        (dispatcher, workflow)
    }

    fn pin_request(file_path: String, linenum: u32) -> crate::quic_rpc_protocol::PinFileRpcService {
        crate::quic_rpc_protocol::PinFileRpcService::PinFile(
            crate::quic_rpc_protocol::PinFileRpcRequest {
                file_path,
                linenum,
                platform: crate::quic_rpc_protocol::current_platform_tag().to_string(),
            },
        )
    }

    fn run_pin_request(
        root: PathBuf,
        workflow: crate::file_update_handler::SinglelineCreateFileWorkflow,
        request: crate::quic_rpc_protocol::PinFileRpcService,
    ) -> (
        crate::quic_rpc_protocol::PinFileRpcResponse,
        QuicRpcUiCommand,
    ) {
        let (ui_tx, ui_rx) = smol::channel::unbounded::<QuicRpcUiCommand>();
        let response = smol::block_on(handle_pin_file_request(request, root, workflow, ui_tx));
        let command = smol::block_on(ui_rx.recv()).expect("receive ui command");
        (response, command)
    }

    fn debug_log_path() -> PathBuf {
        crate::log::trace_debug_log_file_path()
    }

    fn debug_log_len() -> usize {
        fs::metadata(debug_log_path())
            .ok()
            .map(|metadata| metadata.len() as usize)
            .unwrap_or(0)
    }

    fn debug_log_tail(start_offset: usize) -> String {
        let bytes = fs::read(debug_log_path()).unwrap_or_default();
        if start_offset >= bytes.len() {
            return String::new();
        }
        String::from_utf8_lossy(&bytes[start_offset..]).to_string()
    }

    #[test]
    fn qsrv_test12_req_qsrv4_follow_rpc_pin_auto_moves_file_into_today_daily_dir_without_ui_click()
    {
        let root = new_temp_root("qsrv_test12");
        let now = Local::now();
        let yesterday = now - Duration::days(1);
        let file_name = "qsrv_test12_target.txt";

        let yesterday_dir = dated_directory(
            root.as_path(),
            yesterday.year(),
            yesterday.month(),
            yesterday.day(),
        );
        fs::create_dir_all(&yesterday_dir).expect("create yesterday dir");
        let yesterday_file = yesterday_dir.join(file_name);
        fs::write(&yesterday_file, "line1\nline2").expect("seed yesterday file");

        let request_path = relative_rpc_path(
            yesterday.year(),
            yesterday.month(),
            yesterday.day(),
            file_name,
        );
        let request = pin_request(request_path, 2);

        let (dispatcher, workflow) = new_workflow();
        let (response, command) = run_pin_request(root.clone(), workflow, request);

        let today_dir = dated_directory(root.as_path(), now.year(), now.month(), now.day());
        assert!(response.ok, "rpc response must be ok");
        assert!(
            command.resolved_path.starts_with(today_dir.as_path()),
            "resolved path must relocate into today's daily directory"
        );
        assert!(command.resolved_path.exists(), "relocated file must exist");
        assert!(!yesterday_file.exists(), "source file should be moved");

        let response_path = PathBuf::from(
            response
                .resolved_path
                .expect("ok response must include resolved_path"),
        );
        assert_eq!(response_path, command.resolved_path);

        dispatcher.shutdown();
        remove_temp_root(root.as_path());
    }

    #[test]
    fn qsrv_test13_req_qsrv4_follow_rpc_pin_keeps_noop_when_file_already_in_today_daily_dir() {
        let root = new_temp_root("qsrv_test13");
        let now = Local::now();
        let file_name = "qsrv_test13_target.txt";

        let today_dir = dated_directory(root.as_path(), now.year(), now.month(), now.day());
        fs::create_dir_all(&today_dir).expect("create today dir");
        let today_file = today_dir.join(file_name);
        fs::write(&today_file, "line1\nline2\nline3").expect("seed today file");

        let request_path = relative_rpc_path(now.year(), now.month(), now.day(), file_name);
        let request = pin_request(request_path, 3);

        let (dispatcher, workflow) = new_workflow();
        let (response, command) = run_pin_request(root.clone(), workflow, request);

        assert!(response.ok, "rpc response must be ok");
        assert_eq!(command.resolved_path, today_file);
        assert!(today_file.exists(), "today path should remain valid");

        let response_path = PathBuf::from(
            response
                .resolved_path
                .expect("ok response must include resolved_path"),
        );
        assert_eq!(response_path, today_file);

        dispatcher.shutdown();
        remove_temp_root(root.as_path());
    }

    #[test]
    fn qsrv_test14_req_qsrv4_follow_trace_order_shows_daily_move_without_row_click_prerequisite() {
        let root = new_temp_root("qsrv_test14");
        let now = Local::now();
        let yesterday = now - Duration::days(1);
        let unique_name = format!(
            "qsrv_test14_marker_{}_{}.txt",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or(0)
        );

        let yesterday_dir = dated_directory(
            root.as_path(),
            yesterday.year(),
            yesterday.month(),
            yesterday.day(),
        );
        fs::create_dir_all(&yesterday_dir).expect("create yesterday dir");
        let yesterday_file = yesterday_dir.join(unique_name.as_str());
        fs::write(&yesterday_file, "line1\nline2").expect("seed yesterday file");

        let request_path = relative_rpc_path(
            yesterday.year(),
            yesterday.month(),
            yesterday.day(),
            unique_name.as_str(),
        );
        let request = pin_request(request_path, 1);

        let start_offset = debug_log_len();
        let (dispatcher, workflow) = new_workflow();
        let (response, command) = run_pin_request(root.clone(), workflow, request);

        assert!(response.ok, "rpc response must be ok");
        let today_dir = dated_directory(root.as_path(), now.year(), now.month(), now.day());
        assert!(command.resolved_path.starts_with(today_dir.as_path()));

        let tail = debug_log_tail(start_offset);
        let lines: Vec<&str> = tail.lines().collect();
        let req_idx = lines
            .iter()
            .position(|line| {
                line.contains("quic_rpc request recv") && line.contains(unique_name.as_str())
            })
            .expect("request recv trace for marker must exist");
        let move_start_idx = lines
            .iter()
            .position(|line| {
                line.contains("req-newf35 daily-move start") && line.contains(unique_name.as_str())
            })
            .expect("daily-move start trace for marker must exist");
        let move_success_idx = lines
            .iter()
            .position(|line| {
                line.contains("req-newf35 daily-move success")
                    && line.contains(unique_name.as_str())
            })
            .expect("daily-move success trace for marker must exist");

        assert!(
            req_idx < move_start_idx,
            "request recv trace must come before daily-move start"
        );
        assert!(
            move_start_idx < move_success_idx,
            "daily-move start trace must come before daily-move success"
        );

        let marker_lines: Vec<&str> = lines
            .iter()
            .copied()
            .filter(|line| line.contains(unique_name.as_str()))
            .collect();
        assert!(
            marker_lines.iter().all(|line| !line.contains("row_click")),
            "row_click must not be a prerequisite trace for the same pinned target"
        );

        dispatcher.shutdown();
        remove_temp_root(root.as_path());
    }
}
