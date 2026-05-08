use std::time::Duration;

use gpui::{prelude::FluentBuilder as _, *};
use gpui_component::{
    ActiveTheme, IconName, Sizable,
    button::{Button, ButtonVariants as _},
    h_flex,
};

impl crate::app::Papyru2App {
    pub(crate) fn render_spellchecker_bar(
        &self,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let status_text = spellchecker_status_text(
            self.spellchecker.status(),
            self.spellchecker.diagnostics().len(),
        );

        h_flex()
            .justify_between()
            .text_sm()
            .bg(cx.theme().background)
            .py_1()
            .px_3()
            .border_t_1()
            .border_color(cx.theme().border)
            .text_color(cx.theme().muted_foreground)
            .child(
                Button::new("spell-check")
                    .ghost()
                    .xsmall()
                    .when(self.spellchecker.is_enabled(), |this| {
                        this.icon(IconName::Check)
                    })
                    .label("Spell Check")
                    .on_click(cx.listener(Self::toggle_spellchecker)),
            )
            .child(div().child(status_text))
    }

    pub(crate) fn toggle_spellchecker(
        &mut self,
        _: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.spellchecker.is_enabled() {
            self.spellchecker.stop();
            self.editor
                .update(cx, |editor, cx| editor.clear_spellchecker_diagnostics(cx));
            crate::log::trace_debug("spellchecker toggle off");
            cx.notify();
            return;
        }

        let snapshot = self.editor.read(cx).snapshot(cx);
        let current_path = self.editor.read(cx).current_editing_file_path();
        crate::log::trace_debug(format!(
            "spellchecker toggle on text_len={} path={}",
            snapshot.value.len(),
            current_path
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "<none>".to_string())
        ));
        self.spellchecker.start(current_path, snapshot.value);
        if !self.spellchecker.is_enabled() {
            self.editor
                .update(cx, |editor, cx| editor.clear_spellchecker_diagnostics(cx));
        }
        self.apply_spellchecker_diagnostics_to_editor(cx);
        cx.notify();
    }

    pub(crate) fn apply_spellchecker_event(
        &mut self,
        event: crate::spellchecker::SpellCheckEvent,
        cx: &mut Context<Self>,
    ) {
        let changed = self.spellchecker.apply_event(event);
        if changed {
            self.apply_spellchecker_diagnostics_to_editor(cx);
        }
        cx.notify();
    }

    pub(crate) fn on_spellchecker_editor_changed(&mut self, value: String, cx: &mut Context<Self>) {
        if !self.spellchecker.is_enabled() {
            return;
        }
        let current_path = self.editor.read(cx).current_editing_file_path();
        self.spellchecker.update_document(current_path, value);
        cx.notify();
    }

    pub(crate) fn on_spellchecker_document_replaced(&mut self, cx: &mut Context<Self>) {
        if !self.spellchecker.is_enabled() {
            self.editor
                .update(cx, |editor, cx| editor.clear_spellchecker_diagnostics(cx));
            return;
        }
        let snapshot = self.editor.read(cx).snapshot(cx);
        let current_path = self.editor.read(cx).current_editing_file_path();
        self.spellchecker
            .update_document(current_path, snapshot.value);
        self.apply_spellchecker_diagnostics_to_editor(cx);
        cx.notify();
    }

    fn apply_spellchecker_diagnostics_to_editor(&mut self, cx: &mut Context<Self>) {
        if !self.spellchecker.is_enabled() || self.spellchecker.diagnostics().is_empty() {
            self.editor
                .update(cx, |editor, cx| editor.clear_spellchecker_diagnostics(cx));
            return;
        }

        let diagnostics = self.spellchecker.diagnostics().to_vec();
        self.editor.update(cx, |editor, cx| {
            editor.apply_spellchecker_diagnostics(diagnostics, cx);
        });
    }

    pub(crate) fn stop_spellchecker_for_shutdown(&mut self) {
        if self.spellchecker.is_enabled() {
            crate::log::trace_debug("spellchecker window close shutdown");
            self.spellchecker.stop_blocking(Duration::from_millis(1000));
        }
    }
}

fn spellchecker_status_text(
    status: &crate::spellchecker::SpellCheckStatus,
    diagnostic_count: usize,
) -> String {
    match status {
        crate::spellchecker::SpellCheckStatus::Off => "Off".to_string(),
        crate::spellchecker::SpellCheckStatus::Starting => "Starting".to_string(),
        crate::spellchecker::SpellCheckStatus::Checking => "Checking".to_string(),
        crate::spellchecker::SpellCheckStatus::Ready if diagnostic_count == 0 => {
            "Ready".to_string()
        }
        crate::spellchecker::SpellCheckStatus::Ready => {
            format!("{diagnostic_count} issue(s)")
        }
        crate::spellchecker::SpellCheckStatus::Error(message) => {
            format!("Error: {}", crate::app::compact_text(message))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::spellchecker_status_text;
    use crate::spellchecker::SpellCheckStatus;

    #[test]
    fn spchk_ui_test1_default_status_text_is_off() {
        assert_eq!(spellchecker_status_text(&SpellCheckStatus::Off, 0), "Off");
    }

    #[test]
    fn spchk_ui_test2_ready_status_reports_issue_count() {
        assert_eq!(
            spellchecker_status_text(&SpellCheckStatus::Ready, 3),
            "3 issue(s)"
        );
    }
}
