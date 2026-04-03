use std::{
    io,
    path::{Path, PathBuf},
    sync::mpsc::{self, RecvTimeoutError},
    thread,
    time::{Duration, Instant},
};

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use smol::channel::{self, Receiver};

const FILE_TREE_WATCH_DEBOUNCE: Duration = Duration::from_millis(200);

pub struct FileTreeWatcher {
    watcher: Option<RecommendedWatcher>,
    worker: Option<thread::JoinHandle<()>>,
}

impl Drop for FileTreeWatcher {
    fn drop(&mut self) {
        self.watcher.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

pub fn start_file_tree_watcher(root_dir: PathBuf) -> io::Result<(FileTreeWatcher, Receiver<()>)> {
    let (refresh_tx, refresh_rx) = channel::unbounded::<()>();
    let (event_tx, event_rx) = mpsc::channel::<notify::Result<Event>>();

    let mut watcher = notify::recommended_watcher(move |result| {
        let _ = event_tx.send(result);
    })
    .map_err(notify_error_to_io)?;

    watcher
        .watch(root_dir.as_path(), RecursiveMode::Recursive)
        .map_err(notify_error_to_io)?;
    crate::app::trace_debug(format!(
        "file_tree watcher started root_dir={} debounce_ms={}",
        root_dir.display(),
        FILE_TREE_WATCH_DEBOUNCE.as_millis()
    ));

    let worker = thread::spawn({
        let root_dir = root_dir.clone();
        move || watcher_loop(root_dir, event_rx, refresh_tx)
    });

    Ok((
        FileTreeWatcher {
            watcher: Some(watcher),
            worker: Some(worker),
        },
        refresh_rx,
    ))
}

fn watcher_loop(
    root_dir: PathBuf,
    event_rx: mpsc::Receiver<notify::Result<Event>>,
    refresh_tx: channel::Sender<()>,
) {
    let mut pending_deadline: Option<Instant> = None;

    loop {
        let wait_for_event = pending_deadline.map(|deadline| {
            deadline
                .checked_duration_since(Instant::now())
                .unwrap_or_else(|| Duration::from_millis(0))
        });

        let next_event = match wait_for_event {
            Some(wait) => match event_rx.recv_timeout(wait) {
                Ok(event) => Some(event),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => break,
            },
            None => match event_rx.recv() {
                Ok(event) => Some(event),
                Err(_) => break,
            },
        };

        if let Some(event_result) = next_event {
            match event_result {
                Ok(event) => {
                    if should_schedule_refresh(root_dir.as_path(), &event) {
                        let first_path = event
                            .paths
                            .first()
                            .map(|path| path.display().to_string())
                            .unwrap_or_else(|| "<none>".to_string());
                        crate::app::trace_debug(format!(
                            "file_tree watcher event accepted kind={:?} path_count={} first_path={}",
                            event.kind,
                            event.paths.len(),
                            first_path
                        ));
                        pending_deadline = Some(Instant::now() + FILE_TREE_WATCH_DEBOUNCE);
                    }
                }
                Err(error) => {
                    crate::app::trace_debug(format!("file_tree watcher event error={error}"));
                }
            }
            continue;
        }

        if pending_deadline.take().is_none() {
            continue;
        }

        crate::app::trace_debug("file_tree watcher debounce flush");
        if refresh_tx.send_blocking(()).is_err() {
            break;
        }
    }

    crate::app::trace_debug("file_tree watcher loop stopped");
}

pub(crate) fn should_schedule_refresh(root_dir: &Path, event: &Event) -> bool {
    if !event_kind_requires_refresh(&event.kind) {
        return false;
    }

    event
        .paths
        .iter()
        .any(|path| path_is_under_root(root_dir, path.as_path()))
}

fn event_kind_requires_refresh(kind: &EventKind) -> bool {
    matches!(
        kind,
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
    )
}

fn path_is_under_root(root_dir: &Path, path: &Path) -> bool {
    if path.starts_with(root_dir) {
        return true;
    }

    #[cfg(target_os = "windows")]
    {
        let root = root_dir.to_string_lossy().to_ascii_lowercase();
        let target = path.to_string_lossy().to_ascii_lowercase();
        return target.starts_with(root.as_str());
    }

    #[cfg(not(target_os = "windows"))]
    {
        false
    }
}

#[cfg(test)]
pub(crate) fn coalesced_refresh_count(event_offsets_ms: &[u64], debounce_ms: u64) -> usize {
    if event_offsets_ms.is_empty() {
        return 0;
    }

    let mut flush_count = 0usize;
    let mut pending_deadline = event_offsets_ms[0].saturating_add(debounce_ms);
    for offset in event_offsets_ms.iter().skip(1) {
        if *offset >= pending_deadline {
            flush_count += 1;
        }
        pending_deadline = offset.saturating_add(debounce_ms);
    }

    flush_count + 1
}

fn notify_error_to_io(error: notify::Error) -> io::Error {
    io::Error::other(format!("file_tree watcher error: {error}"))
}

#[cfg(test)]
mod tests {
    use super::{coalesced_refresh_count, should_schedule_refresh};
    use notify::{
        Event, EventKind,
        event::{AccessKind, CreateKind, DataChange, ModifyKind, RemoveKind, RenameMode},
    };
    use std::path::PathBuf;

    fn event(kind: EventKind, paths: Vec<PathBuf>) -> Event {
        Event {
            kind,
            paths,
            attrs: Default::default(),
        }
    }

    #[test]
    fn ftr_test27_req_ftr12_watcher_filter_accepts_core_fs_events_under_root() {
        let root = PathBuf::from("C:/tmp/user_document");
        let child = root.join("2026/03/08/note.txt");

        let create = event(EventKind::Create(CreateKind::File), vec![child.clone()]);
        let modify = event(
            EventKind::Modify(ModifyKind::Data(DataChange::Content)),
            vec![child.clone()],
        );
        let rename = event(
            EventKind::Modify(ModifyKind::Name(RenameMode::Both)),
            vec![child.clone(), root.join("2026/03/08/note2.txt")],
        );
        let remove = event(EventKind::Remove(RemoveKind::File), vec![child]);

        assert!(should_schedule_refresh(root.as_path(), &create));
        assert!(should_schedule_refresh(root.as_path(), &modify));
        assert!(should_schedule_refresh(root.as_path(), &rename));
        assert!(should_schedule_refresh(root.as_path(), &remove));
    }

    #[test]
    fn ftr_test28_req_ftr12_watcher_filter_ignores_outside_root_and_access_only_events() {
        let root = PathBuf::from("C:/tmp/user_document");
        let outside = PathBuf::from("C:/tmp/other/place.txt");
        let inside = root.join("2026/03/08/note.txt");

        let outside_create = event(EventKind::Create(CreateKind::File), vec![outside]);
        let inside_access = event(EventKind::Access(AccessKind::Read), vec![inside]);

        assert!(!should_schedule_refresh(root.as_path(), &outside_create));
        assert!(!should_schedule_refresh(root.as_path(), &inside_access));
    }

    #[test]
    fn ftr_test29_req_ftr12_debounce_coalesces_event_bursts() {
        assert_eq!(coalesced_refresh_count(&[0, 20, 40, 55], 200), 1);
        assert_eq!(coalesced_refresh_count(&[0, 250, 600], 200), 3);
        assert_eq!(coalesced_refresh_count(&[], 200), 0);
    }


    #[test]
    fn ftr_test34_req_qsrv4_follow_watcher_filter_accepts_metadata_write_time_modify_event() {
        let root = PathBuf::from("C:/tmp/user_document");
        let child = root.join("2026/04/03/fileA.txt");
        let metadata_modify = event(
            EventKind::Modify(ModifyKind::Metadata(notify::event::MetadataKind::WriteTime)),
            vec![child],
        );

        assert!(should_schedule_refresh(root.as_path(), &metadata_modify));
    }
}
