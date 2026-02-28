use std::{
    collections::VecDeque,
    fs,
    io,
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex, mpsc},
    thread,
    time::{Duration, Instant},
};

use chrono::{DateTime, Local};

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

#[derive(Debug, Clone)]
pub enum FileWorkflowEvent {
    Create(CreateFileRequest),
    Rename(RenameFileRequest),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileWorkflowEventResult {
    Created { path: PathBuf },
    Renamed { path: PathBuf },
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

    pub fn dispatch_blocking(&self, event: FileWorkflowEvent) -> io::Result<FileWorkflowEventResult> {
        let (response_tx, response_rx) = mpsc::channel::<io::Result<FileWorkflowEventResult>>();
        {
            let (lock, wakeup) = &*self.shared;
            let mut state = lock.lock().map_err(|_| {
                io::Error::other("singleline_create_file event queue lock poisoned on enqueue")
            })?;
            state.queue.push_back(EventEnvelope { event, response_tx });
            wakeup.notify_one();
        }

        response_rx.recv().map_err(|_| {
            io::Error::new(
                io::ErrorKind::BrokenPipe,
                "singleline_create_file worker terminated before sending response",
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
        let state = self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
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
        let mut state = self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        state.state = SinglelineFileState::Neutral;
        state.current_edit_path = None;
    }

    pub fn set_edit_from_open_file(&self, path: PathBuf) {
        let mut state = self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        state.state = SinglelineFileState::Edit;
        state.current_edit_path = Some(path);
    }

    pub fn transition_edit_to_neutral(&self) -> bool {
        let mut state = self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
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
            let mut state = self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
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

        let result = self.dispatcher.dispatch_blocking(FileWorkflowEvent::Create(CreateFileRequest {
            user_document_dir: user_document_dir.to_path_buf(),
            singleline_value: singleline_value.to_string(),
            now: now_local,
        }))?;

        match result {
            FileWorkflowEventResult::Created { path } => {
                let mut state = self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                state.state = SinglelineFileState::Edit;
                state.current_edit_path = Some(path.clone());
                Ok(Some(path))
            }
            FileWorkflowEventResult::Renamed { .. } => Ok(None),
        }
    }

    pub fn try_rename_in_edit(
        &self,
        singleline_value: &str,
        now_local: DateTime<Local>,
    ) -> io::Result<Option<PathBuf>> {
        let current_path = {
            let state = self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            if state.state != SinglelineFileState::Edit {
                return Ok(None);
            }
            let Some(path) = state.current_edit_path.clone() else {
                return Ok(None);
            };
            path
        };

        let result = self.dispatcher.dispatch_blocking(FileWorkflowEvent::Rename(RenameFileRequest {
            current_path,
            singleline_value: singleline_value.to_string(),
            now: now_local,
        }))?;

        match result {
            FileWorkflowEventResult::Renamed { path } => {
                let mut state = self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                state.current_edit_path = Some(path.clone());
                Ok(Some(path))
            }
            FileWorkflowEventResult::Created { .. } => Ok(None),
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
    let dir = daily_directory(&request.user_document_dir, request.now);
    fs::create_dir_all(&dir)?;

    let stem = stem_from_singleline_value(&request.singleline_value, request.now);
    let path = resolve_unique_txt_path(&dir, &stem, None);

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
            .try_create_from_neutral("hello", &root, Instant::now(), fixed_now())
            .expect("create from neutral");

        assert!(created.is_some());
        assert_eq!(workflow.state(), SinglelineFileState::Edit);
        assert!(workflow.current_edit_path().is_some());
        workflow.dispatcher.shutdown();
        remove_temp_root(&root);
    }

    #[test]
    fn newf_test4_create_event_throttle_blocks_second_raise_within_one_second() {
        let root = new_temp_root("newf_test4");
        let workflow = SinglelineCreateFileWorkflow::new();
        let now = Instant::now();

        let first = workflow
            .try_create_from_neutral("hello", &root, now, fixed_now())
            .expect("first create");
        assert!(first.is_some());

        let transitioned = workflow.transition_edit_to_neutral();
        assert!(transitioned);

        let second = workflow
            .try_create_from_neutral(
                "world",
                &root,
                now + Duration::from_millis(500),
                fixed_now(),
            )
            .expect("second create");
        assert!(second.is_none());
        assert_eq!(workflow.state(), SinglelineFileState::New);
        workflow.dispatcher.shutdown();
        remove_temp_root(&root);
    }

    #[test]
    fn newf_test5_edit_plus_transition_resets_to_neutral() {
        let root = new_temp_root("newf_test5");
        let workflow = SinglelineCreateFileWorkflow::new();
        workflow
            .try_create_from_neutral("hello", &root, Instant::now(), fixed_now())
            .expect("create");
        assert!(workflow.transition_edit_to_neutral());
        assert_eq!(workflow.state(), SinglelineFileState::Neutral);
        assert_eq!(workflow.current_edit_path(), None);
        workflow.dispatcher.shutdown();
        remove_temp_root(&root);
    }

    #[test]
    fn newf_test6_plus_noop_in_neutral_and_new() {
        let root = new_temp_root("newf_test6");
        let workflow = SinglelineCreateFileWorkflow::new();
        assert!(!workflow.transition_edit_to_neutral());

        let now = Instant::now();
        workflow
            .try_create_from_neutral("hello", &root, now, fixed_now())
            .expect("create");
        assert!(workflow.transition_edit_to_neutral());
        let blocked = workflow
            .try_create_from_neutral("x", &root, now + Duration::from_millis(100), fixed_now())
            .expect("create blocked");
        assert!(blocked.is_none());
        assert!(!workflow.transition_edit_to_neutral());
        workflow.dispatcher.shutdown();
        remove_temp_root(&root);
    }

    #[test]
    fn newf_test7_notitle_format_used_for_empty_buffer() {
        let stem = stem_from_singleline_value("", fixed_now());
        assert!(stem.starts_with("notitle-"));
        assert_eq!(stem.len(), "notitle-".len() + 17);
        assert!(stem["notitle-".len()..]
            .chars()
            .all(|ch| ch.is_ascii_digit()));
    }

    #[test]
    fn newf_test8_daily_directory_uses_yyyy_mm_dd() {
        let root = PathBuf::from("C:/tmp/root");
        let dir = daily_directory(&root, fixed_now());
        assert!(dir.ends_with(Path::new("2026").join("02").join("28")));
    }

    #[test]
    fn newf_test9_collision_suffix_appends_before_txt() {
        let root = new_temp_root("newf_test9");
        let dir = daily_directory(&root, fixed_now());
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
        remove_temp_root(&root);
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
        let stem = stem_from_singleline_value(&source, fixed_now());
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
            .try_create_from_neutral("start", &root, Instant::now(), fixed_now())
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
        remove_temp_root(&root);
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
            .try_create_from_neutral("hello", &root, Instant::now(), fixed_now())
            .expect("create 1");
        let second = workflow
            .try_create_from_neutral(
                "world",
                &root,
                Instant::now() + Duration::from_secs(2),
                fixed_now(),
            )
            .expect("create 2");
        assert!(second.is_none());
        workflow.dispatcher.shutdown();
        remove_temp_root(&root);
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
        remove_temp_root(&root);
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

        let created_dir = daily_directory(&root, fixed_now());
        let count = fs::read_dir(created_dir).expect("read dir").count();
        assert_eq!(count, 4);
        dispatcher.shutdown();
        remove_temp_root(&root);
    }

    #[test]
    fn newf_test20_create_and_rename_support_multibyte_text() {
        let root = new_temp_root("newf_test20");
        let workflow = SinglelineCreateFileWorkflow::new();
        workflow
            .try_create_from_neutral("こんにちは", &root, Instant::now(), fixed_now())
            .expect("create");

        let renamed = workflow
            .try_rename_in_edit("こんにちは 世界", fixed_now())
            .expect("rename")
            .expect("renamed path");
        assert!(renamed
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name == "こんにちは 世界.txt"));
        workflow.dispatcher.shutdown();
        remove_temp_root(&root);
    }

    #[test]
    fn newf_test21_rename_collision_uses_suffix() {
        let root = new_temp_root("newf_test21");
        let workflow = SinglelineCreateFileWorkflow::new();
        let created = workflow
            .try_create_from_neutral("base", &root, Instant::now(), fixed_now())
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
        remove_temp_root(&root);
    }

    #[test]
    fn newf_test22_rename_to_same_name_is_noop() {
        let root = new_temp_root("newf_test22");
        let workflow = SinglelineCreateFileWorkflow::new();
        let created = workflow
            .try_create_from_neutral("same", &root, Instant::now(), fixed_now())
            .expect("create")
            .expect("path");

        let renamed = workflow
            .try_rename_in_edit("same", fixed_now())
            .expect("rename")
            .expect("path");
        assert_eq!(created, renamed);
        workflow.dispatcher.shutdown();
        remove_temp_root(&root);
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

        let daily = daily_directory(&root, fixed_now());
        assert!(path.starts_with(daily));
        remove_temp_root(&root);
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

        let forced =
            forced_singleline_stem_after_create("filename", &second, now).expect("forced stem");
        assert_eq!(forced, "filename_2");
        remove_temp_root(&root);
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

        let forced =
            forced_singleline_stem_after_create("file:name", &created, now).expect("forced stem");
        assert_eq!(forced, "file_name");
        remove_temp_root(&root);
    }
}
