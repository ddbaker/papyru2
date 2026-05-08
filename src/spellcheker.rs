use std::rc::Rc;

use gpui::*;
use gpui_component::{
    highlighter::{Diagnostic, DiagnosticSeverity},
    input::{CodeActionProvider, InputState, RopeExt, ToggleCodeActions},
};

pub(crate) type SpellCheckerController = crate::spellchecker::SpellCheckerController;
pub(crate) type EditorStore = crate::spellchecker::SpellCheckerEditorStore;

pub(crate) fn spawn_app_spellchecker(
    app_paths: crate::path_resolver::AppPaths,
    cx: &mut Context<crate::app::Papyru2App>,
) -> SpellCheckerController {
    let (event_tx, event_rx) = smol::channel::unbounded::<crate::spellchecker::SpellCheckEvent>();
    let spellchecker = crate::spellchecker::SpellCheckerController::new(app_paths, event_tx);

    cx.spawn(async move |this, cx| {
        while let Ok(event) = event_rx.recv().await {
            let Some(this) = this.upgrade() else {
                break;
            };
            let _ = this.update(cx, move |app, cx| app.apply_spellchecker_event(event, cx));
        }
        crate::log::trace_debug("spellchecker ui bridge loop detached");
    })
    .detach();

    spellchecker
}

pub(crate) fn new_editor_store() -> EditorStore {
    EditorStore::new()
}

pub(crate) fn editor_code_action_provider(store: &EditorStore) -> Rc<dyn CodeActionProvider> {
    store.provider()
}

pub(crate) fn attach_editor_code_action_provider(
    mut input_state: InputState,
    provider: Rc<dyn CodeActionProvider>,
) -> InputState {
    input_state.lsp.code_action_providers.push(provider);
    input_state
}

impl crate::editor::Papyru2Editor {
    pub(crate) fn on_spellchecker_editor_mouse_down(
        &mut self,
        _: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.req_assoc18_editor_input_guard_active {
            crate::log::trace_debug("assoc req-assoc18 editor_click_ignored state=NEUTRAL");
        }

        cx.propagate();
    }

    pub(crate) fn on_spellchecker_editor_mouse_up(
        &mut self,
        _: &MouseUpEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.req_assoc18_editor_input_guard_active {
            cx.propagate();
            return;
        }

        cx.on_next_frame(window, move |this, window, cx| {
            if this.req_assoc18_editor_input_guard_active {
                return;
            }

            let cursor = this.input_state.read(cx).cursor_position();
            crate::log::trace_debug(format!(
                "spellchecker code_action toggle requested source=editor_left_click cursor=({}, {})",
                cursor.line, cursor.character
            ));
            window.dispatch_action(Box::new(ToggleCodeActions), cx);
        });

        cx.propagate();
    }

    pub(crate) fn apply_spellchecker_diagnostics(
        &mut self,
        diagnostics: Vec<crate::spellchecker::SpellCheckDiagnostic>,
        cx: &mut Context<Self>,
    ) {
        let store = self.spellchecker_store.clone();
        self.input_state.update(cx, move |state, cx| {
            let text = state.text().clone();
            let text_value = state.value().to_string();
            let mut editor_diagnostics = Vec::new();
            let mut code_actions = Vec::new();
            let mut latest_version = 0;

            for diagnostic in diagnostics {
                latest_version = latest_version.max(diagnostic.version);
                let start =
                    crate::spellchecker_ranges::input_position_from_lsp(diagnostic.range.start);
                let end = crate::spellchecker_ranges::input_position_from_lsp(diagnostic.range.end);
                let severity = spellchecker_diagnostic_severity(diagnostic.severity);
                editor_diagnostics.push(
                    Diagnostic::new(start..end, diagnostic.message.clone()).with_severity(severity),
                );

                let action_range = crate::spellchecker_ranges::lsp_range_to_byte_range(
                    text_value.as_str(),
                    &diagnostic.range,
                )
                .unwrap_or_else(|| text.position_to_offset(&start)..text.position_to_offset(&end));
                for suggestion in &diagnostic.suggestions {
                    code_actions.push((
                        action_range.clone(),
                        crate::spellchecker::code_action_for_suggestion(suggestion),
                    ));
                }
            }

            let diagnostic_count = editor_diagnostics.len();
            state.diagnostics_mut().map(|set| {
                set.clear();
                set.extend(editor_diagnostics);
            });
            store.update_code_actions(code_actions);
            crate::log::trace_debug(format!(
                "spellchecker editor diagnostics applied version={} count={}",
                latest_version, diagnostic_count
            ));
            cx.notify();
        });
    }

    pub(crate) fn clear_spellchecker_diagnostics(&mut self, cx: &mut Context<Self>) {
        let store = self.spellchecker_store.clone();
        self.input_state.update(cx, move |state, cx| {
            state.diagnostics_mut().map(|set| set.clear());
            store.update_code_actions(Vec::new());
            crate::log::trace_debug("spellchecker editor diagnostics cleared");
            cx.notify();
        });
    }
}

fn spellchecker_diagnostic_severity(
    severity: Option<lsp_types::DiagnosticSeverity>,
) -> DiagnosticSeverity {
    match severity {
        Some(severity) if severity == lsp_types::DiagnosticSeverity::ERROR => {
            DiagnosticSeverity::Warning
        }
        Some(severity) if severity == lsp_types::DiagnosticSeverity::WARNING => {
            DiagnosticSeverity::Warning
        }
        Some(severity) if severity == lsp_types::DiagnosticSeverity::INFORMATION => {
            DiagnosticSeverity::Info
        }
        _ => DiagnosticSeverity::Hint,
    }
}
