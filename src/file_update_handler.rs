use std::{
    collections::VecDeque,
    fs, io,
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex, mpsc},
    thread,
    time::{Duration, Instant},
};

use chrono::{DateTime, Local};
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
    pub current_path: PathBuf,
    pub singleline_value: String,
    pub now: DateTime<Local>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EditorAutoSavePayload {
    pub current_path: PathBuf,
    pub editor_text: String,
}

#[derive(Debug, Clone)]
pub struct AutoSaveFileRequest {
    pub payload: EditorAutoSavePayload,
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
                if let Some(payload) = state.pending_payload.as_mut() {
                    payload.current_path = path;
                }
            }
            None => {
                state.pinned_time = None;
                state.pending_payload = None;
                state.last_delta_trace_secs = None;
            }
        }
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileWorkflowEventResult {
    Created { path: PathBuf },
    Renamed { path: PathBuf },
    AutoSaved { path: PathBuf },
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
    }
}

#[derive(Debug)]
struct WorkflowStateInner {
    state: SinglelineFileState,
    current_edit_path: Option<PathBuf>,
    last_create_event_raised_at: Option<Instant>,
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
        {
            let mut state = self
                .inner
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if state.state != SinglelineFileState::Neutral {
                return Ok(None);
            }

            state.state = SinglelineFileState::New;

            if let Some(last) = state.last_create_event_raised_at {
                let ready = now_instant
                    .checked_duration_since(last)
                    .map(|elapsed| elapsed > CREATE_EVENT_MIN_INTERVAL)
                    .unwrap_or(false);
                if !ready {
                    return Ok(None);
                }
            }

            state.last_create_event_raised_at = Some(now_instant);
        }

        let result = self
            .dispatcher
            .dispatch_blocking(FileWorkflowEvent::Create(CreateFileRequest {
                user_document_dir: user_document_dir.to_path_buf(),
                singleline_value: singleline_value.to_string(),
                now: now_local,
            }))?;

        match result {
            FileWorkflowEventResult::Created { path } => {
                let mut state = self
                    .inner
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                state.state = SinglelineFileState::Edit;
                state.current_edit_path = Some(path.clone());
                Ok(Some(path))
            }
            FileWorkflowEventResult::Renamed { .. } | FileWorkflowEventResult::AutoSaved { .. } => {
                Ok(None)
            }
        }
    }

    pub fn try_rename_in_edit(
        &self,
        singleline_value: &str,
        now_local: DateTime<Local>,
    ) -> io::Result<Option<PathBuf>> {
        let current_path = {
            let state = self
                .inner
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if state.state != SinglelineFileState::Edit {
                return Ok(None);
            }
            let Some(path) = state.current_edit_path.clone() else {
                return Ok(None);
            };
            path
        };

        let result = self
            .dispatcher
            .dispatch_blocking(FileWorkflowEvent::Rename(RenameFileRequest {
                current_path,
                singleline_value: singleline_value.to_string(),
                now: now_local,
            }))?;

        match result {
            FileWorkflowEventResult::Renamed { path } => {
                let mut state = self
                    .inner
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                state.current_edit_path = Some(path.clone());
                Ok(Some(path))
            }
            FileWorkflowEventResult::Created { .. } | FileWorkflowEventResult::AutoSaved { .. } => {
                Ok(None)
            }
        }
    }

    pub fn try_autosave_in_edit(&self, payload: EditorAutoSavePayload) -> io::Result<bool> {
        {
            let state = self
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
        }

        let result = self
            .dispatcher
            .dispatch_blocking(FileWorkflowEvent::AutoSave(AutoSaveFileRequest {
                payload: payload.clone(),
            }))?;

        match result {
            FileWorkflowEventResult::AutoSaved { .. } => Ok(true),
            FileWorkflowEventResult::Created { .. } | FileWorkflowEventResult::Renamed { .. } => {
                Ok(false)
            }
        }
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

fn path_stem(path: &Path) -> Option<String> {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .map(ToString::to_string)
}

pub fn forced_singleline_stem_after_create(
    singleline_value: &str,
    created_path: &Path,
    now: DateTime<Local>,
) -> Option<String> {
    let resolved_stem = path_stem(created_path)?;
    let base_stem = stem_from_singleline_value(singleline_value, now);
    let had_collision = resolved_stem != base_stem;
    let had_invalid_chars =
        !singleline_value.is_empty() && singleline_value.chars().any(invalid_filename_char);

    if had_collision || had_invalid_chars {
        return Some(resolved_stem);
    }

    None
}

pub fn forced_singleline_stem_after_rename(
    singleline_value: &str,
    renamed_path: &Path,
    now: DateTime<Local>,
) -> Option<String> {
    let resolved_stem = path_stem(renamed_path)?;
    let base_stem = stem_from_singleline_value(singleline_value, now);
    let had_collision = resolved_stem != base_stem;
    let had_invalid_chars =
        !singleline_value.is_empty() && singleline_value.chars().any(invalid_filename_char);

    if had_collision || had_invalid_chars {
        return Some(resolved_stem);
    }

    None
}

fn resolve_unique_txt_path(dir: &Path, stem: &str, exclude_path: Option<&Path>) -> PathBuf {
    let mut suffix = 1usize;
    loop {
        let file_name = if suffix == 1 {
            format!("{stem}.txt")
        } else {
            format!("{stem}_{suffix}.txt")
        };
        let candidate = dir.join(file_name);

        if exclude_path.is_some_and(|path| path == candidate) {
            return candidate;
        }
        if !candidate.exists() {
            return candidate;
        }

        suffix += 1;
    }
}

pub fn create_new_text_file(request: &CreateFileRequest) -> io::Result<PathBuf> {
    let dir = daily_directory(request.user_document_dir.as_path(), request.now);
    fs::create_dir_all(&dir)?;

    let stem = stem_from_singleline_value(&request.singleline_value, request.now);
    let path = resolve_unique_txt_path(dir.as_path(), &stem, None);

    fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&path)?;

    Ok(path)
}

pub fn rename_text_file(request: &RenameFileRequest) -> io::Result<PathBuf> {
    if !request.current_path.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "current editing file does not exist",
        ));
    }

    let parent = request.current_path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "current editing file path has no parent directory",
        )
    })?;

    let stem = stem_from_singleline_value(&request.singleline_value, request.now);
    let target = resolve_unique_txt_path(parent, &stem, Some(&request.current_path));

    if target == request.current_path {
        return Ok(target);
    }

    fs::rename(&request.current_path, &target)?;
    Ok(target)
}

fn save_editor_text_payload_atomic(payload: &EditorAutoSavePayload) -> io::Result<PathBuf> {
    // Keep a serde round-trip in event handling to satisfy req-aus4 payload serialization contract,
    // while persisting raw editor text as the file content.
    let serialized = serde_json::to_vec(payload)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
    let decoded: EditorAutoSavePayload = serde_json::from_slice(&serialized)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;

    write_editor_text_atomic(
        decoded.current_path.as_path(),
        decoded.editor_text.as_bytes(),
    )?;
    Ok(decoded.current_path)
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
        assert_eq!(workflow.state(), SinglelineFileState::New);
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
            .try_rename_in_edit("next", fixed_now())
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
            .try_rename_in_edit("next", fixed_now())
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
            .try_rename_in_edit("こんにちは 世界", fixed_now())
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
            .try_rename_in_edit("renamed", fixed_now())
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
            .try_rename_in_edit("same", fixed_now())
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
    fn newf_test25_collision_forces_singleline_buffer_stem_update() {
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

        let forced = forced_singleline_stem_after_create("filename", second.as_path(), now)
            .expect("forced stem");
        assert_eq!(forced, "filename_2");
        remove_temp_root(root.as_path());
    }

    #[test]
    fn newf_test26_sanitization_forces_singleline_buffer_stem_update() {
        let root = new_temp_root("newf_test26");
        let now = fixed_now();
        let created = create_new_text_file(&CreateFileRequest {
            user_document_dir: root.clone(),
            singleline_value: "file:name".to_string(),
            now,
        })
        .expect("create sanitized file");

        let forced = forced_singleline_stem_after_create("file:name", created.as_path(), now)
            .expect("forced stem");
        assert_eq!(forced, "file_name");
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
            current_path: path.clone(),
            editor_text: "line-1\nline-2".to_string(),
        };

        let saved = workflow
            .try_autosave_in_edit(payload)
            .expect("dispatch autosave");
        assert!(saved);
        let content = fs::read_to_string(&path).expect("read autosaved file");
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
}
