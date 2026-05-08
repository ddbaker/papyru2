use std::{
    ops::Range,
    path::PathBuf,
    rc::Rc,
    str::FromStr,
    sync::{Arc, RwLock},
    time::Duration,
};

use gpui::*;
use gpui_component::input::{CodeActionProvider, InputState};
use lsp_types::{CodeAction, CodeActionKind, TextEdit, WorkspaceEdit};

use crate::spellchecker_lsp::{self, SpellCheckerWorkerHandle, start_harper_worker};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum SpellCheckStatus {
    Off,
    Starting,
    Ready,
    Checking,
    Error(String),
}

#[derive(Clone, Debug)]
pub(crate) struct SpellCheckDocument {
    pub(crate) uri: String,
    pub(crate) version: i32,
    pub(crate) language_id: String,
    pub(crate) text: String,
}

#[derive(Clone, Debug)]
pub(crate) struct SpellCheckSuggestion {
    pub(crate) label: String,
    pub(crate) replacement_text: String,
    pub(crate) range: lsp_types::Range,
}

#[derive(Clone, Debug)]
pub(crate) struct SpellCheckDiagnostic {
    pub(crate) id: u64,
    pub(crate) version: i32,
    pub(crate) range: lsp_types::Range,
    pub(crate) message: String,
    pub(crate) severity: Option<lsp_types::DiagnosticSeverity>,
    pub(crate) suggestions: Vec<SpellCheckSuggestion>,
}

#[derive(Clone, Debug)]
pub(crate) enum SpellCheckEvent {
    Started {
        generation: u64,
    },
    Diagnostics {
        generation: u64,
        version: Option<i32>,
        diagnostics: Vec<SpellCheckDiagnostic>,
    },
    CodeActions {
        generation: u64,
        version: Option<i32>,
        diagnostic_id: u64,
        suggestions: Vec<SpellCheckSuggestion>,
    },
    Stopped {
        generation: u64,
    },
    Error {
        generation: u64,
        message: String,
    },
}

pub(crate) struct SpellCheckerController {
    enabled: bool,
    status: SpellCheckStatus,
    generation: u64,
    document_version: i32,
    diagnostics: Vec<SpellCheckDiagnostic>,
    worker: Option<SpellCheckerWorkerHandle>,
    app_paths: crate::path_resolver::AppPaths,
    event_tx: smol::channel::Sender<SpellCheckEvent>,
}

impl SpellCheckerController {
    pub(crate) fn new(
        app_paths: crate::path_resolver::AppPaths,
        event_tx: smol::channel::Sender<SpellCheckEvent>,
    ) -> Self {
        Self {
            enabled: false,
            status: SpellCheckStatus::Off,
            generation: 0,
            document_version: 0,
            diagnostics: Vec::new(),
            worker: None,
            app_paths,
            event_tx,
        }
    }

    pub(crate) fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub(crate) fn status(&self) -> &SpellCheckStatus {
        &self.status
    }

    pub(crate) fn diagnostics(&self) -> &[SpellCheckDiagnostic] {
        &self.diagnostics
    }

    pub(crate) fn start(&mut self, current_path: Option<PathBuf>, text: String) {
        if self.enabled {
            return;
        }

        self.generation = self.generation.saturating_add(1);
        self.document_version = 1;
        self.enabled = true;
        self.status = SpellCheckStatus::Starting;
        self.diagnostics.clear();

        let harper_path = spellchecker_lsp::harper_ls_executable_path(&self.app_paths);
        let document = self.document_from_text(current_path, text);
        crate::log::trace_debug(format!(
            "spellchecker start requested path={} version={} text_len={}",
            harper_path.display(),
            document.version,
            document.text.len()
        ));

        match start_harper_worker(
            harper_path,
            self.event_tx.clone(),
            document,
            self.generation,
        ) {
            Ok(worker) => {
                self.worker = Some(worker);
            }
            Err(message) => {
                crate::log::trace_debug(format!("spellchecker error {message}"));
                self.enabled = false;
                self.status = SpellCheckStatus::Error(message);
            }
        }
    }

    pub(crate) fn update_document(&mut self, current_path: Option<PathBuf>, text: String) {
        if !self.enabled {
            return;
        }

        self.document_version = self.document_version.saturating_add(1);
        self.status = SpellCheckStatus::Checking;
        let document = self.document_from_text(current_path, text);
        if let Some(worker) = &self.worker {
            if !worker.send_change(document) {
                self.status = SpellCheckStatus::Error(
                    "spellchecker worker channel closed while sending document".to_string(),
                );
            }
        }
    }

    pub(crate) fn stop(&mut self) {
        if let Some(worker) = self.worker.take() {
            worker.stop();
        }
        self.enabled = false;
        self.status = SpellCheckStatus::Off;
        self.diagnostics.clear();
        self.document_version = 0;
    }

    pub(crate) fn stop_blocking(&mut self, timeout: Duration) {
        if let Some(worker) = self.worker.take() {
            worker.stop_blocking(timeout);
        }
        self.enabled = false;
        self.status = SpellCheckStatus::Off;
        self.diagnostics.clear();
        self.document_version = 0;
    }

    pub(crate) fn apply_event(&mut self, event: SpellCheckEvent) -> bool {
        match event {
            SpellCheckEvent::Started { generation } if generation == self.generation => {
                self.status = SpellCheckStatus::Ready;
                false
            }
            SpellCheckEvent::Diagnostics {
                generation,
                version,
                mut diagnostics,
            } if generation == self.generation
                && diagnostic_event_is_current(version, self.document_version) =>
            {
                let accepted_version = version.unwrap_or(self.document_version);
                self.document_version = accepted_version;
                for diagnostic in &mut diagnostics {
                    diagnostic.version = accepted_version;
                }
                self.status = SpellCheckStatus::Ready;
                self.diagnostics = diagnostics;
                true
            }
            SpellCheckEvent::CodeActions {
                generation,
                version,
                diagnostic_id,
                suggestions,
            } if generation == self.generation
                && code_action_event_is_current(version, self.document_version) =>
            {
                let mut changed = false;
                if let Some(diagnostic) = self
                    .diagnostics
                    .iter_mut()
                    .find(|diagnostic| diagnostic.id == diagnostic_id)
                {
                    diagnostic.suggestions = suggestions;
                    changed = true;
                }
                changed
            }
            SpellCheckEvent::Diagnostics {
                generation,
                version,
                ..
            } if generation == self.generation => {
                crate::log::trace_debug(format!(
                    "spellchecker stale diagnostics ignored event_version={} current_version={}",
                    optional_event_version_for_log(version),
                    self.document_version
                ));
                false
            }
            SpellCheckEvent::CodeActions {
                generation,
                version,
                ..
            } if generation == self.generation => {
                crate::log::trace_debug(format!(
                    "spellchecker stale code_actions ignored event_version={} current_version={}",
                    optional_event_version_for_log(version),
                    self.document_version
                ));
                false
            }
            SpellCheckEvent::Stopped { generation } if generation == self.generation => {
                if self.enabled {
                    self.status = SpellCheckStatus::Error(
                        "harper-ls stopped while spellcheck was enabled".to_string(),
                    );
                }
                false
            }
            SpellCheckEvent::Error {
                generation,
                message,
            } if generation == self.generation => {
                self.enabled = false;
                self.worker = None;
                self.diagnostics.clear();
                self.status = SpellCheckStatus::Error(message);
                true
            }
            _ => false,
        }
    }

    fn document_from_text(
        &self,
        current_path: Option<PathBuf>,
        text: String,
    ) -> SpellCheckDocument {
        let language_id = spellchecker_lsp::language_id_for_path(current_path.as_deref());
        let uri = spellchecker_lsp::file_uri_for_path(current_path.as_deref());
        SpellCheckDocument {
            uri,
            version: self.document_version,
            language_id,
            text,
        }
    }
}

impl Drop for SpellCheckerController {
    fn drop(&mut self) {
        self.stop_blocking(Duration::from_millis(1000));
    }
}

fn diagnostic_event_is_current(event_version: Option<i32>, current_version: i32) -> bool {
    event_version
        .map(|event_version| event_version >= current_version)
        .unwrap_or(true)
}

fn code_action_event_is_current(event_version: Option<i32>, current_version: i32) -> bool {
    event_version
        .map(|event_version| event_version == current_version)
        .unwrap_or(true)
}

fn optional_event_version_for_log(version: Option<i32>) -> String {
    version
        .map(|version| version.to_string())
        .unwrap_or_else(|| "<none>".to_string())
}

#[derive(Clone, Default)]
pub(crate) struct SpellCheckerEditorStore {
    code_actions: Arc<RwLock<Vec<(Range<usize>, CodeAction)>>>,
}

impl SpellCheckerEditorStore {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn provider(&self) -> Rc<Self> {
        Rc::new(self.clone())
    }

    pub(crate) fn update_code_actions(&self, code_actions: Vec<(Range<usize>, CodeAction)>) {
        let mut guard = self
            .code_actions
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = code_actions;
    }

    fn code_actions_for_range(&self, range: Range<usize>) -> Vec<CodeAction> {
        let guard = self
            .code_actions
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard
            .iter()
            .filter(|(action_range, _)| {
                range.start >= action_range.start && range.end <= action_range.end
            })
            .map(|(_, action)| action.clone())
            .collect()
    }
}

impl CodeActionProvider for SpellCheckerEditorStore {
    fn id(&self) -> SharedString {
        "papyru2-spellchecker".into()
    }

    fn code_actions(
        &self,
        _state: Entity<InputState>,
        range: Range<usize>,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Task<Result<Vec<CodeAction>>> {
        Task::ready(Ok(self.code_actions_for_range(range)))
    }

    fn perform_code_action(
        &self,
        state: Entity<InputState>,
        action: CodeAction,
        _push_to_history: bool,
        window: &mut Window,
        cx: &mut App,
    ) -> Task<Result<()>> {
        let Some(edit) = action.edit else {
            return Task::ready(Ok(()));
        };
        let Some(changes) = edit.changes else {
            return Task::ready(Ok(()));
        };
        let Some((_, text_edits)) = changes.into_iter().next() else {
            return Task::ready(Ok(()));
        };

        crate::log::trace_debug(format!(
            "spellchecker apply_edit edit_count={}",
            text_edits.len()
        ));
        let state = state.downgrade();
        window.spawn(cx, async move |cx| {
            state.update_in(cx, |state, window, cx| {
                state.apply_lsp_edits(&text_edits, window, cx);
            })
        })
    }
}

pub(crate) fn code_action_for_suggestion(suggestion: &SpellCheckSuggestion) -> CodeAction {
    let uri = lsp_types::Uri::from_str("file:///papyru2-spellchecker").unwrap();
    CodeAction {
        title: suggestion.label.clone(),
        kind: Some(CodeActionKind::QUICKFIX),
        edit: Some(WorkspaceEdit {
            changes: Some(
                std::iter::once((
                    uri,
                    vec![TextEdit {
                        range: suggestion.range,
                        new_text: suggestion.replacement_text.clone(),
                        ..Default::default()
                    }],
                ))
                .collect(),
            ),
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        SpellCheckDiagnostic, SpellCheckEvent, SpellCheckStatus, SpellCheckerController,
        SpellCheckerEditorStore,
    };
    use crate::path_resolver::{AppPaths, RunEnvPattern};
    use std::path::PathBuf;

    fn test_app_paths() -> AppPaths {
        let home = PathBuf::from("Z:/papyru2-test-home");
        AppPaths {
            mode: RunEnvPattern::DevCargoRun,
            app_home: home.clone(),
            conf_dir: home.join("conf"),
            data_dir: home.join("data"),
            user_document_dir: home.join("data").join("user_document"),
            recyclebin_dir: home.join("data").join("user_document").join("recyclebin"),
            log_dir: home.join("log"),
            bin_dir: home.join("bin"),
        }
    }

    #[test]
    fn spchk_test5_stale_diagnostics_do_not_replace_newer_document_version() {
        let (tx, _rx) = smol::channel::unbounded();
        let mut controller = SpellCheckerController::new(test_app_paths(), tx);
        controller.generation = 1;
        controller.enabled = true;
        controller.document_version = 3;

        let changed = controller.apply_event(SpellCheckEvent::Diagnostics {
            generation: 1,
            version: Some(2),
            diagnostics: vec![diagnostic(2, 0)],
        });

        assert!(!changed);
        assert!(controller.diagnostics().is_empty());
    }

    #[test]
    fn spchk_test5_current_generation_diagnostics_are_accepted() {
        let (tx, _rx) = smol::channel::unbounded();
        let mut controller = SpellCheckerController::new(test_app_paths(), tx);
        controller.generation = 1;
        controller.enabled = true;
        controller.document_version = 2;

        let changed = controller.apply_event(SpellCheckEvent::Diagnostics {
            generation: 1,
            version: Some(2),
            diagnostics: vec![diagnostic(2, 7)],
        });

        assert!(changed);
        assert_eq!(controller.diagnostics().len(), 1);
        assert_eq!(controller.status(), &SpellCheckStatus::Ready);
    }

    #[test]
    fn spchk_test5_versionless_diagnostics_are_accepted_for_current_generation() {
        let (tx, _rx) = smol::channel::unbounded();
        let mut controller = SpellCheckerController::new(test_app_paths(), tx);
        controller.generation = 1;
        controller.enabled = true;
        controller.document_version = 4;

        let changed = controller.apply_event(SpellCheckEvent::Diagnostics {
            generation: 1,
            version: None,
            diagnostics: vec![diagnostic(0, 9)],
        });

        assert!(changed);
        assert_eq!(controller.diagnostics().len(), 1);
        assert_eq!(controller.diagnostics()[0].version, 4);
        assert_eq!(controller.status(), &SpellCheckStatus::Ready);
    }

    #[test]
    fn spchk_test5_versionless_code_actions_update_matching_diagnostic() {
        let (tx, _rx) = smol::channel::unbounded();
        let mut controller = SpellCheckerController::new(test_app_paths(), tx);
        controller.generation = 1;
        controller.enabled = true;
        controller.document_version = 4;
        controller.diagnostics = vec![diagnostic(4, 9)];

        let changed = controller.apply_event(SpellCheckEvent::CodeActions {
            generation: 1,
            version: None,
            diagnostic_id: 9,
            suggestions: vec![crate::spellchecker::SpellCheckSuggestion {
                label: "correct".to_string(),
                replacement_text: "correct".to_string(),
                range: lsp_types::Range::new(
                    lsp_types::Position::new(0, 0),
                    lsp_types::Position::new(0, 4),
                ),
            }],
        });

        assert!(changed);
        assert_eq!(controller.diagnostics()[0].suggestions.len(), 1);
    }

    #[test]
    fn spchk_test3_code_action_store_filters_actions_by_byte_range() {
        let store = SpellCheckerEditorStore::new();
        store.update_code_actions(vec![
            (4..8, lsp_types::CodeAction::default()),
            (10..12, lsp_types::CodeAction::default()),
        ]);

        assert_eq!(store.code_actions_for_range(5..6).len(), 1);
        assert_eq!(store.code_actions_for_range(6..6).len(), 1);
        assert_eq!(store.code_actions_for_range(9..10).len(), 0);
    }

    fn diagnostic(version: i32, id: u64) -> SpellCheckDiagnostic {
        SpellCheckDiagnostic {
            id,
            version,
            range: lsp_types::Range::new(
                lsp_types::Position::new(0, 0),
                lsp_types::Position::new(0, 4),
            ),
            message: "test diagnostic".to_string(),
            severity: Some(lsp_types::DiagnosticSeverity::HINT),
            suggestions: Vec::new(),
        }
    }
}
