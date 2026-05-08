use std::{
    collections::HashMap,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicI64, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use serde_json::{Value, json};

use crate::spellchecker::{
    SpellCheckDiagnostic, SpellCheckDocument, SpellCheckEvent, SpellCheckSuggestion,
};

const HARPER_LS_WINDOWS_EXE: &str = "harper-ls.exe";
const HARPER_LS_UNIX_EXE: &str = "harper-ls";

#[derive(Debug)]
pub(crate) enum SpellCheckerWorkerCommand {
    DidChange(SpellCheckDocument),
    Stop,
}

#[derive(Clone)]
pub(crate) struct SpellCheckerWorkerHandle {
    command_tx: mpsc::Sender<SpellCheckerWorkerCommand>,
    stopped_rx: Arc<Mutex<Option<mpsc::Receiver<()>>>>,
}

impl SpellCheckerWorkerHandle {
    pub(crate) fn send_change(&self, document: SpellCheckDocument) -> bool {
        self.command_tx
            .send(SpellCheckerWorkerCommand::DidChange(document))
            .is_ok()
    }

    pub(crate) fn stop(&self) {
        let _ = self.command_tx.send(SpellCheckerWorkerCommand::Stop);
    }

    pub(crate) fn stop_blocking(&self, timeout: Duration) {
        self.stop();
        let stopped_rx = self
            .stopped_rx
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        let Some(stopped_rx) = stopped_rx else {
            return;
        };
        if stopped_rx.recv_timeout(timeout).is_err() {
            crate::log::trace_debug("spellchecker worker stop wait timed out");
        }
    }
}

pub(crate) fn harper_ls_executable_name_for_target(is_windows: bool) -> &'static str {
    if is_windows {
        HARPER_LS_WINDOWS_EXE
    } else {
        HARPER_LS_UNIX_EXE
    }
}

pub(crate) fn harper_ls_executable_name() -> &'static str {
    harper_ls_executable_name_for_target(cfg!(windows))
}

pub(crate) fn harper_ls_executable_path(app_paths: &crate::path_resolver::AppPaths) -> PathBuf {
    app_paths.bin_dir.join(harper_ls_executable_name())
}

pub(crate) fn language_id_for_path(path: Option<&Path>) -> String {
    match path
        .and_then(|path| path.extension())
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "md" | "markdown" => "markdown".to_string(),
        "txt" | "text" => "text".to_string(),
        "tex" => "tex".to_string(),
        "typ" | "typst" => "typst".to_string(),
        _ => "plaintext".to_string(),
    }
}

pub(crate) fn file_uri_for_path(path: Option<&Path>) -> String {
    match path {
        Some(path) => path_to_file_uri(path),
        None => "file:///papyru2-current.txt".to_string(),
    }
}

fn path_to_file_uri(path: &Path) -> String {
    let raw = normalize_file_uri_path(&path.to_string_lossy());
    if raw.starts_with("//") {
        format!("file:{}", percent_encode_file_uri_path(&raw))
    } else if raw.starts_with('/') {
        format!("file://{}", percent_encode_file_uri_path(&raw))
    } else {
        format!("file:///{}", percent_encode_file_uri_path(&raw))
    }
}

fn normalize_file_uri_path(path: &str) -> String {
    let raw = path.replace('\\', "/");
    if let Some(rest) = raw.strip_prefix("//?/UNC/") {
        format!("//{rest}")
    } else if let Some(rest) = raw.strip_prefix("//./UNC/") {
        format!("//{rest}")
    } else if let Some(rest) = raw.strip_prefix("//?/") {
        rest.to_string()
    } else if let Some(rest) = raw.strip_prefix("//./") {
        rest.to_string()
    } else {
        raw
    }
}

fn percent_encode_file_uri_path(path: &str) -> String {
    let mut encoded = String::with_capacity(path.len());
    for byte in path.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b':' | b'.' | b'-' | b'_' | b'~' => {
                encoded.push(byte as char)
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

pub(crate) fn encode_lsp_message(value: &Value) -> Vec<u8> {
    let body = value.to_string();
    format!("Content-Length: {}\r\n\r\n{}", body.len(), body).into_bytes()
}

pub(crate) fn content_length_from_header(header: &str) -> Result<usize, String> {
    header
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>())
        })
        .ok_or_else(|| "missing Content-Length header".to_string())?
        .map_err(|error| format!("invalid Content-Length header: {error}"))
}

#[derive(Default)]
pub(crate) struct LspMessageBuffer {
    bytes: Vec<u8>,
}

impl LspMessageBuffer {
    pub(crate) fn push(&mut self, chunk: &[u8]) -> Result<Vec<Value>, String> {
        self.bytes.extend_from_slice(chunk);
        let mut messages = Vec::new();

        loop {
            let Some(header_end) = find_header_end(&self.bytes) else {
                break;
            };
            let header = std::str::from_utf8(&self.bytes[..header_end])
                .map_err(|error| format!("invalid LSP header utf8: {error}"))?;
            let content_length = content_length_from_header(header)?;
            let body_start = header_end + 4;
            let body_end = body_start + content_length;
            if self.bytes.len() < body_end {
                break;
            }
            let body = self.bytes[body_start..body_end].to_vec();
            self.bytes.drain(..body_end);
            let message = serde_json::from_slice::<Value>(&body)
                .map_err(|error| format!("invalid LSP JSON body: {error}"))?;
            messages.push(message);
        }

        Ok(messages)
    }
}

fn find_header_end(bytes: &[u8]) -> Option<usize> {
    bytes.windows(4).position(|window| window == b"\r\n\r\n")
}

pub(crate) fn start_harper_worker(
    executable_path: PathBuf,
    event_tx: smol::channel::Sender<SpellCheckEvent>,
    initial_document: SpellCheckDocument,
    generation: u64,
) -> Result<SpellCheckerWorkerHandle, String> {
    if !executable_path.is_file() {
        return Err(format!(
            "harper-ls executable not found at {}",
            executable_path.display()
        ));
    }

    let (command_tx, command_rx) = mpsc::channel();
    let (stopped_tx, stopped_rx) = mpsc::channel();
    thread::Builder::new()
        .name("papyru2-spellchecker".to_string())
        .spawn(move || {
            worker_loop(
                executable_path,
                event_tx,
                initial_document,
                generation,
                command_rx,
                stopped_tx,
            )
        })
        .map_err(|error| format!("failed to spawn spellchecker worker thread: {error}"))?;

    Ok(SpellCheckerWorkerHandle {
        command_tx,
        stopped_rx: Arc::new(Mutex::new(Some(stopped_rx))),
    })
}

fn worker_loop(
    executable_path: PathBuf,
    event_tx: smol::channel::Sender<SpellCheckEvent>,
    initial_document: SpellCheckDocument,
    generation: u64,
    command_rx: mpsc::Receiver<SpellCheckerWorkerCommand>,
    stopped_tx: mpsc::Sender<()>,
) {
    let _done_guard = WorkerDoneGuard::new(stopped_tx);
    crate::log::trace_debug(format!(
        "spellchecker start path={}",
        executable_path.display()
    ));

    let mut child = match Command::new(&executable_path)
        .arg("--stdio")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            send_event(
                &event_tx,
                SpellCheckEvent::Error {
                    generation,
                    message: format!("failed to start harper-ls: {error}"),
                },
            );
            return;
        }
    };

    crate::log::trace_debug(format!("spellchecker started pid={}", child.id()));
    send_event(&event_tx, SpellCheckEvent::Started { generation });

    let Some(stdin) = child.stdin.take() else {
        send_event(
            &event_tx,
            SpellCheckEvent::Error {
                generation,
                message: "failed to capture harper-ls stdin".to_string(),
            },
        );
        wait_or_kill_child(&mut child, Duration::from_millis(0));
        return;
    };
    let Some(stdout) = child.stdout.take() else {
        send_event(
            &event_tx,
            SpellCheckEvent::Error {
                generation,
                message: "failed to capture harper-ls stdout".to_string(),
            },
        );
        wait_or_kill_child(&mut child, Duration::from_millis(0));
        return;
    };

    if let Some(stderr) = child.stderr.take() {
        thread::spawn(move || drain_harper_stderr(stderr));
    }

    let stdin = Arc::new(Mutex::new(stdin));
    let pending_code_actions = Arc::new(Mutex::new(HashMap::<i64, (u64, Option<i32>)>::new()));
    let next_request_id = Arc::new(AtomicI64::new(10));
    let stop_requested = Arc::new(AtomicBool::new(false));
    let (initialize_response_tx, initialize_response_rx) = mpsc::channel();

    let reader_stop_requested = stop_requested.clone();
    let reader_stdin = stdin.clone();
    let reader_pending_code_actions = pending_code_actions.clone();
    let reader_next_request_id = next_request_id.clone();
    let reader_event_tx = event_tx.clone();
    let reader_initialize_response_tx = Some(initialize_response_tx);
    thread::spawn(move || {
        read_stdout_loop(
            stdout,
            reader_stdin,
            reader_pending_code_actions,
            reader_next_request_id,
            reader_event_tx,
            generation,
            reader_stop_requested,
            reader_initialize_response_tx,
        );
    });

    if let Err(error) = send_lsp_value(&stdin, &initialize_request(1)) {
        send_event(
            &event_tx,
            SpellCheckEvent::Error {
                generation,
                message: format!("failed to send initialize: {error}"),
            },
        );
        wait_or_kill_child(&mut child, Duration::from_millis(0));
        return;
    }
    match initialize_response_rx.recv_timeout(Duration::from_secs(5)) {
        Ok(Ok(())) => {}
        Ok(Err(message)) => {
            send_event(
                &event_tx,
                SpellCheckEvent::Error {
                    generation,
                    message: format!("harper-ls initialize failed: {message}"),
                },
            );
            wait_or_kill_child(&mut child, Duration::from_millis(0));
            return;
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
            send_event(
                &event_tx,
                SpellCheckEvent::Error {
                    generation,
                    message: "timed out waiting for harper-ls initialize response".to_string(),
                },
            );
            wait_or_kill_child(&mut child, Duration::from_millis(0));
            return;
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            send_event(
                &event_tx,
                SpellCheckEvent::Error {
                    generation,
                    message: "harper-ls stdout closed before initialize response".to_string(),
                },
            );
            wait_or_kill_child(&mut child, Duration::from_millis(0));
            return;
        }
    }
    if let Err(error) = send_lsp_value(&stdin, &initialized_notification()) {
        send_event(
            &event_tx,
            SpellCheckEvent::Error {
                generation,
                message: format!("failed to send initialized: {error}"),
            },
        );
        wait_or_kill_child(&mut child, Duration::from_millis(0));
        return;
    }
    if let Err(error) = send_lsp_value(&stdin, &did_open_notification(&initial_document)) {
        send_event(
            &event_tx,
            SpellCheckEvent::Error {
                generation,
                message: format!("failed to send didOpen: {error}"),
            },
        );
        wait_or_kill_child(&mut child, Duration::from_millis(0));
        return;
    }
    crate::log::trace_debug(format!(
        "spellchecker did_open uri={} version={} len={}",
        initial_document.uri,
        initial_document.version,
        initial_document.text.len()
    ));

    while let Ok(command) = command_rx.recv() {
        match command {
            SpellCheckerWorkerCommand::DidChange(document) => {
                crate::log::trace_debug(format!(
                    "spellchecker did_change uri={} version={} len={}",
                    document.uri,
                    document.version,
                    document.text.len()
                ));
                if let Err(error) = send_lsp_value(&stdin, &did_change_notification(&document)) {
                    send_event(
                        &event_tx,
                        SpellCheckEvent::Error {
                            generation,
                            message: format!("failed to send didChange: {error}"),
                        },
                    );
                }
            }
            SpellCheckerWorkerCommand::Stop => {
                crate::log::trace_debug("spellchecker stop requested");
                stop_requested.store(true, Ordering::SeqCst);
                let _ = send_lsp_value(&stdin, &shutdown_request(2));
                let _ = send_lsp_value(&stdin, &exit_notification());
                break;
            }
        }
    }

    wait_or_kill_child(&mut child, Duration::from_millis(500));
    crate::log::trace_debug("spellchecker stopped");
    send_event(&event_tx, SpellCheckEvent::Stopped { generation });
}

fn wait_or_kill_child(child: &mut Child, timeout: Duration) {
    let wait_started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_status)) => break,
            Ok(None) if wait_started.elapsed() < timeout => {
                thread::sleep(Duration::from_millis(25));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                break;
            }
            Err(_) => break,
        }
    }
}

struct WorkerDoneGuard {
    stopped_tx: Option<mpsc::Sender<()>>,
}

impl WorkerDoneGuard {
    fn new(stopped_tx: mpsc::Sender<()>) -> Self {
        Self {
            stopped_tx: Some(stopped_tx),
        }
    }
}

impl Drop for WorkerDoneGuard {
    fn drop(&mut self) {
        if let Some(stopped_tx) = self.stopped_tx.take() {
            let _ = stopped_tx.send(());
        }
    }
}

fn read_stdout_loop(
    mut stdout: impl Read,
    stdin: Arc<Mutex<ChildStdin>>,
    pending_code_actions: Arc<Mutex<HashMap<i64, (u64, Option<i32>)>>>,
    next_request_id: Arc<AtomicI64>,
    event_tx: smol::channel::Sender<SpellCheckEvent>,
    generation: u64,
    stop_requested: Arc<AtomicBool>,
    initialize_response_tx: Option<mpsc::Sender<Result<(), String>>>,
) {
    let mut buffer = LspMessageBuffer::default();
    let mut chunk = [0u8; 8192];

    loop {
        if stop_requested.load(Ordering::SeqCst) {
            break;
        }
        let read = match stdout.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => read,
            Err(error) => {
                send_event(
                    &event_tx,
                    SpellCheckEvent::Error {
                        generation,
                        message: format!("failed reading harper-ls stdout: {error}"),
                    },
                );
                break;
            }
        };

        let messages = match buffer.push(&chunk[..read]) {
            Ok(messages) => messages,
            Err(error) => {
                send_event(
                    &event_tx,
                    SpellCheckEvent::Error {
                        generation,
                        message: error,
                    },
                );
                break;
            }
        };

        for message in messages {
            handle_lsp_message(
                message,
                &stdin,
                &pending_code_actions,
                &next_request_id,
                &event_tx,
                generation,
                &initialize_response_tx,
            );
        }
    }
}

fn handle_lsp_message(
    message: Value,
    stdin: &Arc<Mutex<ChildStdin>>,
    pending_code_actions: &Arc<Mutex<HashMap<i64, (u64, Option<i32>)>>>,
    next_request_id: &Arc<AtomicI64>,
    event_tx: &smol::channel::Sender<SpellCheckEvent>,
    generation: u64,
    initialize_response_tx: &Option<mpsc::Sender<Result<(), String>>>,
) {
    if message.get("method").and_then(Value::as_str) == Some("textDocument/publishDiagnostics") {
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        let uri = params
            .get("uri")
            .and_then(Value::as_str)
            .unwrap_or("file:///papyru2-current.txt")
            .to_string();
        let version = params
            .get("version")
            .and_then(Value::as_i64)
            .and_then(|version| i32::try_from(version).ok());
        let diagnostic_version = version.unwrap_or(0);
        let diagnostics = params
            .get("diagnostics")
            .cloned()
            .and_then(|value| serde_json::from_value::<Vec<lsp_types::Diagnostic>>(value).ok())
            .unwrap_or_default()
            .into_iter()
            .enumerate()
            .map(|(index, diagnostic)| SpellCheckDiagnostic {
                id: diagnostic_id(diagnostic_version, index),
                version: diagnostic_version,
                range: diagnostic.range,
                message: diagnostic.message,
                severity: diagnostic.severity,
                suggestions: Vec::new(),
            })
            .collect::<Vec<_>>();

        crate::log::trace_debug(format!(
            "spellchecker diagnostics version={} count={}",
            optional_version_for_log(version),
            diagnostics.len()
        ));
        send_event(
            event_tx,
            SpellCheckEvent::Diagnostics {
                generation,
                version,
                diagnostics: diagnostics.clone(),
            },
        );

        for diagnostic in diagnostics {
            let request_id = next_request_id.fetch_add(1, Ordering::SeqCst);
            pending_code_actions
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .insert(request_id, (diagnostic.id, version));
            let value = code_action_request(request_id, &uri, &diagnostic);
            let _ = send_lsp_value(stdin, &value);
        }
        return;
    }

    if let Some(method) = message.get("method").and_then(Value::as_str) {
        match method {
            "workspace/configuration" => {
                if let Some(id) = message.get("id").and_then(Value::as_i64) {
                    let item_count = message
                        .get("params")
                        .and_then(|params| params.get("items"))
                        .and_then(Value::as_array)
                        .map(Vec::len)
                        .unwrap_or(0);
                    let response = workspace_configuration_response(id, item_count);
                    if let Err(error) = send_lsp_value(stdin, &response) {
                        send_event(
                            event_tx,
                            SpellCheckEvent::Error {
                                generation,
                                message: format!("failed to send workspace/configuration: {error}"),
                            },
                        );
                    } else {
                        crate::log::trace_debug(format!(
                            "spellchecker workspace/configuration response id={} items={}",
                            id, item_count
                        ));
                    }
                }
            }
            "client/registerCapability" => {
                if let Some(id) = message.get("id").and_then(Value::as_i64) {
                    let response = null_response(id);
                    if let Err(error) = send_lsp_value(stdin, &response) {
                        send_event(
                            event_tx,
                            SpellCheckEvent::Error {
                                generation,
                                message: format!(
                                    "failed to send client/registerCapability: {error}"
                                ),
                            },
                        );
                    } else {
                        crate::log::trace_debug(format!(
                            "spellchecker client/registerCapability response id={id}"
                        ));
                    }
                }
            }
            "window/logMessage" | "window/showMessage" => {
                let message_text = message
                    .get("params")
                    .and_then(|params| params.get("message"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                crate::log::trace_debug(format!(
                    "spellchecker server message method={} text={}",
                    method,
                    compact_lsp_log_text(message_text)
                ));
            }
            _ => {}
        }
        return;
    }

    let Some(id) = message.get("id").and_then(Value::as_i64) else {
        return;
    };
    if id == 1 {
        let initialize_result = initialize_response_from_message(&message);
        if let Some(initialize_response_tx) = initialize_response_tx {
            let _ = initialize_response_tx.send(initialize_result.clone());
        }
        match initialize_result {
            Ok(()) => {
                crate::log::trace_debug("spellchecker initialize response received");
            }
            Err(message) => {
                crate::log::trace_debug(format!(
                    "spellchecker initialize response error={}",
                    compact_lsp_log_text(&message)
                ));
            }
        }
        return;
    }
    let Some((diagnostic_id, version)) = pending_code_actions
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .remove(&id)
    else {
        return;
    };
    let suggestions =
        code_actions_to_suggestions(message.get("result").cloned().unwrap_or(Value::Null));
    crate::log::trace_debug(format!(
        "spellchecker code_actions diagnostic_id={} version={} count={}",
        diagnostic_id,
        optional_version_for_log(version),
        suggestions.len()
    ));
    send_event(
        event_tx,
        SpellCheckEvent::CodeActions {
            generation,
            version,
            diagnostic_id,
            suggestions,
        },
    );
}

fn initialize_response_from_message(message: &Value) -> Result<(), String> {
    if let Some(error) = message.get("error") {
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("unknown initialize error");
        Err(message.to_string())
    } else {
        Ok(())
    }
}

fn workspace_configuration_response(id: i64, item_count: usize) -> Value {
    let result = (0..item_count)
        .map(|_| harper_configuration())
        .collect::<Vec<_>>();
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result
    })
}

fn harper_configuration() -> Value {
    json!({
        "harper-ls": {
            "userDictPath": "",
            "workspaceDictPath": "",
            "fileDictPath": "",
            "linters": {
                "SpellCheck": true,
                "SpelledNumbers": false,
                "AnA": true,
                "SentenceCapitalization": true,
                "UnclosedQuotes": true,
                "WrongQuotes": false,
                "LongSentences": true,
                "RepeatedWords": true,
                "Spaces": true,
                "Matcher": true,
                "CorrectNumberSuffix": true
            },
            "codeActions": {
                "ForceStable": false
            },
            "markdown": {
                "IgnoreLinkTitle": false
            },
            "diagnosticSeverity": "hint",
            "isolateEnglish": false,
            "dialect": "American",
            "maxFileLength": 120000,
            "ignoredLintsPath": "",
            "excludePatterns": []
        }
    })
}

fn null_response(id: i64) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": null
    })
}

fn optional_version_for_log(version: Option<i32>) -> String {
    version
        .map(|version| version.to_string())
        .unwrap_or_else(|| "<none>".to_string())
}

fn compact_lsp_log_text(text: &str) -> String {
    text.replace('\\', "\\\\").replace('\n', "\\n")
}

fn diagnostic_id(version: i32, index: usize) -> u64 {
    ((version.max(0) as u64) << 32) | index as u64
}

fn code_actions_to_suggestions(result: Value) -> Vec<SpellCheckSuggestion> {
    let Some(items) = result.as_array() else {
        return Vec::new();
    };

    items
        .iter()
        .filter_map(|item| {
            let title = item.get("title").and_then(Value::as_str)?.to_string();
            let changes = item.get("edit")?.get("changes")?.as_object()?;
            let edits = changes.values().next()?.as_array()?;
            let edit = edits.first()?;
            let range =
                serde_json::from_value::<lsp_types::Range>(edit.get("range")?.clone()).ok()?;
            let new_text = edit.get("newText").and_then(Value::as_str)?.to_string();
            Some(SpellCheckSuggestion {
                label: title,
                replacement_text: new_text,
                range,
            })
        })
        .collect()
}

fn send_lsp_value(stdin: &Arc<Mutex<ChildStdin>>, value: &Value) -> io::Result<()> {
    let mut stdin = stdin
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    stdin.write_all(&encode_lsp_message(value))?;
    stdin.flush()
}

fn send_event(event_tx: &smol::channel::Sender<SpellCheckEvent>, event: SpellCheckEvent) {
    if let Err(error) = event_tx.try_send(event) {
        crate::log::trace_debug(format!("spellchecker event send failed error={error}"));
    }
}

fn drain_harper_stderr(mut stderr: impl Read) {
    let mut buffer = [0u8; 4096];
    loop {
        match stderr.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => {
                let text = String::from_utf8_lossy(&buffer[..read]);
                for line in text.lines().filter(|line| !line.trim().is_empty()) {
                    crate::log::trace_debug(format!("spellchecker stderr {}", line.trim()));
                }
            }
            Err(_) => break,
        }
    }
}

fn initialize_request(id: i64) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "initialize",
        "params": {
            "processId": std::process::id(),
            "rootUri": null,
            "capabilities": {
                "textDocument": {
                    "publishDiagnostics": {
                        "relatedInformation": false
                    }
                }
            },
            "workspaceFolders": null
        }
    })
}

fn initialized_notification() -> Value {
    json!({
        "jsonrpc": "2.0",
        "method": "initialized",
        "params": {}
    })
}

fn did_open_notification(document: &SpellCheckDocument) -> Value {
    json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": {
                "uri": document.uri,
                "languageId": document.language_id,
                "version": document.version,
                "text": document.text
            }
        }
    })
}

fn did_change_notification(document: &SpellCheckDocument) -> Value {
    json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didChange",
        "params": {
            "textDocument": {
                "uri": document.uri,
                "version": document.version
            },
            "contentChanges": [
                {
                    "text": document.text
                }
            ]
        }
    })
}

fn code_action_request(id: i64, uri: &str, diagnostic: &SpellCheckDiagnostic) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "textDocument/codeAction",
        "params": {
            "textDocument": {
                "uri": uri
            },
            "range": diagnostic.range,
            "context": {
                "diagnostics": [
                    {
                        "range": diagnostic.range,
                        "severity": diagnostic.severity,
                        "source": "harper-ls",
                        "message": diagnostic.message
                    }
                ],
                "only": ["quickfix"]
            }
        }
    })
}

fn shutdown_request(id: i64) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "shutdown",
        "params": null
    })
}

fn exit_notification() -> Value {
    json!({
        "jsonrpc": "2.0",
        "method": "exit",
        "params": null
    })
}

#[cfg(test)]
mod tests {
    use super::{
        LspMessageBuffer, content_length_from_header, encode_lsp_message, file_uri_for_path,
        harper_ls_executable_name_for_target, initialize_response_from_message,
        language_id_for_path, null_response, workspace_configuration_response,
    };
    use serde_json::json;
    use std::path::Path;

    #[test]
    fn spchk_test1_executable_name_selects_platform_suffix() {
        assert_eq!(harper_ls_executable_name_for_target(true), "harper-ls.exe");
        assert_eq!(harper_ls_executable_name_for_target(false), "harper-ls");
    }

    #[test]
    fn spchk_test1_language_id_maps_common_text_file_extensions() {
        assert_eq!(language_id_for_path(Some(Path::new("note.md"))), "markdown");
        assert_eq!(language_id_for_path(Some(Path::new("note.txt"))), "text");
        assert_eq!(language_id_for_path(Some(Path::new("note.typ"))), "typst");
        assert_eq!(
            language_id_for_path(Some(Path::new("note.rs"))),
            "plaintext"
        );
    }

    #[test]
    fn spchk_test1_file_uri_encodes_windows_style_path_without_backslashes() {
        let uri = file_uri_for_path(Some(Path::new("D:\\devel\\gpui\\papyru2\\my note.txt")));
        assert_eq!(uri, "file:///D:/devel/gpui/papyru2/my%20note.txt");
    }

    #[test]
    fn spchk_test1_file_uri_normalizes_windows_verbatim_prefix() {
        let uri = file_uri_for_path(Some(Path::new(r"\\?\D:\devel\gpui\papyru2\my note.txt")));
        assert_eq!(uri, "file:///D:/devel/gpui/papyru2/my%20note.txt");
    }

    #[test]
    fn spchk_test1_file_uri_normalizes_windows_unc_verbatim_prefix() {
        let uri = file_uri_for_path(Some(Path::new(r"\\?\UNC\server\share\my note.txt")));
        assert_eq!(uri, "file://server/share/my%20note.txt");
    }

    #[test]
    fn spchk_test2_content_length_header_parser_accepts_valid_header() {
        assert_eq!(
            content_length_from_header("Content-Length: 42\r\nOther: x").unwrap(),
            42
        );
    }

    #[test]
    fn spchk_test2_lsp_message_buffer_parses_multiple_messages() {
        let first = json!({"jsonrpc":"2.0","method":"one"});
        let second = json!({"jsonrpc":"2.0","method":"two"});
        let mut bytes = encode_lsp_message(&first);
        bytes.extend(encode_lsp_message(&second));

        let mut buffer = LspMessageBuffer::default();
        let messages = buffer.push(&bytes).unwrap();

        assert_eq!(messages, vec![first, second]);
    }

    #[test]
    fn spchk_test2_lsp_message_buffer_rejects_malformed_header() {
        let mut buffer = LspMessageBuffer::default();
        let error = buffer
            .push(b"Content-Length: nope\r\n\r\n{}")
            .expect_err("malformed header should fail");
        assert!(error.contains("invalid Content-Length"));
    }

    #[test]
    fn spchk_test2_initialize_response_error_is_reported() {
        let response = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "error": {
                "code": -32603,
                "message": "init failed"
            }
        });

        assert_eq!(
            initialize_response_from_message(&response).expect_err("response should fail"),
            "init failed"
        );
    }

    #[test]
    fn spchk_test2_workspace_configuration_response_contains_harper_ls_key() {
        let response = workspace_configuration_response(7, 1);
        assert_eq!(
            response.get("id").and_then(serde_json::Value::as_i64),
            Some(7)
        );
        assert!(
            response
                .get("result")
                .and_then(serde_json::Value::as_array)
                .and_then(|items| items.first())
                .and_then(|item| item.get("harper-ls"))
                .is_some()
        );
    }

    #[test]
    fn spchk_test2_client_registration_response_is_null_result() {
        let response = null_response(8);
        assert_eq!(
            response.get("id").and_then(serde_json::Value::as_i64),
            Some(8)
        );
        assert!(
            response
                .get("result")
                .is_some_and(serde_json::Value::is_null)
        );
    }
}
