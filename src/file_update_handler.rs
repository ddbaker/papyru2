use std::{
    collections::VecDeque,
    fs, io,
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex, mpsc},
    thread,
    time::{Duration, Instant},
};

use chrono::{DateTime, Local};
use filetime::{FileTime, set_file_mtime};
use gpui::{Context, Window};
use serde::{Deserialize, Serialize};

pub const MAX_FILE_STEM_CHARS: usize = 64;
pub const CREATE_EVENT_MIN_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SinglelineFileState {
    Neutral,
    New,
    Edit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowSnapshot {
    pub state: SinglelineFileState,
    pub current_edit_path: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct CreateFileRequest {
    pub user_document_dir: PathBuf,
    pub singleline_value: String,
    pub now: DateTime<Local>,
}

#[derive(Debug, Clone)]
pub struct RenameFileRequest {
    pub user_document_dir: PathBuf,
    pub current_path: PathBuf,
    pub singleline_value: String,
    pub now: DateTime<Local>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EditorAutoSavePayload {
    pub user_document_dir: PathBuf,
    pub current_path: PathBuf,
    pub editor_text: String,
}

#[derive(Debug, Clone)]
pub struct AutoSaveFileRequest {
    pub payload: EditorAutoSavePayload,
}

#[derive(Debug, Clone)]
pub struct RpcPinFileRequest {
    pub full_path: PathBuf,
    pub linenum: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RpcPinFileResult {
    pub path: PathBuf,
    pub content: String,
    pub linenum: u32,
}

pub const EDITOR_AUTOSAVE_IDLE_DURATION: Duration = Duration::from_secs(6);
pub const EDITOR_AUTOSAVE_TICK_DURATION: Duration = Duration::from_millis(200);

#[derive(Debug, Default)]
struct EditorAutoSaveState {
    pinned_time: Option<Instant>,
    pending_payload: Option<EditorAutoSavePayload>,
    last_delta_trace_secs: Option<u64>,
}

#[derive(Clone, Debug)]
pub struct EditorAutoSaveCoordinator {
    inner: Arc<Mutex<EditorAutoSaveState>>,
}

impl EditorAutoSaveCoordinator {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(EditorAutoSaveState::default())),
        }
    }

    pub fn mark_user_edit(&self, payload: EditorAutoSavePayload, now: Instant) {
        let mut state = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.pinned_time.is_none() {
            state.pinned_time = Some(now);
            state.last_delta_trace_secs = None;
        }
        state.pending_payload = Some(payload);
    }

    pub fn on_edit_path_changed(&self, path: Option<PathBuf>) {
        let mut state = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match path {
            Some(path) => {
                if let Some(payload) = state.pending_payload.as_ref()
                    && payload.current_path != path
                {
                    crate::app::trace_debug(format!(
                        "autosave drop pending on path switch old={} new={}",
                        payload.current_path.display(),
                        path.display()
                    ));
                    state.pinned_time = None;
                    state.pending_payload = None;
                    state.last_delta_trace_secs = None;
                }
            }
            None => {
                state.pinned_time = None;
                state.pending_payload = None;
                state.last_delta_trace_secs = None;
            }
        }
    }

    pub fn reset_cycle(&self) {
        let mut state = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.pinned_time = None;
        state.pending_payload = None;
        state.last_delta_trace_secs = None;
    }

    pub fn pop_due_payload(
        &self,
        now: Instant,
        idle_duration: Duration,
    ) -> Option<EditorAutoSavePayload> {
        let mut state = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let pinned_time = state.pinned_time?;
        let delta = now.duration_since(pinned_time);
        if state.pending_payload.is_some() {
            let delta_secs = delta.as_secs();
            if state.last_delta_trace_secs != Some(delta_secs) {
                state.last_delta_trace_secs = Some(delta_secs);
                crate::app::trace_debug(format!(
                    "autosave step-3 delta_ms={} threshold_ms={} armed=true",
                    delta.as_millis(),
                    idle_duration.as_millis()
                ));
            }
        }

        if delta < idle_duration {
            return None;
        }

        let payload = state.pending_payload.take();
        state.pinned_time = None;
        state.last_delta_trace_secs = None;
        payload
    }

    #[cfg(test)]
    pub fn has_pending_payload(&self) -> bool {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .pending_payload
            .is_some()
    }
}

pub fn spawn_editor_autosave_worker(
    autosave_coordinator: EditorAutoSaveCoordinator,
    autosave_workflow: SinglelineCreateFileWorkflow,
) {
    thread::spawn(move || {
        crate::app::trace_debug("autosave timer thread started");
        loop {
            thread::sleep(EDITOR_AUTOSAVE_TICK_DURATION);
            let Some(payload) =
                autosave_coordinator.pop_due_payload(Instant::now(), EDITOR_AUTOSAVE_IDLE_DURATION)
            else {
                continue;
            };

            let target = payload.current_path.display().to_string();
            let editor_len = payload.editor_text.len();
            crate::app::trace_debug(format!(
                "autosave step-5 raise event path={} text_len={}",
                target, editor_len
            ));

            match autosave_workflow.try_autosave_in_edit(payload) {
                Ok(true) => {
                    crate::app::trace_debug(format!(
                        "autosave success path={} text_len={} (step-6 reset)",
                        target, editor_len
                    ));
                }
                Ok(false) => {
                    crate::app::trace_debug(format!(
                        "autosave critical skipped (state/path invalid) path={}",
                        target
                    ));
                    debug_assert!(
                        false,
                        "autosave invariant violation: event raised while state/path invalid"
                    );
                }
                Err(error) => {
                    crate::app::trace_debug(format!(
                        "autosave failure path={} error={error} (step-6 reset)",
                        target
                    ));
                }
            }
        }
    });
}

#[derive(Debug, Clone)]
pub enum FileWorkflowEvent {
    Create(CreateFileRequest),
    Rename(RenameFileRequest),
    AutoSave(AutoSaveFileRequest),
    RpcPin(RpcPinFileRequest),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileWorkflowEventResult {
    Created {
        path: PathBuf,
    },
    Renamed {
        path: PathBuf,
    },
    AutoSaved {
        path: PathBuf,
    },
    RpcPinned {
        path: PathBuf,
        content: String,
        linenum: u32,
    },
}

#[derive(Debug)]
struct EventEnvelope {
    event: FileWorkflowEvent,
    response_tx: mpsc::Sender<io::Result<FileWorkflowEventResult>>,
}

#[derive(Debug, Default)]
struct QueueState {
    queue: VecDeque<EventEnvelope>,
    shutdown: bool,
}

#[derive(Clone, Debug)]
pub struct FileWorkflowEventDispatcher {
    shared: Arc<(Mutex<QueueState>, Condvar)>,
}

impl FileWorkflowEventDispatcher {
    pub fn new() -> Self {
        let shared = Arc::new((Mutex::new(QueueState::default()), Condvar::new()));
        let worker_shared = shared.clone();

        thread::spawn(move || worker_loop(worker_shared));

        Self { shared }
    }

    pub fn dispatch_blocking(
        &self,
        event: FileWorkflowEvent,
    ) -> io::Result<FileWorkflowEventResult> {
        let (response_tx, response_rx) = mpsc::channel::<io::Result<FileWorkflowEventResult>>();
        {
            let (lock, wakeup) = &*self.shared;
            let mut state = lock.lock().map_err(|_| {
                io::Error::other("file_update_handler event queue lock poisoned on enqueue")
            })?;
            state.queue.push_back(EventEnvelope { event, response_tx });
            wakeup.notify_one();
        }

        response_rx.recv().map_err(|_| {
            io::Error::new(
                io::ErrorKind::BrokenPipe,
                "file_update_handler worker terminated before sending response",
            )
        })?
    }

    #[cfg(test)]
    pub fn shutdown(&self) {
        let (lock, wakeup) = &*self.shared;
        if let Ok(mut state) = lock.lock() {
            state.shutdown = true;
            wakeup.notify_all();
        }
    }
}

fn worker_loop(shared: Arc<(Mutex<QueueState>, Condvar)>) {
    loop {
        let envelope = {
            let (lock, wakeup) = &*shared;
            let mut state = match lock.lock() {
                Ok(state) => state,
                Err(poisoned) => poisoned.into_inner(),
            };

            while state.queue.is_empty() && !state.shutdown {
                state = match wakeup.wait(state) {
                    Ok(state) => state,
                    Err(poisoned) => poisoned.into_inner(),
                };
            }

            if state.shutdown && state.queue.is_empty() {
                break;
            }

            state.queue.pop_front()
        };

        if let Some(envelope) = envelope {
            let result = process_event(envelope.event);
            let _ = envelope.response_tx.send(result);
        }
    }
}

fn process_event(event: FileWorkflowEvent) -> io::Result<FileWorkflowEventResult> {
    match event {
        FileWorkflowEvent::Create(request) => {
            let path = create_new_text_file(&request)?;
            Ok(FileWorkflowEventResult::Created { path })
        }
        FileWorkflowEvent::Rename(request) => {
            let path = rename_text_file(&request)?;
            Ok(FileWorkflowEventResult::Renamed { path })
        }
        FileWorkflowEvent::AutoSave(request) => {
            let path = save_editor_text_payload_atomic(&request.payload)?;
            Ok(FileWorkflowEventResult::AutoSaved { path })
        }
        FileWorkflowEvent::RpcPin(request) => {
            let result = pin_existing_text_file(&request)?;
            Ok(FileWorkflowEventResult::RpcPinned {
                path: result.path,
                content: result.content,
                linenum: result.linenum,
            })
        }
    }
}

fn pin_existing_text_file(request: &RpcPinFileRequest) -> io::Result<RpcPinFileResult> {
    if !request.full_path.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "rpc pin target file does not exist: {}",
                request.full_path.display()
            ),
        ));
    }

    let content = fs::read_to_string(request.full_path.as_path())?;
    let total_lines = crate::quic_rpc_protocol::content_line_count(&content);
    let clamped_linenum =
        crate::quic_rpc_protocol::clamp_linenum_1_based(request.linenum, total_lines);

    touch_file_modified_now(request.full_path.as_path())?;

    Ok(RpcPinFileResult {
        path: request.full_path.clone(),
        content,
        linenum: clamped_linenum,
    })
}

fn touch_file_modified_now(path: &Path) -> io::Result<()> {
    let now = FileTime::from_system_time(std::time::SystemTime::now());
    set_file_mtime(path, now)
        .map_err(|error| io::Error::other(format!("failed to update modified time: {error}")))
}

#[derive(Debug)]
struct WorkflowStateInner {
    state: SinglelineFileState,
    current_edit_path: Option<PathBuf>,
    last_create_event_raised_at: Option<Instant>,
}

fn rollback_new_to_neutral(state: &mut WorkflowStateInner) {
    state.state = SinglelineFileState::Neutral;
    state.current_edit_path = None;
}

#[derive(Clone, Debug)]
pub struct SinglelineCreateFileWorkflow {
    inner: Arc<Mutex<WorkflowStateInner>>,
    dispatcher: FileWorkflowEventDispatcher,
}

impl SinglelineCreateFileWorkflow {
    pub fn new() -> Self {
        Self::with_dispatcher(FileWorkflowEventDispatcher::new())
    }

    pub fn with_dispatcher(dispatcher: FileWorkflowEventDispatcher) -> Self {
        Self {
            inner: Arc::new(Mutex::new(WorkflowStateInner {
                state: SinglelineFileState::Neutral,
                current_edit_path: None,
                last_create_event_raised_at: None,
            })),
            dispatcher,
        }
    }

    pub fn snapshot(&self) -> WorkflowSnapshot {
        let state = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        WorkflowSnapshot {
            state: state.state,
            current_edit_path: state.current_edit_path.clone(),
        }
    }

    pub fn state(&self) -> SinglelineFileState {
        self.snapshot().state
    }

    pub fn current_edit_path(&self) -> Option<PathBuf> {
        self.snapshot().current_edit_path
    }

    pub fn reset_startup_to_neutral(&self) {
        let mut state = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.state = SinglelineFileState::Neutral;
        state.current_edit_path = None;
    }

    pub fn set_edit_from_open_file(&self, path: PathBuf) {
        let mut state = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.state = SinglelineFileState::Edit;
        state.current_edit_path = Some(path);
    }

    pub fn transition_edit_to_neutral(&self) -> bool {
        let mut state = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.state != SinglelineFileState::Edit {
            return false;
        }

        state.state = SinglelineFileState::Neutral;
        state.current_edit_path = None;
        true
    }

    pub fn try_create_from_neutral(
        &self,
        singleline_value: &str,
        user_document_dir: &Path,
        now_instant: Instant,
        now_local: DateTime<Local>,
    ) -> io::Result<Option<PathBuf>> {
        // Keep workflow-state lock across dispatch to serialize workflow transitions
        // with file-update side effects. This lock does not participate in queue-state
        // lock ordering and therefore does not introduce lock cycles.
        let mut state = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.state != SinglelineFileState::Neutral {
            return Ok(None);
        }

        if let Some(last) = state.last_create_event_raised_at {
            let ready = now_instant
                .checked_duration_since(last)
                .map(|elapsed| elapsed > CREATE_EVENT_MIN_INTERVAL)
                .unwrap_or(false);
            if !ready {
                return Ok(None);
            }
        }

        state.state = SinglelineFileState::New;
        state.last_create_event_raised_at = Some(now_instant);

        let result =
            match self
                .dispatcher
                .dispatch_blocking(FileWorkflowEvent::Create(CreateFileRequest {
                    user_document_dir: user_document_dir.to_path_buf(),
                    singleline_value: singleline_value.to_string(),
                    now: now_local,
                })) {
                Ok(result) => result,
                Err(error) => {
                    rollback_new_to_neutral(&mut state);
                    return Err(error);
                }
            };

        match result {
            FileWorkflowEventResult::Created { path } => {
                state.state = SinglelineFileState::Edit;
                state.current_edit_path = Some(path.clone());
                Ok(Some(path))
            }
            FileWorkflowEventResult::Renamed { .. }
            | FileWorkflowEventResult::AutoSaved { .. }
            | FileWorkflowEventResult::RpcPinned { .. } => {
                rollback_new_to_neutral(&mut state);
                debug_assert!(
                    false,
                    "create invariant violation: create event must only return Created"
                );
                Ok(None)
            }
        }
    }

    pub fn try_rename_in_edit(
        &self,
        singleline_value: &str,
        user_document_dir: &Path,
        now_local: DateTime<Local>,
    ) -> io::Result<Option<PathBuf>> {
        let mut state = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.state != SinglelineFileState::Edit {
            return Ok(None);
        }
        let Some(current_path) = state.current_edit_path.clone() else {
            return Ok(None);
        };

        let result = self
            .dispatcher
            .dispatch_blocking(FileWorkflowEvent::Rename(RenameFileRequest {
                user_document_dir: user_document_dir.to_path_buf(),
                current_path,
                singleline_value: singleline_value.to_string(),
                now: now_local,
            }))?;

        match result {
            FileWorkflowEventResult::Renamed { path } => {
                state.current_edit_path = Some(path.clone());
                Ok(Some(path))
            }
            FileWorkflowEventResult::Created { .. }
            | FileWorkflowEventResult::AutoSaved { .. }
            | FileWorkflowEventResult::RpcPinned { .. } => {
                debug_assert!(
                    false,
                    "rename invariant violation: rename event must only return Renamed"
                );
                Ok(None)
            }
        }
    }

    pub fn try_autosave_in_edit(&self, payload: EditorAutoSavePayload) -> io::Result<bool> {
        let mut state = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.state != SinglelineFileState::Edit {
            return Ok(false);
        }
        let Some(current_path) = state.current_edit_path.as_ref() else {
            return Ok(false);
        };
        if *current_path != payload.current_path {
            return Ok(false);
        }

        let result = self
            .dispatcher
            .dispatch_blocking(FileWorkflowEvent::AutoSave(AutoSaveFileRequest {
                payload: payload.clone(),
            }))?;

        match result {
            FileWorkflowEventResult::AutoSaved { path } => {
                if state.current_edit_path.as_ref() != Some(&path) {
                    let previous = state
                        .current_edit_path
                        .as_ref()
                        .map(|old| old.display().to_string())
                        .unwrap_or_else(|| "<none>".to_string());
                    crate::app::trace_debug(format!(
                        "req-newf35 autosave path updated old={} new={}",
                        previous,
                        path.display()
                    ));
                }
                state.current_edit_path = Some(path);
                Ok(true)
            }
            FileWorkflowEventResult::Created { .. }
            | FileWorkflowEventResult::Renamed { .. }
            | FileWorkflowEventResult::RpcPinned { .. } => {
                debug_assert!(
                    false,
                    "autosave invariant violation: autosave event must only return AutoSaved"
                );
                Ok(false)
            }
        }
    }

    pub fn try_pin_file_via_rpc(
        &self,
        full_path: PathBuf,
        linenum: u32,
    ) -> io::Result<RpcPinFileResult> {
        let result = self
            .dispatcher
            .dispatch_blocking(FileWorkflowEvent::RpcPin(RpcPinFileRequest {
                full_path,
                linenum,
            }))?;

        match result {
            FileWorkflowEventResult::RpcPinned {
                path,
                content,
                linenum,
            } => Ok(RpcPinFileResult {
                path,
                content,
                linenum,
            }),
            FileWorkflowEventResult::Created { .. }
            | FileWorkflowEventResult::Renamed { .. }
            | FileWorkflowEventResult::AutoSaved { .. } => {
                debug_assert!(
                    false,
                    "rpc-pin invariant violation: rpc pin event must only return RpcPinned"
                );
                Err(io::Error::other(
                    "rpc-pin invariant violation: unexpected event result variant",
                ))
            }
        }
    }

    pub fn flush_editor_content_in_edit(
        &self,
        editor_text: &str,
        user_document_dir: &Path,
    ) -> io::Result<bool> {
        let snapshot = self.snapshot();
        if snapshot.state != SinglelineFileState::Edit {
            return Ok(false);
        }
        let Some(current_path) = snapshot.current_edit_path else {
            return Ok(false);
        };

        self.try_autosave_in_edit(EditorAutoSavePayload {
            user_document_dir: user_document_dir.to_path_buf(),
            current_path,
            editor_text: editor_text.to_string(),
        })
    }
}

pub fn invalid_filename_char(ch: char) -> bool {
    matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') || ch.is_control()
}

pub fn sanitize_filename_stem(raw: &str) -> String {
    let replaced: String = raw
        .chars()
        .map(|ch| if invalid_filename_char(ch) { '_' } else { ch })
        .collect();
    replaced.chars().take(MAX_FILE_STEM_CHARS).collect()
}

pub fn notitle_stem(now: DateTime<Local>) -> String {
    format!("notitle-{}", now.format("%Y%m%d%H%M%S%3f"))
}

pub fn stem_from_singleline_value(value: &str, now: DateTime<Local>) -> String {
    if value.is_empty() {
        return notitle_stem(now);
    }

    let sanitized = sanitize_filename_stem(value);
    if sanitized.is_empty() {
        return notitle_stem(now);
    }

    sanitized
}

pub fn daily_directory(user_document_dir: &Path, now: DateTime<Local>) -> PathBuf {
    user_document_dir.join(now.format("%Y/%m/%d").to_string())
}

pub fn ensure_daily_directory(
    user_document_dir: &Path,
    now: DateTime<Local>,
) -> io::Result<PathBuf> {
    let dir = daily_directory(user_document_dir, now);
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn comparable_path_for_daily_directory(path: &Path) -> PathBuf {
    let path_str = path.to_string_lossy();
    if let Some(stripped) = path_str.strip_prefix(r"\\?\") {
        return PathBuf::from(stripped);
    }
    path.to_path_buf()
}

fn is_path_under_daily_directory(current_path: &Path, daily_dir: &Path) -> bool {
    current_path
        .parent()
        .map(|parent| {
            comparable_path_for_daily_directory(parent)
                == comparable_path_for_daily_directory(daily_dir)
        })
        .unwrap_or(false)
}

fn relocated_daily_candidate_path(
    daily_dir: &Path,
    original_file_name: &str,
    suffix: usize,
) -> PathBuf {
    if suffix == 1 {
        return daily_dir.join(original_file_name);
    }

    let original = Path::new(original_file_name);
    let stem = original
        .file_stem()
        .map(|value| value.to_string_lossy().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "file".to_string());
    let extension = original
        .extension()
        .map(|value| value.to_string_lossy().to_string())
        .filter(|value| !value.is_empty());

    let file_name = match extension {
        Some(extension) => format!("{stem}_{suffix}.{extension}"),
        None => format!("{stem}_{suffix}"),
    };
    daily_dir.join(file_name)
}

fn move_existing_file_to_daily_directory(
    current_path: &Path,
    user_document_dir: &Path,
    now: DateTime<Local>,
) -> io::Result<PathBuf> {
    if !current_path.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "current editing file does not exist",
        ));
    }

    let daily_dir = ensure_daily_directory(user_document_dir, now)?;
    if is_path_under_daily_directory(current_path, daily_dir.as_path()) {
        crate::app::trace_debug(format!(
            "req-newf35 daily-move noop path={} daily_dir={}",
            current_path.display(),
            daily_dir.display()
        ));
        return Ok(current_path.to_path_buf());
    }

    let original_file_name = current_path
        .file_name()
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "current editing file path has no file name",
            )
        })?
        .to_string_lossy()
        .to_string();

    crate::app::trace_debug(format!(
        "req-newf35 daily-move start from={} to_dir={}",
        current_path.display(),
        daily_dir.display()
    ));

    let mut suffix = 1usize;
    loop {
        let target =
            relocated_daily_candidate_path(daily_dir.as_path(), &original_file_name, suffix);
        if target.exists() {
            suffix += 1;
            continue;
        }

        match fs::rename(current_path, &target) {
            Ok(_) => {
                crate::app::trace_debug(format!(
                    "req-newf35 daily-move success from={} to={}",
                    current_path.display(),
                    target.display()
                ));
                return Ok(target);
            }
            Err(error) if is_retryable_name_conflict_error(&error) || target.exists() => {
                suffix += 1;
                continue;
            }
            Err(error) => return Err(error),
        }
    }
}

pub fn forced_singleline_stem_after_create(
    singleline_value: &str,
    created_path: &Path,
    now: DateTime<Local>,
) -> Option<String> {
    forced_singleline_stem_after_resolution(singleline_value, created_path, now)
}

pub fn forced_singleline_stem_after_rename(
    singleline_value: &str,
    renamed_path: &Path,
    now: DateTime<Local>,
) -> Option<String> {
    forced_singleline_stem_after_resolution(singleline_value, renamed_path, now)
}

fn forced_singleline_stem_after_resolution(
    _singleline_value: &str,
    _resolved_path: &Path,
    _now: DateTime<Local>,
) -> Option<String> {
    // req-newf32: disabled forced singleline buffer rewrite even when
    // create/rename resolution applies collision suffix or sanitization.
    None
}

fn is_retryable_name_conflict_error(error: &io::Error) -> bool {
    if error.kind() == io::ErrorKind::AlreadyExists {
        return true;
    }

    #[cfg(target_os = "windows")]
    {
        return error.raw_os_error() == Some(183);
    }

    #[cfg(not(target_os = "windows"))]
    {
        false
    }
}

fn txt_candidate_path(dir: &Path, stem: &str, suffix: usize) -> PathBuf {
    let file_name = if suffix == 1 {
        format!("{stem}.txt")
    } else {
        format!("{stem}_{suffix}.txt")
    };
    dir.join(file_name)
}

pub fn create_new_text_file(request: &CreateFileRequest) -> io::Result<PathBuf> {
    let dir = ensure_daily_directory(request.user_document_dir.as_path(), request.now)?;

    let stem = stem_from_singleline_value(&request.singleline_value, request.now);
    let mut suffix = 1usize;
    loop {
        let path = txt_candidate_path(dir.as_path(), &stem, suffix);
        match fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
        {
            Ok(_) => return Ok(path),
            Err(error) if is_retryable_name_conflict_error(&error) => {
                suffix += 1;
                continue;
            }
            Err(error) => return Err(error),
        }
    }
}

pub fn rename_text_file(request: &RenameFileRequest) -> io::Result<PathBuf> {
    if !request.current_path.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "current editing file does not exist",
        ));
    }

    let relocated_path = move_existing_file_to_daily_directory(
        request.current_path.as_path(),
        request.user_document_dir.as_path(),
        request.now,
    )?;
    let parent = relocated_path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "current editing file path has no parent directory",
        )
    })?;

    let stem = stem_from_singleline_value(&request.singleline_value, request.now);
    let mut suffix = 1usize;
    loop {
        let target = txt_candidate_path(parent, &stem, suffix);
        if target == relocated_path {
            return Ok(target);
        }
        if target.exists() {
            suffix += 1;
            continue;
        }

        match fs::rename(&relocated_path, &target) {
            Ok(_) => return Ok(target),
            Err(error) if is_retryable_name_conflict_error(&error) || target.exists() => {
                suffix += 1;
                continue;
            }
            Err(error) => return Err(error),
        }
    }
}

fn save_editor_text_payload_atomic(payload: &EditorAutoSavePayload) -> io::Result<PathBuf> {
    // Keep a serde round-trip in event handling to satisfy req-aus4 payload serialization contract,
    // while persisting raw editor text as the file content.
    let serialized = serde_json::to_vec(payload)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
    let decoded: EditorAutoSavePayload = serde_json::from_slice(&serialized)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;

    let relocated_path = move_existing_file_to_daily_directory(
        decoded.current_path.as_path(),
        decoded.user_document_dir.as_path(),
        Local::now(),
    )?;
    write_editor_text_atomic(relocated_path.as_path(), decoded.editor_text.as_bytes())?;
    Ok(relocated_path)
}

fn write_editor_text_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write_editor_text_atomic_with_replace(path, bytes, replace_editor_target_with_temp)
}

fn write_editor_text_atomic_with_replace<F>(
    path: &Path,
    bytes: &[u8],
    replace_fn: F,
) -> io::Result<()>
where
    F: Fn(&Path, &Path) -> io::Result<()>,
{
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "editor autosave path has no parent directory",
        )
    })?;
    fs::create_dir_all(parent)?;

    let temp_path = editor_temp_path_for_atomic_write(path)?;
    if temp_path.is_file() {
        fs::remove_file(&temp_path)?;
    }
    let mut temp_file = fs::File::create(&temp_path).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("editor autosave atomic write failed (create temp): {error}"),
        )
    })?;
    std::io::Write::write_all(&mut temp_file, bytes).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("editor autosave atomic write failed (write temp): {error}"),
        )
    })?;
    temp_file.sync_all().map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("editor autosave atomic write failed (sync temp): {error}"),
        )
    })?;
    drop(temp_file);

    if let Err(replace_error) = replace_fn(&temp_path, path).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("editor autosave atomic write failed (replace target): {error}"),
        )
    }) {
        if let Err(cleanup_error) = cleanup_editor_temp_file(&temp_path) {
            return Err(io::Error::new(
                replace_error.kind(),
                format!("{replace_error}; cleanup temp failed: {cleanup_error}"),
            ));
        }
        return Err(replace_error);
    }

    Ok(())
}

fn cleanup_editor_temp_file(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn replace_editor_target_with_temp(temp_path: &Path, target_path: &Path) -> io::Result<()> {
    // Safety invariant: do not delete existing target before replacement succeeds.
    // A replace failure must keep the last-good autosave target file intact.
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::ffi::OsStrExt;
        use std::ptr::{null, null_mut};

        use windows_sys::Win32::Storage::FileSystem::ReplaceFileW;

        if !target_path.exists() {
            return fs::rename(temp_path, target_path);
        }

        let mut target_wide = target_path
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<u16>>();
        let mut temp_wide = temp_path
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<u16>>();

        let result = unsafe {
            ReplaceFileW(
                target_wide.as_mut_ptr(),
                temp_wide.as_mut_ptr(),
                null(),
                0,
                null_mut(),
                null_mut(),
            )
        };
        if result == 0 {
            return Err(io::Error::last_os_error());
        }
        return Ok(());
    }

    #[cfg(not(target_os = "windows"))]
    {
        fs::rename(temp_path, target_path)
    }
}

fn editor_temp_path_for_atomic_write(path: &Path) -> io::Result<PathBuf> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "editor autosave path has no parent directory",
        )
    })?;
    let file_name = path.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "editor autosave path has no file name",
        )
    })?;

    Ok(parent.join(format!("{}.tmp", file_name.to_string_lossy())))
}

impl crate::app::Papyru2App {
    pub(crate) fn sync_current_editing_path_to_components(
        &mut self,
        path: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        let autosave_path = path.clone();
        self.singleline.update(cx, |singleline, _| {
            singleline.set_current_editing_file_path(path.clone());
        });
        self.editor.update(cx, |editor, _| {
            editor.set_current_editing_file_path(path);
        });
        self.editor_autosave.on_edit_path_changed(autosave_path);

        let sl_path = self.singleline.read(cx).current_editing_file_path();
        let ed_path = self.editor.read(cx).current_editing_file_path();
        crate::app::trace_debug(format!(
            "current_edit_path sync singleline={} editor={}",
            sl_path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "<none>".to_string()),
            ed_path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "<none>".to_string())
        ));
    }

    pub(crate) fn apply_forced_singleline_stem(
        &mut self,
        forced_stem: Option<String>,
        trace_label: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(forced_stem) = forced_stem else {
            crate::app::trace_debug(format!(
                "{trace_label} force singleline stem update skipped (req-newf32)"
            ));
            return;
        };

        crate::app::trace_debug(format!(
            "{trace_label} force singleline stem update='{}'",
            crate::app::compact_text(&forced_stem)
        ));
        self.singleline.update(cx, |singleline, cx| {
            singleline.apply_text_and_cursor(
                forced_stem.clone(),
                forced_stem.chars().count(),
                window,
                cx,
            );
        });
    }

    pub(crate) fn ensure_new_file_flow(
        &mut self,
        trigger: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.file_workflow.state() != SinglelineFileState::Neutral {
            return;
        }

        let singleline_snapshot = self.singleline.read(cx).snapshot(cx);
        let singleline_was_focused = self.singleline.read(cx).is_focused(window, cx);
        let editor_was_focused = self.editor.read(cx).is_focused(window, cx);
        crate::app::trace_debug(format!(
            "new_file_flow trigger={} state=NEUTRAL singleline='{}' singleline_focused={} editor_focused={}",
            trigger,
            crate::app::compact_text(&singleline_snapshot.value),
            singleline_was_focused,
            editor_was_focused
        ));

        let now_local = Local::now();
        match self.file_workflow.try_create_from_neutral(
            &singleline_snapshot.value,
            self.app_paths.user_document_dir.as_path(),
            Instant::now(),
            now_local,
        ) {
            Ok(Some(path)) => {
                crate::app::trace_debug(format!("new_file_flow created path={}", path.display()));
                self.sync_current_editing_path_to_components(Some(path.clone()), cx);
                if crate::app::req_ftr14_create_flow_uses_watcher_refresh_only() {
                    crate::app::trace_debug(
                        "new_file_flow watcher_refresh_only=true direct_refresh_skipped",
                    );
                }
                self.apply_forced_singleline_stem(
                    forced_singleline_stem_after_create(
                        &singleline_snapshot.value,
                        path.as_path(),
                        now_local,
                    ),
                    "new_file_flow",
                    window,
                    cx,
                );
                self.editor.update(cx, |editor, cx| {
                    let _ = editor.open_file(path, window, cx);
                });

                if crate::app::should_restore_singleline_focus_after_new_file(
                    singleline_was_focused,
                    editor_was_focused,
                ) {
                    let singleline_after = self.singleline.read(cx).snapshot(cx);
                    let restore_cursor_char = singleline_snapshot
                        .cursor_char
                        .min(singleline_after.value.chars().count());

                    crate::app::trace_debug(format!(
                        "new_file_flow restore singleline focus cursor={} (rule-1)",
                        restore_cursor_char
                    ));
                    self.singleline.update(cx, |singleline, cx| {
                        singleline.apply_cursor(restore_cursor_char, window, cx);
                        singleline.focus(window, cx);
                    });
                } else {
                    crate::app::trace_debug("new_file_flow no focus restore (rule-2)");
                }
            }
            Ok(None) => {
                crate::app::trace_debug(format!(
                    "new_file_flow trigger={} skipped (state/throttle gate)",
                    trigger
                ));
            }
            Err(error) => {
                crate::app::trace_debug(format!(
                    "new_file_flow trigger={} failed error={error}",
                    trigger
                ));
            }
        }
    }

    pub(crate) fn on_editor_user_buffer_changed(&mut self, value: &str, cx: &mut Context<Self>) {
        let snapshot = self.file_workflow.snapshot();
        let Some(current_path) = snapshot.current_edit_path.clone() else {
            crate::app::trace_debug(format!(
                "autosave critical invalid path on user edit state={:?} text_len={}",
                snapshot.state,
                value.len()
            ));
            debug_assert!(
                false,
                "autosave invariant violation: current_edit_path must be present on editor user edit"
            );
            return;
        };

        if snapshot.state != SinglelineFileState::Edit {
            crate::app::trace_debug(format!(
                "autosave critical invalid state on user edit state={:?} path={}",
                snapshot.state,
                current_path.display()
            ));
            debug_assert!(
                false,
                "autosave invariant violation: state must be EDIT on editor user edit"
            );
            return;
        }

        let editor_path = self.editor.read(cx).current_editing_file_path();
        if editor_path.as_ref() != Some(&current_path) {
            crate::app::trace_debug(format!(
                "autosave path mismatch workflow={} editor={} (resync)",
                current_path.display(),
                editor_path
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "<none>".to_string())
            ));
            self.sync_current_editing_path_to_components(Some(current_path.clone()), cx);
        }

        crate::app::trace_debug(format!(
            "autosave step-2 pin user edit path={} text_len={}",
            current_path.display(),
            value.len()
        ));

        self.editor_autosave.mark_user_edit(
            EditorAutoSavePayload {
                user_document_dir: self.app_paths.user_document_dir.clone(),
                current_path,
                editor_text: value.to_string(),
            },
            Instant::now(),
        );
    }

    pub(crate) fn flush_editor_content_before_context_switch(
        &mut self,
        trigger: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        let snapshot = self.file_workflow.snapshot();
        if snapshot.state != SinglelineFileState::Edit {
            crate::app::trace_debug(format!(
                "autosave pre-switch trigger={} skipped state={:?}",
                trigger, snapshot.state
            ));
            return true;
        }

        let Some(current_path) = snapshot.current_edit_path.clone() else {
            crate::app::trace_debug(format!(
                "autosave pre-switch trigger={} critical missing path state={:?}",
                trigger, snapshot.state
            ));
            debug_assert!(
                false,
                "autosave invariant violation: current_edit_path must be present for pre-switch flush"
            );
            return false;
        };

        let editor_snapshot = self.editor.read(cx).snapshot(cx);
        crate::app::trace_debug(format!(
            "autosave pre-switch trigger={} raise path={} text_len={}",
            trigger,
            current_path.display(),
            editor_snapshot.value.len()
        ));

        let flush_result = self.file_workflow.flush_editor_content_in_edit(
            &editor_snapshot.value,
            self.app_paths.user_document_dir.as_path(),
        );
        self.editor_autosave.reset_cycle();

        match flush_result {
            Ok(true) => {
                let resolved_path = self
                    .file_workflow
                    .current_edit_path()
                    .unwrap_or_else(|| current_path.clone());
                if resolved_path != current_path {
                    crate::app::trace_debug(format!(
                        "req-newf35 pre-switch path updated old={} new={}",
                        current_path.display(),
                        resolved_path.display()
                    ));
                    self.sync_current_editing_path_to_components(Some(resolved_path.clone()), cx);
                }
                crate::app::trace_debug(format!(
                    "autosave pre-switch trigger={} consumed path={}",
                    trigger,
                    resolved_path.display()
                ));
                true
            }
            Ok(false) => {
                crate::app::trace_debug(format!(
                    "autosave pre-switch trigger={} no-op by workflow gate path={}",
                    trigger,
                    current_path.display()
                ));
                true
            }
            Err(error) => {
                crate::app::trace_debug(format!(
                    "autosave pre-switch trigger={} failed path={} error={error}",
                    trigger,
                    current_path.display()
                ));
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::DateTime;
    use std::time::UNIX_EPOCH;

    fn fixed_now() -> DateTime<Local> {
        DateTime::parse_from_rfc3339("2026-02-28T12:34:56.789+00:00")
            .expect("parse fixed timestamp")
            .with_timezone(&Local)
    }

    fn new_temp_root(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        let stamp = std::time::SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        path.push(format!(
            "papyru2_singleline_newfile_{name}_{}_{}",
            std::process::id(),
            stamp
        ));
        fs::create_dir_all(&path).expect("create temp root");
        path
    }

    fn remove_temp_root(path: &Path) {
        let _ = fs::remove_dir_all(path);
    }

    #[test]
    fn newf_test1_startup_state_is_neutral() {
        let workflow = SinglelineCreateFileWorkflow::new();
        assert_eq!(workflow.state(), SinglelineFileState::Neutral);
        workflow.dispatcher.shutdown();
    }

    #[test]
    fn newf_test2_startup_has_no_current_edit_path() {
        let workflow = SinglelineCreateFileWorkflow::new();
        assert_eq!(workflow.current_edit_path(), None);
        workflow.dispatcher.shutdown();
    }

    #[test]
    fn newf_test3_neutral_create_success_transitions_to_edit() {
        let root = new_temp_root("newf_test3");
        let workflow = SinglelineCreateFileWorkflow::new();
        let created = workflow
            .try_create_from_neutral("hello", root.as_path(), Instant::now(), fixed_now())
            .expect("create from neutral");

        assert!(created.is_some());
        assert_eq!(workflow.state(), SinglelineFileState::Edit);
        assert!(workflow.current_edit_path().is_some());
        workflow.dispatcher.shutdown();
        remove_temp_root(root.as_path());
    }

    #[test]
    fn newf_test4_create_event_throttle_blocks_second_raise_within_one_second() {
        let root = new_temp_root("newf_test4");
        let workflow = SinglelineCreateFileWorkflow::new();
        let now = Instant::now();

        let first = workflow
            .try_create_from_neutral("hello", root.as_path(), now, fixed_now())
            .expect("first create");
        assert!(first.is_some());

        let transitioned = workflow.transition_edit_to_neutral();
        assert!(transitioned);

        let second = workflow
            .try_create_from_neutral(
                "world",
                root.as_path(),
                now + Duration::from_millis(500),
                fixed_now(),
            )
            .expect("second create");
        assert!(second.is_none());
        assert_eq!(workflow.state(), SinglelineFileState::Neutral);
        workflow.dispatcher.shutdown();
        remove_temp_root(root.as_path());
    }

    #[test]
    fn newf_test5_edit_plus_transition_resets_to_neutral() {
        let root = new_temp_root("newf_test5");
        let workflow = SinglelineCreateFileWorkflow::new();
        workflow
            .try_create_from_neutral("hello", root.as_path(), Instant::now(), fixed_now())
            .expect("create");
        assert!(workflow.transition_edit_to_neutral());
        assert_eq!(workflow.state(), SinglelineFileState::Neutral);
        assert_eq!(workflow.current_edit_path(), None);
        workflow.dispatcher.shutdown();
        remove_temp_root(root.as_path());
    }

    #[test]
    fn newf_test6_plus_noop_in_neutral_and_new() {
        let root = new_temp_root("newf_test6");
        let workflow = SinglelineCreateFileWorkflow::new();
        assert!(!workflow.transition_edit_to_neutral());

        let now = Instant::now();
        workflow
            .try_create_from_neutral("hello", root.as_path(), now, fixed_now())
            .expect("create");
        assert!(workflow.transition_edit_to_neutral());
        let blocked = workflow
            .try_create_from_neutral(
                "x",
                root.as_path(),
                now + Duration::from_millis(100),
                fixed_now(),
            )
            .expect("create blocked");
        assert!(blocked.is_none());
        assert!(!workflow.transition_edit_to_neutral());
        workflow.dispatcher.shutdown();
        remove_temp_root(root.as_path());
    }

    #[test]
    fn newf_test7_notitle_format_used_for_empty_buffer() {
        let stem = stem_from_singleline_value("", fixed_now());
        assert!(stem.starts_with("notitle-"));
        assert_eq!(stem.len(), "notitle-".len() + 17);
        assert!(
            stem["notitle-".len()..]
                .chars()
                .all(|ch| ch.is_ascii_digit())
        );
    }

    #[test]
    fn newf_test8_daily_directory_uses_yyyy_mm_dd() {
        let root = PathBuf::from("C:/tmp/root");
        let dir = daily_directory(root.as_path(), fixed_now());
        assert!(dir.ends_with(Path::new("2026").join("02").join("28")));
    }

    #[test]
    fn ftr_test56_startup_daily_directory_helper_creates_missing_yyyy_mm_dd() {
        let root = new_temp_root("ftr_test56");
        let created = ensure_daily_directory(root.as_path(), fixed_now())
            .expect("ensure startup daily directory");

        assert!(created.ends_with(Path::new("2026").join("02").join("28")));
        assert!(created.is_dir());

        remove_temp_root(root.as_path());
    }

    #[test]
    fn newf_test9_collision_suffix_appends_before_txt() {
        let root = new_temp_root("newf_test9");
        let dir = daily_directory(root.as_path(), fixed_now());
        fs::create_dir_all(&dir).expect("create daily directory");
        fs::write(dir.join("hello.txt"), "").expect("write hello.txt");
        fs::write(dir.join("hello_2.txt"), "").expect("write hello_2.txt");

        let created = create_new_text_file(&CreateFileRequest {
            user_document_dir: root.clone(),
            singleline_value: "hello".to_string(),
            now: fixed_now(),
        })
        .expect("create new text file");

        assert!(created.ends_with(Path::new("hello_3.txt")));
        remove_temp_root(root.as_path());
    }

    #[test]
    fn newf_test10_non_empty_buffer_uses_buffer_txt() {
        let stem = stem_from_singleline_value("hello world", fixed_now());
        assert_eq!(stem, "hello world");
    }

    #[test]
    fn newf_test11_multibyte_buffer_is_preserved() {
        let stem = stem_from_singleline_value("こんにちは 世界", fixed_now());
        assert_eq!(stem, "こんにちは 世界");
    }

    #[test]
    fn newf_test12_invalid_filename_characters_replaced_with_underscore() {
        let stem = stem_from_singleline_value("he<l>l:o*?\"|/\\\\", fixed_now());
        assert_eq!(stem, "he_l_l_o_______");
    }

    #[test]
    fn newf_test13_filename_stem_is_trimmed_to_64_chars() {
        let source = "a".repeat(80);
        let stem = stem_from_singleline_value(source.as_str(), fixed_now());
        assert_eq!(stem.chars().count(), 64);
    }

    #[test]
    fn newf_test14_open_file_transition_sets_edit_path() {
        let workflow = SinglelineCreateFileWorkflow::new();
        let path = PathBuf::from("C:/tmp/some.txt");
        workflow.set_edit_from_open_file(path.clone());
        assert_eq!(workflow.state(), SinglelineFileState::Edit);
        assert_eq!(workflow.current_edit_path(), Some(path));
        workflow.dispatcher.shutdown();
    }

    #[test]
    fn newf_test15_edit_rename_updates_current_path() {
        let root = new_temp_root("newf_test15");
        let workflow = SinglelineCreateFileWorkflow::new();
        let created = workflow
            .try_create_from_neutral("start", root.as_path(), Instant::now(), fixed_now())
            .expect("create")
            .expect("path");
        assert!(created.exists());

        let renamed = workflow
            .try_rename_in_edit("next", root.as_path(), fixed_now())
            .expect("rename in edit")
            .expect("renamed path");
        assert!(renamed.ends_with(Path::new("next.txt")));
        assert!(renamed.exists());
        assert_eq!(workflow.current_edit_path(), Some(renamed));
        workflow.dispatcher.shutdown();
        remove_temp_root(root.as_path());
    }

    #[test]
    fn newf_test16_rename_event_is_noop_when_not_in_edit() {
        let workflow = SinglelineCreateFileWorkflow::new();
        let renamed = workflow
            .try_rename_in_edit("next", Path::new("C:/tmp"), fixed_now())
            .expect("rename in neutral");
        assert!(renamed.is_none());
        workflow.dispatcher.shutdown();
    }

    #[test]
    fn newf_test17_create_event_only_when_state_is_neutral() {
        let root = new_temp_root("newf_test17");
        let workflow = SinglelineCreateFileWorkflow::new();
        workflow
            .try_create_from_neutral("hello", root.as_path(), Instant::now(), fixed_now())
            .expect("create 1");
        let second = workflow
            .try_create_from_neutral(
                "world",
                root.as_path(),
                Instant::now() + Duration::from_secs(2),
                fixed_now(),
            )
            .expect("create 2");
        assert!(second.is_none());
        workflow.dispatcher.shutdown();
        remove_temp_root(root.as_path());
    }

    #[test]
    fn newf_test18_event_queue_preserves_fifo_order_for_creates() {
        let root = new_temp_root("newf_test18");
        let dispatcher = FileWorkflowEventDispatcher::new();
        let first = dispatcher
            .dispatch_blocking(FileWorkflowEvent::Create(CreateFileRequest {
                user_document_dir: root.clone(),
                singleline_value: "a".to_string(),
                now: fixed_now(),
            }))
            .expect("first create");
        let second = dispatcher
            .dispatch_blocking(FileWorkflowEvent::Create(CreateFileRequest {
                user_document_dir: root.clone(),
                singleline_value: "b".to_string(),
                now: fixed_now(),
            }))
            .expect("second create");

        let first_path = match first {
            FileWorkflowEventResult::Created { path } => path,
            _ => panic!("unexpected first result"),
        };
        let second_path = match second {
            FileWorkflowEventResult::Created { path } => path,
            _ => panic!("unexpected second result"),
        };

        assert!(first_path.file_name().and_then(|n| n.to_str()) == Some("a.txt"));
        assert!(second_path.file_name().and_then(|n| n.to_str()) == Some("b.txt"));
        dispatcher.shutdown();
        remove_temp_root(root.as_path());
    }

    #[test]
    fn newf_test19_event_dispatcher_supports_multi_producer_single_consumer() {
        let root = new_temp_root("newf_test19");
        let dispatcher = FileWorkflowEventDispatcher::new();

        let mut threads = Vec::new();
        for ix in 0..4 {
            let dispatcher = dispatcher.clone();
            let root = root.clone();
            threads.push(thread::spawn(move || {
                dispatcher.dispatch_blocking(FileWorkflowEvent::Create(CreateFileRequest {
                    user_document_dir: root,
                    singleline_value: format!("p{ix}"),
                    now: fixed_now(),
                }))
            }));
        }

        for handle in threads {
            let result = handle.join().expect("join producer thread");
            assert!(result.is_ok());
        }

        let created_dir = daily_directory(root.as_path(), fixed_now());
        let count = fs::read_dir(created_dir).expect("read dir").count();
        assert_eq!(count, 4);
        dispatcher.shutdown();
        remove_temp_root(root.as_path());
    }

    #[test]
    fn newf_test20_create_and_rename_support_multibyte_text() {
        let root = new_temp_root("newf_test20");
        let workflow = SinglelineCreateFileWorkflow::new();
        workflow
            .try_create_from_neutral("こんにちは", root.as_path(), Instant::now(), fixed_now())
            .expect("create");

        let renamed = workflow
            .try_rename_in_edit("こんにちは 世界", root.as_path(), fixed_now())
            .expect("rename")
            .expect("renamed path");
        assert!(
            renamed
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name == "こんにちは 世界.txt")
        );
        workflow.dispatcher.shutdown();
        remove_temp_root(root.as_path());
    }

    #[test]
    fn newf_test21_rename_collision_uses_suffix() {
        let root = new_temp_root("newf_test21");
        let workflow = SinglelineCreateFileWorkflow::new();
        let created = workflow
            .try_create_from_neutral("base", root.as_path(), Instant::now(), fixed_now())
            .expect("create")
            .expect("path");

        let parent = created.parent().expect("parent").to_path_buf();
        fs::write(parent.join("renamed.txt"), "").expect("seed renamed.txt");

        let renamed = workflow
            .try_rename_in_edit("renamed", root.as_path(), fixed_now())
            .expect("rename")
            .expect("path");
        assert!(renamed.ends_with(Path::new("renamed_2.txt")));
        workflow.dispatcher.shutdown();
        remove_temp_root(root.as_path());
    }

    #[test]
    fn newf_test22_rename_to_same_name_is_noop() {
        let root = new_temp_root("newf_test22");
        let workflow = SinglelineCreateFileWorkflow::new();
        let created = workflow
            .try_create_from_neutral("same", root.as_path(), Instant::now(), fixed_now())
            .expect("create")
            .expect("path");

        let renamed = workflow
            .try_rename_in_edit("same", root.as_path(), fixed_now())
            .expect("rename")
            .expect("path");
        assert_eq!(created, renamed);
        workflow.dispatcher.shutdown();
        remove_temp_root(root.as_path());
    }

    #[test]
    fn newf_test23_reset_startup_to_neutral_clears_current_edit_path() {
        let workflow = SinglelineCreateFileWorkflow::new();
        workflow.set_edit_from_open_file(PathBuf::from("C:/tmp/file.txt"));
        workflow.reset_startup_to_neutral();
        let snapshot = workflow.snapshot();
        assert_eq!(snapshot.state, SinglelineFileState::Neutral);
        assert!(snapshot.current_edit_path.is_none());
        workflow.dispatcher.shutdown();
    }

    #[test]
    fn newf_test24_create_path_is_under_user_document_yyyy_mm_dd() {
        let root = new_temp_root("newf_test24");
        let path = create_new_text_file(&CreateFileRequest {
            user_document_dir: root.clone(),
            singleline_value: "abc".to_string(),
            now: fixed_now(),
        })
        .expect("create new file");

        let daily = daily_directory(root.as_path(), fixed_now());
        assert!(path.starts_with(daily));
        remove_temp_root(root.as_path());
    }

    #[test]
    fn newf_test25_collision_does_not_force_singleline_buffer_stem_update() {
        let root = new_temp_root("newf_test25");
        let now = fixed_now();
        let _first = create_new_text_file(&CreateFileRequest {
            user_document_dir: root.clone(),
            singleline_value: "filename".to_string(),
            now,
        })
        .expect("create first file");
        let second = create_new_text_file(&CreateFileRequest {
            user_document_dir: root.clone(),
            singleline_value: "filename".to_string(),
            now,
        })
        .expect("create second file");

        assert!(second.ends_with(Path::new("filename_2.txt")));
        let forced = forced_singleline_stem_after_create("filename", second.as_path(), now);
        assert!(forced.is_none());
        remove_temp_root(root.as_path());
    }

    #[test]
    fn newf_test26_sanitization_does_not_force_singleline_buffer_stem_update() {
        let root = new_temp_root("newf_test26");
        let now = fixed_now();
        let created = create_new_text_file(&CreateFileRequest {
            user_document_dir: root.clone(),
            singleline_value: "file:name".to_string(),
            now,
        })
        .expect("create sanitized file");

        assert!(created.ends_with(Path::new("file_name.txt")));
        let forced = forced_singleline_stem_after_create("file:name", created.as_path(), now);
        assert!(forced.is_none());
        remove_temp_root(root.as_path());
    }

    #[test]
    fn newf_test27_create_error_rolls_back_to_neutral() {
        let root = new_temp_root("newf_test27");
        let blocked = root.join("blocked");
        fs::write(&blocked, "not a directory").expect("create blocking file");
        let workflow = SinglelineCreateFileWorkflow::new();

        let create_error = workflow
            .try_create_from_neutral("hello", blocked.as_path(), Instant::now(), fixed_now())
            .expect_err("create should fail");
        assert!(
            create_error.kind() == io::ErrorKind::NotADirectory
                || create_error.kind() == io::ErrorKind::AlreadyExists
                || create_error.kind() == io::ErrorKind::PermissionDenied
                || create_error.kind() == io::ErrorKind::Other
        );
        assert_eq!(workflow.state(), SinglelineFileState::Neutral);
        assert!(workflow.current_edit_path().is_none());
        workflow.dispatcher.shutdown();
        remove_temp_root(root.as_path());
    }

    #[test]
    fn newf_test28_create_success_still_transitions_to_edit() {
        let root = new_temp_root("newf_test28");
        let workflow = SinglelineCreateFileWorkflow::new();
        let created = workflow
            .try_create_from_neutral("hello", root.as_path(), Instant::now(), fixed_now())
            .expect("create should succeed")
            .expect("created path");
        assert!(created.exists());
        assert_eq!(workflow.state(), SinglelineFileState::Edit);
        assert_eq!(workflow.current_edit_path(), Some(created));
        workflow.dispatcher.shutdown();
        remove_temp_root(root.as_path());
    }

    #[test]
    fn newf_test29_create_retries_on_name_conflict() {
        let root = new_temp_root("newf_test29");
        let now = fixed_now();
        let daily = daily_directory(root.as_path(), now);
        fs::create_dir_all(&daily).expect("create daily directory");
        fs::write(daily.join("race.txt"), "existing").expect("seed existing file");

        let created = create_new_text_file(&CreateFileRequest {
            user_document_dir: root.clone(),
            singleline_value: "race".to_string(),
            now,
        })
        .expect("create with conflict retry");
        assert!(created.ends_with(Path::new("race_2.txt")));
        assert_eq!(
            fs::read_to_string(daily.join("race.txt")).expect("read seed"),
            "existing"
        );
        remove_temp_root(root.as_path());
    }

    #[test]
    fn newf_test30_rename_retries_on_name_conflict() {
        let root = new_temp_root("newf_test30");
        let now = fixed_now();
        let daily = daily_directory(root.as_path(), now);
        fs::create_dir_all(&daily).expect("create daily directory");
        let source = daily.join("source.txt");
        fs::write(&source, "source").expect("seed source");
        fs::write(daily.join("target.txt"), "target").expect("seed conflict target");

        let renamed = rename_text_file(&RenameFileRequest {
            user_document_dir: root.clone(),
            current_path: source.clone(),
            singleline_value: "target".to_string(),
            now,
        })
        .expect("rename with conflict retry");
        assert!(renamed.ends_with(Path::new("target_2.txt")));
        assert!(!source.exists());
        assert_eq!(
            fs::read_to_string(daily.join("target.txt")).expect("read target"),
            "target"
        );
        remove_temp_root(root.as_path());
    }

    #[test]
    fn newf_test31_req_newf33_create_collision_keeps_existing_file_and_uses_suffix() {
        let root = new_temp_root("newf_test31");
        let now = fixed_now();
        let daily = daily_directory(root.as_path(), now);
        fs::create_dir_all(&daily).expect("create daily directory");
        let existing = daily.join("same.txt");
        fs::write(&existing, "existing").expect("seed existing file");

        let created = create_new_text_file(&CreateFileRequest {
            user_document_dir: root.clone(),
            singleline_value: "same".to_string(),
            now,
        })
        .expect("create with collision suffix");

        assert!(created.ends_with(Path::new("same_2.txt")));
        assert_eq!(
            fs::read_to_string(&existing).expect("read existing content"),
            "existing"
        );
        remove_temp_root(root.as_path());
    }

    #[test]
    fn newf_test32_req_newf33_rename_collision_preserves_existing_target_content() {
        let root = new_temp_root("newf_test32");
        let now = fixed_now();
        let daily = daily_directory(root.as_path(), now);
        fs::create_dir_all(&daily).expect("create daily directory");
        let source = daily.join("source.txt");
        let target = daily.join("target.txt");
        fs::write(&source, "source-content").expect("seed source");
        fs::write(&target, "target-content").expect("seed target");

        let renamed = rename_text_file(&RenameFileRequest {
            user_document_dir: root.clone(),
            current_path: source.clone(),
            singleline_value: "target".to_string(),
            now,
        })
        .expect("rename with suffix");

        assert!(renamed.ends_with(Path::new("target_2.txt")));
        assert_eq!(
            fs::read_to_string(&target).expect("read original target"),
            "target-content"
        );
        assert_eq!(
            fs::read_to_string(&renamed).expect("read renamed source"),
            "source-content"
        );
        assert!(!source.exists());
        remove_temp_root(root.as_path());
    }

    #[test]
    fn newf_test33_req_newf32_forced_singleline_stem_is_disabled_for_rename_resolution() {
        let root = new_temp_root("newf_test33");
        let now = fixed_now();
        let daily = daily_directory(root.as_path(), now);
        fs::create_dir_all(&daily).expect("create daily directory");
        let source_collision = daily.join("source_collision.txt");
        fs::write(&source_collision, "source-collision").expect("seed collision source");
        fs::write(daily.join("conflict.txt"), "existing").expect("seed conflict");

        let renamed_collision = rename_text_file(&RenameFileRequest {
            user_document_dir: root.clone(),
            current_path: source_collision,
            singleline_value: "conflict".to_string(),
            now,
        })
        .expect("rename collision");
        assert!(renamed_collision.ends_with(Path::new("conflict_2.txt")));
        assert!(
            forced_singleline_stem_after_rename("conflict", renamed_collision.as_path(), now)
                .is_none()
        );

        let source_sanitize = daily.join("source_sanitize.txt");
        fs::write(&source_sanitize, "source-sanitize").expect("seed sanitize source");
        let renamed_sanitize = rename_text_file(&RenameFileRequest {
            user_document_dir: root.clone(),
            current_path: source_sanitize,
            singleline_value: "file:name".to_string(),
            now,
        })
        .expect("rename sanitize");
        assert!(renamed_sanitize.ends_with(Path::new("file_name.txt")));
        assert!(
            forced_singleline_stem_after_rename("file:name", renamed_sanitize.as_path(), now)
                .is_none()
        );

        remove_temp_root(root.as_path());
    }

    #[test]
    fn newf_test36_req_newf35_rename_update_moves_existing_file_to_today_and_updates_path() {
        let root = new_temp_root("newf_test36");
        let now = fixed_now();
        let today_dir = daily_directory(root.as_path(), now);
        fs::create_dir_all(&today_dir).expect("create today directory");
        let old_dir = root.join("1999").join("01").join("01");
        fs::create_dir_all(&old_dir).expect("create old directory");
        let source = old_dir.join("fileA.txt");
        fs::write(&source, "A-old").expect("seed source");

        let workflow = SinglelineCreateFileWorkflow::new();
        workflow.set_edit_from_open_file(source.clone());

        let renamed = workflow
            .try_rename_in_edit("fileB", root.as_path(), now)
            .expect("rename in edit")
            .expect("renamed path");
        assert!(renamed.starts_with(today_dir.as_path()));
        assert!(renamed.ends_with(Path::new("fileB.txt")));
        assert!(renamed.exists());
        assert!(!source.exists());
        assert_eq!(workflow.current_edit_path(), Some(renamed));
        workflow.dispatcher.shutdown();
        remove_temp_root(root.as_path());
    }

    #[test]
    fn newf_test37_req_newf35_autosave_update_moves_existing_file_to_today_and_updates_path() {
        let root = new_temp_root("newf_test37");
        let old_dir = root.join("1999").join("01").join("01");
        fs::create_dir_all(&old_dir).expect("create old directory");
        let source = old_dir.join("fileA.txt");
        fs::write(&source, "A-old").expect("seed source");

        let workflow = SinglelineCreateFileWorkflow::new();
        workflow.set_edit_from_open_file(source.clone());

        let before = Local::now();
        let saved = workflow
            .try_autosave_in_edit(EditorAutoSavePayload {
                user_document_dir: root.clone(),
                current_path: source.clone(),
                editor_text: "A-new".to_string(),
            })
            .expect("autosave after move");
        let after = Local::now();

        assert!(saved);
        let current = workflow
            .current_edit_path()
            .expect("current edit path after autosave");
        let today_before = daily_directory(root.as_path(), before);
        let today_after = daily_directory(root.as_path(), after);
        assert!(
            is_path_under_daily_directory(current.as_path(), today_before.as_path())
                || is_path_under_daily_directory(current.as_path(), today_after.as_path())
        );
        assert!(current.ends_with(Path::new("fileA.txt")));
        assert_eq!(
            fs::read_to_string(&current).expect("read moved file"),
            "A-new"
        );
        assert!(!source.exists());
        workflow.dispatcher.shutdown();
        remove_temp_root(root.as_path());
    }

    #[test]
    fn newf_test38_req_newf35_noop_when_path_is_already_today_daily_dir() {
        let root = new_temp_root("newf_test38");
        let now = fixed_now();
        let today_dir = daily_directory(root.as_path(), now);
        fs::create_dir_all(&today_dir).expect("create today directory");
        let source = today_dir.join("fileA.txt");
        fs::write(&source, "A-old").expect("seed source");

        let renamed = rename_text_file(&RenameFileRequest {
            user_document_dir: root.clone(),
            current_path: source.clone(),
            singleline_value: "fileA".to_string(),
            now,
        })
        .expect("rename no-op in today directory");

        assert_eq!(renamed, source);
        assert!(renamed.exists());
        assert_eq!(
            fs::read_to_string(&renamed).expect("read no-op file"),
            "A-old"
        );
        remove_temp_root(root.as_path());
    }

    #[test]
    fn newf_test39_req_newf36_event_b_rename_uses_event_a_updated_path() {
        let root = new_temp_root("newf_test39");
        let now = fixed_now();
        let today_dir = daily_directory(root.as_path(), now);
        fs::create_dir_all(&today_dir).expect("create today directory");
        let old_dir = root.join("1999").join("01").join("01");
        fs::create_dir_all(&old_dir).expect("create old directory");
        let source = old_dir.join("fileA.txt");
        fs::write(&source, "A-old").expect("seed source");

        let workflow = SinglelineCreateFileWorkflow::new();
        workflow.set_edit_from_open_file(source.clone());

        let event_a_path = workflow
            .try_rename_in_edit("fileA", root.as_path(), now)
            .expect("event-a rename")
            .expect("event-a path");
        assert!(event_a_path.starts_with(today_dir.as_path()));
        assert!(event_a_path.ends_with(Path::new("fileA.txt")));

        let event_b_path = workflow
            .try_rename_in_edit("fileB", root.as_path(), now)
            .expect("event-b rename")
            .expect("event-b path");
        assert!(event_b_path.starts_with(today_dir.as_path()));
        assert!(event_b_path.ends_with(Path::new("fileB.txt")));
        assert!(!source.exists());
        assert!(!event_a_path.exists());
        assert_eq!(workflow.current_edit_path(), Some(event_b_path));
        workflow.dispatcher.shutdown();
        remove_temp_root(root.as_path());
    }

    #[test]
    fn newf_test40_req_newf36_stale_path_payload_is_rejected_after_move_commit() {
        let root = new_temp_root("newf_test40");
        let now = fixed_now();
        let old_dir = root.join("1999").join("01").join("01");
        fs::create_dir_all(&old_dir).expect("create old directory");
        let source = old_dir.join("fileA.txt");
        fs::write(&source, "A-old").expect("seed source");

        let workflow = SinglelineCreateFileWorkflow::new();
        workflow.set_edit_from_open_file(source.clone());

        let moved_path = workflow
            .try_rename_in_edit("fileA", root.as_path(), now)
            .expect("move via rename")
            .expect("moved path");
        assert!(!source.exists());

        let stale_saved = workflow
            .try_autosave_in_edit(EditorAutoSavePayload {
                user_document_dir: root.clone(),
                current_path: source,
                editor_text: "STALE".to_string(),
            })
            .expect("stale autosave call");

        assert!(!stale_saved);
        assert_eq!(
            fs::read_to_string(&moved_path).expect("read moved path content"),
            "A-old"
        );
        assert_eq!(workflow.current_edit_path(), Some(moved_path));
        workflow.dispatcher.shutdown();
        remove_temp_root(root.as_path());
    }

    #[test]
    fn aus_test1_autosave_event_writes_latest_editor_text() {
        let root = new_temp_root("aus_test1");
        let workflow = SinglelineCreateFileWorkflow::new();
        let path = workflow
            .try_create_from_neutral("autosave", root.as_path(), Instant::now(), fixed_now())
            .expect("create")
            .expect("created path");
        let payload = EditorAutoSavePayload {
            user_document_dir: root.clone(),
            current_path: path,
            editor_text: "line-1\nline-2".to_string(),
        };

        let saved = workflow
            .try_autosave_in_edit(payload)
            .expect("dispatch autosave");
        assert!(saved);
        let current = workflow
            .current_edit_path()
            .expect("current path after autosave");
        let content = fs::read_to_string(&current).expect("read autosaved file");
        assert_eq!(content, "line-1\nline-2");
        workflow.dispatcher.shutdown();
        remove_temp_root(root.as_path());
    }

    #[test]
    fn aus_test2_autosave_is_noop_when_not_in_edit_state() {
        let root = new_temp_root("aus_test2");
        let workflow = SinglelineCreateFileWorkflow::new();
        let path = root.join("not_edit.txt");
        let payload = EditorAutoSavePayload {
            user_document_dir: root.clone(),
            current_path: path.clone(),
            editor_text: "content".to_string(),
        };

        let saved = workflow
            .try_autosave_in_edit(payload)
            .expect("autosave in non-edit");
        assert!(!saved);
        assert!(!path.exists());
        workflow.dispatcher.shutdown();
        remove_temp_root(root.as_path());
    }

    #[test]
    fn aus_test3_autosave_is_noop_when_edit_path_is_missing() {
        let root = new_temp_root("aus_test3");
        let workflow = SinglelineCreateFileWorkflow::new();
        {
            let mut state = workflow
                .inner
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state.state = SinglelineFileState::Edit;
            state.current_edit_path = None;
        }
        let payload = EditorAutoSavePayload {
            user_document_dir: root.clone(),
            current_path: root.join("missing.txt"),
            editor_text: "content".to_string(),
        };

        let saved = workflow
            .try_autosave_in_edit(payload)
            .expect("autosave with missing edit path");
        assert!(!saved);
        workflow.dispatcher.shutdown();
        remove_temp_root(root.as_path());
    }

    #[test]
    fn aus_test6_atomic_autosave_failure_preserves_last_good_file() {
        let root = new_temp_root("aus_test6");
        let path = root.join("atomic.txt");
        fs::write(&path, "old").expect("seed old file");

        let error = write_editor_text_atomic_with_replace(&path, b"new", |_temp, _target| {
            Err(io::Error::other("forced replace failure"))
        })
        .expect_err("forced replace failure expected");
        assert!(error.to_string().contains("replace target"));

        let content = fs::read_to_string(&path).expect("read old content");
        assert_eq!(content, "old");
        remove_temp_root(root.as_path());
    }

    #[test]
    fn aus_test8_path_switch_drops_pending_payload() {
        let coordinator = EditorAutoSaveCoordinator::new();
        let now = Instant::now();
        let path_a = PathBuf::from("C:/tmp/a.txt");
        let path_b = PathBuf::from("C:/tmp/b.txt");
        coordinator.mark_user_edit(
            EditorAutoSavePayload {
                user_document_dir: PathBuf::from("C:/tmp"),
                current_path: path_a,
                editor_text: "stale".to_string(),
            },
            now,
        );

        coordinator.on_edit_path_changed(Some(path_b));
        assert!(!coordinator.has_pending_payload());
        let due =
            coordinator.pop_due_payload(now + Duration::from_secs(10), Duration::from_secs(6));
        assert!(due.is_none());
    }

    #[test]
    fn aus_test9_same_path_keeps_pending_payload() {
        let coordinator = EditorAutoSaveCoordinator::new();
        let now = Instant::now();
        let path_a = PathBuf::from("C:/tmp/a.txt");
        coordinator.mark_user_edit(
            EditorAutoSavePayload {
                user_document_dir: PathBuf::from("C:/tmp"),
                current_path: path_a.clone(),
                editor_text: "keep".to_string(),
            },
            now,
        );

        coordinator.on_edit_path_changed(Some(path_a.clone()));
        assert!(coordinator.has_pending_payload());
        let due = coordinator
            .pop_due_payload(now + Duration::from_secs(6), Duration::from_secs(6))
            .expect("due payload should remain");
        assert_eq!(due.current_path, path_a);
        assert_eq!(due.editor_text, "keep");
    }

    #[test]
    fn aus_test10_autosave_and_path_transition_are_serialized() {
        use std::sync::{Arc, Barrier, mpsc};

        let root = new_temp_root("aus_test10");
        let now = fixed_now();
        let daily = daily_directory(root.as_path(), now);
        fs::create_dir_all(&daily).expect("create daily directory");
        let path_a = daily.join("fileA.txt");
        let path_b = daily.join("fileB.txt");
        fs::write(&path_a, "A-old").expect("seed fileA");
        fs::write(&path_b, "B-old").expect("seed fileB");

        let workflow = SinglelineCreateFileWorkflow::new();
        workflow.set_edit_from_open_file(path_a.clone());

        let payload = EditorAutoSavePayload {
            user_document_dir: root.clone(),
            current_path: path_a.clone(),
            editor_text: "A-new".to_string(),
        };

        let barrier = Arc::new(Barrier::new(3));
        let (autosave_tx, autosave_rx) = mpsc::channel();
        let workflow_for_autosave = workflow.clone();
        let barrier_for_autosave = barrier.clone();
        let autosave_thread = thread::spawn(move || {
            barrier_for_autosave.wait();
            let result = workflow_for_autosave
                .try_autosave_in_edit(payload)
                .expect("autosave call");
            autosave_tx.send(result).expect("send autosave result");
        });

        let workflow_for_switch = workflow.clone();
        let barrier_for_switch = barrier.clone();
        let path_b_for_switch = path_b.clone();
        let switch_thread = thread::spawn(move || {
            barrier_for_switch.wait();
            workflow_for_switch.set_edit_from_open_file(path_b_for_switch);
        });

        barrier.wait();
        autosave_thread.join().expect("join autosave thread");
        switch_thread.join().expect("join switch thread");
        let autosaved = autosave_rx.recv().expect("receive autosave result");

        let content_b = fs::read_to_string(&path_b).expect("read fileB");
        assert_eq!(content_b, "B-old");

        let content_a = fs::read_to_string(&path_a).expect("read fileA");
        if autosaved {
            assert_eq!(content_a, "A-new");
        } else {
            assert_eq!(content_a, "A-old");
        }

        workflow.dispatcher.shutdown();
        remove_temp_root(root.as_path());
    }

    #[test]
    fn aus_test11_req_aus6_pre_new_file_flushes_before_edit_to_neutral() {
        let root = new_temp_root("aus_test11");
        let workflow = SinglelineCreateFileWorkflow::new();
        let path_a = workflow
            .try_create_from_neutral("fileA", root.as_path(), Instant::now(), fixed_now())
            .expect("create")
            .expect("created path");

        fs::write(&path_a, "A-old").expect("seed fileA");
        let flushed = workflow
            .flush_editor_content_in_edit("A-new", root.as_path())
            .expect("flush before plus");
        assert!(flushed);
        let updated_path = workflow
            .current_edit_path()
            .expect("current path after flush");
        let moved = workflow.transition_edit_to_neutral();
        assert!(moved);

        let content_a = fs::read_to_string(&updated_path).expect("read updated fileA path");
        assert_eq!(content_a, "A-new");
        workflow.dispatcher.shutdown();
        remove_temp_root(root.as_path());
    }

    #[test]
    fn aus_test12_req_aus8_pre_open_file_flushes_previous_file_before_switch() {
        let root = new_temp_root("aus_test12");
        let now = fixed_now();
        let daily = daily_directory(root.as_path(), now);
        fs::create_dir_all(&daily).expect("create daily directory");
        let path_a = daily.join("fileA.txt");
        let path_b = daily.join("fileB.txt");
        fs::write(&path_a, "A-old").expect("seed fileA");
        fs::write(&path_b, "B-old").expect("seed fileB");

        let workflow = SinglelineCreateFileWorkflow::new();
        workflow.set_edit_from_open_file(path_a.clone());

        let flushed = workflow
            .flush_editor_content_in_edit("A-new", root.as_path())
            .expect("flush before open fileB");
        assert!(flushed);
        let updated_path_a = workflow
            .current_edit_path()
            .expect("current path after pre-open flush");
        assert_eq!(
            fs::read_to_string(&updated_path_a).expect("read updated fileA"),
            "A-new"
        );

        workflow.set_edit_from_open_file(path_b.clone());

        let content_b = fs::read_to_string(&path_b).expect("read fileB");
        assert_eq!(content_b, "B-old");
        assert_eq!(workflow.current_edit_path(), Some(path_b));
        workflow.dispatcher.shutdown();
        remove_temp_root(root.as_path());
    }

    #[test]
    fn aus_test13_req_aus7_pre_close_flushes_without_path_transition() {
        let root = new_temp_root("aus_test13");
        let now = fixed_now();
        let daily = daily_directory(root.as_path(), now);
        fs::create_dir_all(&daily).expect("create daily directory");
        let path_a = daily.join("fileA.txt");
        fs::write(&path_a, "A-old").expect("seed fileA");

        let workflow = SinglelineCreateFileWorkflow::new();
        workflow.set_edit_from_open_file(path_a.clone());

        let flushed = workflow
            .flush_editor_content_in_edit("A-new", root.as_path())
            .expect("flush before close");
        assert!(flushed);
        let updated_path = workflow
            .current_edit_path()
            .expect("current path after pre-close flush");
        let content_a = fs::read_to_string(&updated_path).expect("read updated fileA");
        assert_eq!(content_a, "A-new");
        assert_eq!(workflow.current_edit_path(), Some(updated_path));
        workflow.dispatcher.shutdown();
        remove_temp_root(root.as_path());
    }

    #[test]
    fn qsrv_file_test1_rpc_pin_reads_content_clamps_line_and_updates_mtime() {
        let root = new_temp_root("qsrv_file_test1");
        let now = fixed_now();
        let daily = daily_directory(root.as_path(), now);
        fs::create_dir_all(&daily).expect("create daily directory");
        let target = daily.join("fileA.txt");
        fs::write(&target, "line1\nline2\nline3").expect("seed target file");

        let old = FileTime::from_unix_time(1, 0);
        set_file_mtime(target.as_path(), old).expect("set old modified time");
        let modified_before = fs::metadata(&target)
            .expect("metadata before rpc pin")
            .modified()
            .expect("modified before rpc pin");

        let workflow = SinglelineCreateFileWorkflow::new();
        let pinned = workflow
            .try_pin_file_via_rpc(target.clone(), 999)
            .expect("rpc pin must succeed");

        assert_eq!(pinned.path, target);
        assert_eq!(pinned.content, "line1\nline2\nline3");
        assert_eq!(pinned.linenum, 3);

        let modified_after = fs::metadata(&pinned.path)
            .expect("metadata after rpc pin")
            .modified()
            .expect("modified after rpc pin");
        assert!(modified_after >= modified_before);

        workflow.dispatcher.shutdown();
        remove_temp_root(root.as_path());
    }

    #[test]
    fn qsrv_file_test2_rpc_pin_missing_file_returns_not_found() {
        let root = new_temp_root("qsrv_file_test2");
        let workflow = SinglelineCreateFileWorkflow::new();
        let missing = root.join("2026").join("03").join("22").join("missing.txt");

        let error = workflow
            .try_pin_file_via_rpc(missing.clone(), 1)
            .expect_err("missing file must fail");
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert!(error.to_string().contains("does not exist"));

        workflow.dispatcher.shutdown();
        remove_temp_root(root.as_path());
    }
}
