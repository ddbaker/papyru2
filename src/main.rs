#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]
mod app;
mod editor;
mod file_tree;
mod file_tree_watcher;
mod file_update_handler;
mod log;
mod quic_rpc;
mod single_instance;
mod singleline_input;
mod sl_editor_association;
mod spellchecker;
mod spellchecker_lsp;
mod spellchecker_ranges;
mod spellchecker_ui;
mod spellcheker;
mod top_bars;
mod window_position;

pub use papyru2::path_resolver;
pub use papyru2::quic_rpc_protocol;

use app::run;

fn main() {
    run();
}
