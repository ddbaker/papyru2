use std::path::PathBuf;

use gpui::*;
use gpui_component::{
    ActiveTheme,
    input::{Input, InputState},
};

pub struct Papyru2Editor {
    input_state: Entity<InputState>,
}

impl Papyru2Editor {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input_state = cx.new(|cx| {
            InputState::new(window, cx)
                .code_editor("rust")
                .line_number(true)
                .soft_wrap(false)
                .placeholder("Test area (integrated editor)")
        });

        Self { input_state }
    }

    pub fn open_file(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let content = match std::fs::read_to_string(&path) {
            Ok(content) => content,
            Err(_) => return,
        };

        let language = path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("txt")
            .to_string();

        self.input_state.update(cx, |state, cx| {
            state.set_highlighter(language, cx);
            state.set_value(content, window, cx);
        });
    }
}

impl Render for Papyru2Editor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        Input::new(&self.input_state)
            .h_full()
            .font_family(cx.theme().mono_font_family.clone())
            .text_size(cx.theme().mono_font_size)
    }
}
