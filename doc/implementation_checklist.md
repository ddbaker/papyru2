# Implementation Checklist

This checklist maps the current project requirements into actionable work items.

Related debug note:
- `doc/req_assoc1_assoc2_debug_postmortem.md` (req-assoc1/req-assoc2 log-assisted debug + fixes)

## Requirement Mapping

- [x] req-editor1: Analyze `editor.rs`
  - Source: `D:\devel\gpui\gpui-component\crates\story\examples\editor.rs`
  - Extract required types, traits, and UI patterns used by the example.
  - Identify integration points needed for this repository.
  - Output: `doc/req_editor1_analysis.md`

- [x] req-editor2: Integrate `editor.rs` and make it work with `singleline_input.rs`
  - Integrated editor behavior using `InputState::code_editor("rust")`.
  - Wired file-open flow from file viewer to editor state.
  - Kept integration compatible with current `gpui` / `gpui-component` APIs.

- [x] req-editor3: Organize main window layout (corrected)
  - Top row: `(round-button1) (round-button2) [singleline_input]`.
  - Main area: split layout where left = file viewer and right = test area.
  - Implemented integrated editor in the right test area panel.

- [x] req-editor4: Round buttons are placeholders
  - Added two round icon buttons in top row.
  - Click handlers are no-op placeholders.

- [x] req-editor5: Build quality gate (corrected)
  - `cargo check` is error free.
  - As corrected requirement states: no need to run `cargo build` or `cargo run`.

## Refactor Requirement Mapping

- [x] req-rf1: Refactor `src/*.rs` to keep `src/main.rs` small/light-weight
  - Source: `doc/papyru2_requirements_refactor.md`
  - Target source layout:
    - `src/main.rs`
    - `src/singleline_input.rs`
    - `src/editor.rs`
  - Moved editor-specific UI/state/render/file-open logic into `src/editor.rs`.
  - Kept `src/main.rs` focused on app composition, file tree, and bootstrap.
  - Wired new module/imports (`mod editor;` and `use editor::Papyru2Editor;`).
  - Verification passed: `cargo check` is error free after refactor.
  - Working baseline noted in requirement: branch `feat/refactor`, commit `5219ad4839bbccb0dcfa79332bdfcd1e2f402887`.

## Refactor Follow-up Mapping

- [x] req-rf2: Keep `src/main.rs` as thin entrypoint (no GUI component `impl` blocks)
  - Source: user clarification in current task thread.
  - Allowed in `src/main.rs`: `mod`, `use`, `fn main()`.
  - Moved out of `src/main.rs`: GUI component/state `impl` blocks (top bar buttons, file viewer tree, editor wiring/composition render).
  - Implemented module split:
    - `src/app.rs`: `Papyru2App` state, render composition, file tree helpers, and app bootstrap `run()`.
    - `src/editor.rs`: editor component state/render.
    - `src/singleline_input.rs`: single-line input component state/render.
  - Verification gates:
    - `cargo check` is error free (passed).
    - `src/main.rs` contains no `impl` blocks (verified).
    - App layout/behavior remains aligned with `req-editor3` and `req-editor4`.

- [x] req-rf3: Reorganize GUI composition into dedicated modules (`file_tree.rs`, `top_bars.rs`)
  - Source: user-approved plan in current task thread.
  - Target source layout:
    - `src/main.rs`
    - `src/editor.rs`
    - `src/app.rs`
    - `src/file_tree.rs`
    - `src/top_bars.rs`
    - `src/singleline_input.rs`
  - Keep `src/singleline_input.rs` as a standalone component file.
  - `src/top_bars.rs` renders two placeholder round buttons and includes `SingleLineInput`.
  - Moved file-tree state/render/helpers from `src/app.rs` into `src/file_tree.rs`.
  - Kept `src/app.rs` focused on app orchestration/composition and bootstrap `run()`.
  - Verification gates:
    - `cargo check` is error free (passed).
    - `src/main.rs` still has no `impl` blocks (verified).
    - Layout behavior remains aligned with existing requirements.

## Layout Spacing Mapping (`req-lo2`, `req-lo3`)

- [x] req-lo2: Put `10px` left-hand-side spacing for `singleline_input`
  - Source: `doc/papyru2_requirements_layout.md` (Date: 2026 March 5th update).
  - Apply `10px` spacing between top-row left block (round buttons) and top-row right block (`singleline_input`).
  - Implemented via shared constant in `src/top_bars.rs`:
    - `SHARED_INTER_PANEL_SPACING_PX = 10.0`
    - top-right panel wrapper `div().pl(px(SHARED_INTER_PANEL_SPACING_PX))`.
  - Keep `req-lo1` invariant: `singleline_input` and editor text-area x-start remain aligned during split-drag and window resize.

- [x] req-lo3: Put `10px` spacing between `file_tree` and `editor`
  - Source: `doc/papyru2_requirements_layout.md` (Date: 2026 March 5th update).
  - Apply `10px` spacing at the boundary between bottom-row left panel (`file_tree`) and right panel (`editor`).
  - Implemented in `src/app.rs` using the same shared constant:
    - bottom-right panel wrapper `div().pl(px(SHARED_INTER_PANEL_SPACING_PX))`.
  - Keep shared split semantics used by `req-lo1` so alignment behavior remains deterministic.

### Layout Spacing Verification

- [x] verify-lo1: `cargo check` passes after layout spacing updates.
- [x] verify-lo1a: Focused unit tests pass for layout spacing constants:
  - `cargo test lo_test -- --nocapture` (passed: `lo_test1`, `lo_test2`).
- [x] verify-lo2: Visual/manual check confirms `singleline_input` has `10px` left-hand-side spacing from the top-row left block.
- [x] verify-lo3: Visual/manual check confirms `file_tree` and `editor` have `10px` spacing in the bottom row.
- [x] verify-lo4: Drag both top and bottom splitters, then resize window; x-start alignment rule from `req-lo1` remains intact.

### Layout Spacing Manual Verification Procedure

- [x] manual-lo1: Launch app with `cargo run`.
- [x] manual-lo2: In the top row, focus `singleline_input` and visually confirm a clear `10px` gap from the left button panel boundary.
- [x] manual-lo3: In the bottom row, focus `editor` and visually confirm a clear `10px` gap from the `file_tree` boundary.
- [x] manual-lo4: Drag top splitter left/right; confirm top spacing remains visible and stable.
- [x] manual-lo5: Drag bottom splitter left/right; confirm bottom spacing remains visible and stable.
- [x] manual-lo6: Resize main window larger/smaller; confirm spacing is preserved and no border overlap reappears.
- [x] manual-lo7: Compare x-start alignment of `singleline_input` text area and editor text area; confirm `req-lo1` remains true after splitter drags and window resize.

## Master Prompt Constraints

- [x] Use Rust only.
- [x] Use Serena MCP for project analysis/symbol editing operations.
- [x] Use Context7 MCP proactively to align with latest `gpui` / `gpui-components` APIs.

## Completion Criteria

- [x] Corrected requirements are reflected.
- [x] `cargo check` passes.
- [x] Association debug postmortem is documented (`doc/req_assoc1_assoc2_debug_postmortem.md`).

## Singleline-Editor Association Mapping

- [x] assoc-core1: Add pure Unicode-safe transfer module (`src/association.rs`)
  - Implement char-boundary-safe split/merge helpers for text movement between singleline input and editor line-1.
  - Keep logic UI-agnostic so it can be unit-tested without GPUI window context.

- [x] assoc-core2: Add component-level read/write hooks
  - `src/singleline_input.rs`: expose value/cursor read-write methods needed by coordinator.
  - `src/editor.rs`: expose line-1/cursor read-write methods and value replacement helpers needed by coordinator.

- [x] assoc-core3: Add app-level association coordinator (`src/app.rs`)
  - Coordinate Enter-from-singleline and Backspace-from-editor-head transfer flows.
  - Keep existing file-open integration behavior unchanged.

- [x] assoc-core4: Wire key-triggered association events
  - `src/top_bars.rs` / `src/singleline_input.rs`: emit event when Enter is pressed in singleline input.
  - `src/editor.rs`: emit event when Backspace is pressed at editor line-1 head.
  - App subscribes and invokes coordinator while preserving normal edit behavior for non-trigger conditions.

- [x] req-assoc1-impl: Enter transfer (ASCII)
  - Given `singleline = abcdef|ghijkl` and `editor line-1 = xyz`, Enter results in:
  - `singleline = abcdef`
  - `editor line-1 = |ghijkl`
  - previous editor line-1 shifts to line-2 (`xyz`).

- [x] req-assoc2-impl: Backspace reverse transfer (ASCII)
  - Given editor head state `|ghijkl` at line-1 and `singleline = abcdef`, Backspace results in:
  - `singleline = abcdef|ghijkl`
  - editor line-1 removed/shifted so next line becomes new line-1.

- [x] req-assoc3-impl: Enter transfer (multi-byte)
  - Same behavior as `req-assoc1-impl` for multi-byte chars (e.g. `こんにち|は世界`).

- [x] req-assoc4-impl: Backspace reverse transfer (multi-byte)
  - Same behavior as `req-assoc2-impl` for multi-byte chars (e.g. `こんにち|は世界`).

- [x] req-assoc5-impl: Down-arrow cursor transfer (ASCII, same-position)
  - Given `singleline = 12345|6789` and `editor line-1 = 123456789`, Down moves focus/cursor to:
  - `editor line-1 = 12345|6789` (same char index transfer).

- [x] req-assoc6-impl: Up-arrow cursor transfer (ASCII, same-position)
  - Given `editor line-1 = 12345|6789` and `singleline = 123456789`, Up moves focus/cursor to:
  - `singleline = 12345|6789` (same char index transfer).

- [x] req-assoc7-impl: Down-arrow cursor transfer (ASCII, clamp-to-tail)
  - Given `singleline = 12345678|9` and `editor line-1 = 123`, Down moves focus/cursor to:
  - `editor line-1 = 123|` (clamped to line-1 tail).

- [x] req-assoc8-impl: Up-arrow cursor transfer (ASCII, clamp-to-tail)
  - Given `editor line-1 = 12345678|9` and `singleline = 123`, Up moves focus/cursor to:
  - `singleline = 123|` (clamped to singleline tail).

- [x] req-assoc9-impl: Multi-byte support for req-assoc5..8
  - Up/Down same-position and clamp semantics are Unicode-safe (char index based, not byte index based).

- [x] req-assoc10-impl: Log-assisted trace coverage for Up/Down
  - Log captures Up/Down keydown and cursor positions at:
  - event source (`singleline`/`editor`) and transfer coordinator (`app`) before/result/after checkpoints.
  - UP-arrow capture fix note:
  - final stable hook uses editor root `.capture_action(MoveUp)` in `src/editor.rs` (not constructor-time `Context::on_action`).
  - this avoids startup panic in `cargo run` and reliably routes `EditorEvent::PressUpAtFirstLine`.

- [x] req-assoc11-impl: Enter at singleline tail inserts empty editor line-1
  - Given `singleline = abcdefg|` and `editor line-1 = xyz`, Enter results in:
  - `singleline = abcdefg`
  - `editor = |` at line-1 and previous `xyz` shifted to line-2.

- [x] req-assoc12-impl: Backspace at empty editor line-1 head returns to singleline tail
  - Given `singleline = abcdefg` and `editor = |\nxyz`, Backspace results in:
  - `singleline = abcdefg|`
  - `editor line-1 = xyz`.

- [x] req-assoc13-impl: Enter at singleline tail with blank editor line-1 moves focus to editor head
  - Given `singleline = abcdefg|` and `editor line-1 = (blank)`, Enter results in:
  - `singleline = abcdefg`
  - `editor line-1` remains blank with cursor at head (`|`).

### Association Tests

- [x] assoc-test1: `req-assoc1` ASCII forward transfer unit test.
- [x] assoc-test2: `req-assoc2` ASCII reverse transfer unit test.
- [x] assoc-test3: `req-assoc3` multi-byte forward transfer unit test.
- [x] assoc-test4: `req-assoc4` multi-byte reverse transfer unit test.
- [x] assoc-test5: Enter with invalid singleline cursor index is no-op transfer.
- [x] assoc-test6: Backspace when editor cursor is not at line-1 head follows normal editor behavior.
- [x] assoc-test7: UTF-8 boundary safety (no byte-index slicing panic).
- [x] assoc-test8: Deterministic focus ownership after transfer (Enter -> editor, reverse -> singleline).
- [x] assoc-test9: Backspace reverse transfer from single editor line appends at singleline end.
- [x] assoc-test10: `req-assoc5` Down-arrow same-position transfer (ASCII).
- [x] assoc-test11: `req-assoc6` Up-arrow same-position transfer (ASCII).
- [x] assoc-test12: `req-assoc7` Down-arrow clamp-to-tail transfer (ASCII).
- [x] assoc-test13: `req-assoc8` Up-arrow clamp-to-tail transfer (ASCII).
- [x] assoc-test14: `req-assoc9` multi-byte Up/Down and clamp behavior.
- [x] assoc-test15: Up-arrow transfer is disabled when editor cursor is not on line-1.
- [x] assoc-test16: Down-arrow to empty editor line clamps cursor to `0`.
- [x] assoc-test17: Up-arrow to empty singleline clamps cursor to `0`.
- [x] assoc-test18: `req-assoc11` Enter at singleline tail inserts empty editor line-1.
- [x] assoc-test19: `req-assoc12` Backspace at empty editor line-1 head returns to singleline tail.
- [x] assoc-test20: `req-assoc13` Enter at singleline tail with blank editor moves focus to editor head.

## Filesystem Path Resolver Mapping

- [x] req-path1: Add path resolver module and domain model (`AppPaths`, `RunEnvPattern`)
  - Source requirements: `doc/papyru2_requirements_filesystem_path_resolver.md`.
  - Advisory inputs: `doc/req_filepath_resolver_analysis.md`.
  - Include fields: `app_home`, `conf_dir`, `data_dir`, `log_dir`, `bin_dir`.

- [x] req-path2: Resolve `APP_HOME` for run-env-pattern-1 (development via `cargo run`)
  - Resolve from current executable directory.
  - Detect `.../target/debug` and `.../target/release` layouts.
  - Set `APP_HOME` to two levels above executable directory.
  - Guard with `Cargo.toml` existence check at resolved root to reduce false positives.

- [x] req-path3: Resolve `APP_HOME` for run-env-pattern-3 (portable)
  - Detect executable under `<APP_HOME>/bin/<exe>`.
  - Resolve `APP_HOME` to one level above executable directory.
  - Prefer explicit marker file `${APP_HOME}/papyru2.portable` for deterministic detection.

- [x] req-path4: Resolve `APP_HOME` for run-env-pattern-2 (installed fallback)
  - Linux/macOS: `${HOME}/.${APP_NAME}`.
  - Windows: `%USERPROFILE%\\.${APP_NAME}`.
  - Keep app name constant normalized to `papyru2`.

- [x] req-path5: Deterministic resolver priority order (single binary)
  - Priority:
  - `PAPYRU2_HOME` explicit override (for test/debug operations).
  - portable detection.
  - development (`cargo run`) detection.
  - installed fallback.
  - Output includes selected mode as `RunEnvPattern`.

- [x] req-path6: Ensure required directories are created under `APP_HOME`
  - Create-if-missing: `conf`, `data`, `log`, `bin`.
  - Preserve existing directories and be idempotent.

- [x] req-path7: Integrate resolver into startup path without breaking GPUI boot flow
  - Keep `gpui_component::init(cx)` and window bootstrap ordering unchanged.
  - Initialize/record resolved paths before runtime features that need filesystem access.

- [x] req-path8: Add trace logging for resolver decisions
  - Log selected mode, resolved `APP_HOME`, and branch reason (env/portable/dev/installed).
  - Keep logs concise and consistent with existing debug tracing style.

- [x] req-path9: Add richer derived file-path helper APIs on top of `AppPaths`
  - Add helpers such as:
  - `config_file_path("app.toml")` -> `${APP_HOME}/conf/app.toml`
  - `log_file_path("papyru2.log")` -> `${APP_HOME}/log/papyru2.log`
  - Keep helpers path-join based (no manual separator handling).

- [x] req-path10: Add manual CLI run-mode override flags
  - Support optional startup flags such as:
  - `--portable`
  - `--installed`
  - Define explicit precedence against existing resolver inputs (env/path heuristics).
  - Keep default behavior unchanged when no override flags are provided.

- [x] req-ptres10: Resolve `user_document_dir` under `data_dir`
  - Source: `doc/papyru2_requirements_filesystem_path_resolver.md` (2026-02-27 update).
  - Add derived path:
  - `user_document_dir = ${APP_HOME}/data/user_document`.
  - Keep join-based path construction (no manual separator handling).

- [x] req-ptres11: Create `user_document_dir` during ensure/create flow
  - When required runtime directories are created, also create `user_document_dir`.
  - Keep creation idempotent and non-destructive.

- [x] req-ptres12: Add focused unit tests for `user_document_dir`
  - Cover resolve behavior and ensure/create behavior for `user_document_dir`.

### Filesystem Path Resolver Tests

- [x] path-test1: Env override (`PAPYRU2_HOME`) takes highest priority.
- [x] path-test2: Portable marker path resolves to parent of `bin`.
- [x] path-test3: Portable detection does not trigger when marker/layout is invalid.
- [x] path-test4: Dev path detection resolves repo root from `target/debug` and `target/release`.
- [x] path-test5: Dev detection rejects lookalike path without `Cargo.toml`.
- [x] path-test6: Installed fallback uses correct per-OS home-based path.
- [x] path-test7: `ensure_dirs` creates `conf/data/log/bin` and is idempotent.
- [x] path-test8: Resolver priority order is deterministic when multiple conditions could match.
- [x] path-test9: `config_file_path("app.toml")` resolves under `conf_dir`.
- [x] path-test10: `log_file_path("papyru2.log")` resolves under `log_dir`.
- [x] path-test11: `--portable` override selects portable mode deterministically.
- [x] path-test12: `--installed` override selects installed mode deterministically.
- [x] path-test14: `user_document_dir` resolves to `${APP_HOME}/data/user_document`.
- [x] path-test15: `ensure_dirs` creates `user_document_dir` and remains idempotent.

## Window Position Persistence Mapping

- [x] req-win1: Persist and restore window state lifecycle
  - Store window state file under config directory.
  - Load on startup.
  - Apply on `WindowOptions` creation.
  - Save again on close (and optional debounced updates).

- [x] req-win2: Cross-platform support baseline
  - Behavior must work on Windows, Linux, and macOS.

- [x] req-win3: Use GPUI-native window APIs for state capture/apply
  - Startup apply via `WindowOptions { window_bounds: Some(...), ..Default::default() }`.
  - Capture on close via `window.window_bounds()`.
  - Preserve mode semantics through `WindowBounds` variants.

- [x] req-win4: Persist structured window state payload
  - Include: `x`, `y`, `width`, `height`.
  - Include window mode: `windowed | maximized | fullscreen`.
  - Include monitor/display identifier.
  - Include last known DPI/scale value.

- [x] req-win5: Use `serde`-serializable state model
  - State read/write model should derive `Serialize`/`Deserialize`.

- [x] req-win6: Store state under `${APP_HOME}/conf`
  - Reuse existing path resolver output (`conf_dir`) from `req-path*`.

- [x] req-win7: Fixed file name for window state
  - `window_position.toml`.

- [x] req-win8: Follow GPUI startup/close flow
  - On startup: load state, build `WindowOptions`, then `open_window`.
  - On close: use close callback, grab `window.window_bounds()`, save file, allow close.

- [x] req-win9: Handle Wayland positioning caveat gracefully
  - Restore size/state everywhere.
  - Restore exact position only where supported.
  - Treat unsupported exact positioning on Wayland as non-fatal behavior.

- [x] req-win10: Save timing policy
  - Implement save-on-quit.
  - Add debounced save strategy (recommended) for move/resize updates.

- [x] req-win11: Avoid restoring minimized state
  - Do not persist minimized state as next-launch window mode.

- [x] req-win12: Validate/clamp persisted position on startup
  - Handle changed monitor layouts and invalid/outdated coordinates.

- [x] req-win13: Off-screen fallback behavior
  - If saved position is off-screen, fallback to centered/default bounds.

- [x] req-win14: Atomic file writes for state persistence
  - Use temp-file + rename strategy to avoid corrupted state files.

- [x] req-win15: First-launch fallback uses 70% of primary display and centered placement
  - If saved window geometry exists, restore persisted state first (existing behavior).
  - If saved geometry does not exist, compute startup size from `primary_display().bounds()` at ~70%.
  - Center computed first-launch bounds before `open_window(...)`.
  - Keep fallback deterministic when primary display is unavailable.

### Window Position Tests

- [x] win-test1: First run without file uses centered/default bounds.
- [x] win-test2: Startup applies previously saved `WindowBounds` payload.
- [x] win-test3: Close callback writes `window_position.toml` under `conf_dir`.
- [x] win-test4: Persisted state round-trips with serde TOML (read/write consistency).
- [x] win-test5: Maximized/fullscreen restore bounds round-trip correctly.
- [x] win-test6: Minimized state is not persisted as startup mode.
- [x] win-test7: Off-screen saved position falls back to centered/default.
- [x] win-test8: Clamp/validation prevents invalid monitor coordinates from breaking startup.
- [x] win-test9: Atomic write path leaves either old valid file or new valid file on failure.
- [x] win-test10: Wayland-unsupported exact positioning is handled gracefully (non-fatal path).
- [x] win-test11: No persisted geometry => first-launch bounds use ~70% of primary display and are centered.

## Singleline Create-File Mapping (`req-newf*`)

- [x] req-newf1: Add explicit singleline file-workflow state model with three states
  - Define state enum with `NEUTRAL`, `NEW`, `EDIT`.
  - Keep state owner at app-level coordinator so all components can observe consistent behavior.

- [x] req-newf2: Make all state transitions atomic and lock-protected
  - Protect state read/write with exclusive lock in transition coordinator.
  - Transition helper must apply guard + side effects as one operation.

- [x] req-newf3: Initialize workflow state to `NEUTRAL` on app startup
  - Apply before user interaction subscriptions are active.

- [x] req-newf4: Initialize cursor/focus to singleline position `0` on startup
  - Ensure `singleline_input` is focused and cursor index is `0` right after launch.

- [x] req-newf5: `NEUTRAL` + keyboard character input transitions to `NEW`
  - Trigger from singleline text-input path.
  - Keep transition idempotent if already in `NEW`/`EDIT`.

- [x] req-newf6: `NEUTRAL` + `DOWN ARROW` transitions to `NEW`
  - Trigger from existing singleline-to-editor down-arrow action flow.

- [x] req-newf7: `NEUTRAL` + focus move singleline -> editor transitions to `NEW`
  - Trigger when focus transfer occurs without file-open action.

- [x] req-newf8: `NEUTRAL` + existing file open from `file_tree` transitions to `EDIT`
  - Preserve existing open-file behavior.
  - Set current editing file path to selected existing file.

- [x] req-newf9: `EDIT` + `+` button transitions to `NEUTRAL`
  - Use top-bar `+` action as transition trigger only in `EDIT`.

- [x] req-newf10: `NEUTRAL` + `+` button is no-op
  - Keep state and buffers unchanged.

- [x] req-newf11: `NEW` + `+` button is no-op
  - Keep state and buffers unchanged.

- [x] req-newf12: `EDIT` -> `NEUTRAL` transition applies required UI reset side effects
  - Clear `singleline_input` buffer.
  - Clear `editor` buffer.
  - Move cursor to `singleline_input` position `0` and focus `singleline_input`.

- [x] req-newf13: `NEUTRAL` -> `NEW` transition triggers filesystem new-file creation
  - Raise/create new-file custom event as part of transition flow.

- [x] req-newf14: On successful new-file creation, transition `NEW` -> `EDIT`
  - Update app state only after file create succeeds.

- [x] req-newf15: Empty singleline buffer uses notitle timestamp filename
  - Filename format: `notitle-YYYYMMDDHHMMSSSSS.txt`.

- [x] req-newf16: New-file target path follows resolver + date directory policy
  - Resolve `user_document_dir` from `path_resolver`.
  - Ensure subdirectory `${user_document_dir}/YYYY/MM/dd` exists.
  - Create file inside that directory.

- [x] req-newf17: Collision handling appends `_N` before `.txt`
  - Apply deterministic probe order: `_2`, `_3`, `_4`, ...

- [x] req-newf18: Non-empty singleline buffer defines new filename
  - Use `"singleline_buffer_value".txt` as base filename.

- [x] req-newf19: Filename generation from singleline buffer supports multibyte characters
  - Keep UTF-8 safe transformations (no byte-splitting).

- [x] req-newf20: Replace invalid filesystem filename characters with `_`
  - Sanitize all invalid filename characters before create/rename operations.

- [x] req-newf21: Trim filename stem to max 64 characters
  - Apply trim before adding `.txt`.

- [x] req-newf22: Track and expose current editing file full-path globally
  - Maintain app-level `currently_under_edit_full_path`.
  - Ensure existing components can reference this path through coordinator/state access.

- [x] req-newf23: In `EDIT`, singleline buffer updates trigger file rename + tracked-path update
  - Rename currently edited file to `updated_buffer_value.txt` (after sanitize/trim policy).
  - Update stored full-path to renamed target path.

- [x] req-newf24: File rename action must be handled as custom event
  - Route rename requests through event queue, not direct UI-thread rename.

- [x] req-newf25: New-file creation action must be handled as custom event
  - Route file-create requests through event queue, not direct UI-thread create.

- [x] req-newf26: Process new-file and rename events in dedicated MPSC/FIFO thread
  - Add dedicated single-consumer worker thread.
  - Queue operations must be FIFO and protected by exclusive lock.

- [x] req-newf27: Enforce event trigger conditions
  - Create event only when state transitions `NEUTRAL` -> `NEW`.
  - Rename event only when state is `EDIT`.
  - Enforce >1 second elapsed since last create-event raise.

- [x] req-newf28: Event raise supports multibyte + IME finalization semantics
  - Raise create/rename only after IME composition is finalized.
  - Confirm multibyte inputs pass through event pipeline unchanged except sanitization policy.

- [x] req-newf29: Add unit tests covering all above requirements
  - Add focused tests for state transitions, filename/path policies, event gating, and worker queue behavior.

- [x] req-newf30: Sync `singleline_input` buffer to resolved filename stem on forced filename changes
  - Source: `doc/papyru2_requirements_singleline_create_file.md` (2026-02-28 update).
  - When create/rename result differs from requested stem due to:
  - collision handling (`req-newf17`) or invalid-character sanitization (`req-newf20`),
  - force-update `singleline_input` buffer to resolved stem (without `.txt` extension).
  - Example: requested `filename` -> created `filename_2.txt` => buffer becomes `filename_2`.

- [x] req-newf31: Add unit tests for `req-newf30`
  - Add focused tests validating buffer sync after collision and sanitization.

- [x] req-newf32: Disable forced `singleline_input` buffer rewrite after create/rename resolution
  - Source: `doc/papyru2_requirements_singleline_create_file.md` (Date: 2026 March 3rd).
  - Remove forced UI rewrite path that applies resolved stem back to `singleline_input` after collision/sanitization.
  - Keep user-typed `singleline_input` value/cursor stable (no programmatic overwrite from resolved filename).
  - Supersedes `req-newf30` UX policy; filesystem resolution logic remains unchanged.

- [x] req-newf33: Keep conflicted filename rename behavior unchanged
  - Keep collision suffix resolution (`_2`, `_3`, ...) in create/rename flows.
  - Keep sanitize/trim filename policy unchanged for filesystem target path generation.
  - Preserve no-overwrite guarantees (`create_new(true)` create path and collision-aware rename retry loop).
  - Preserve current tracked full-path updates and autosave path integrity.

### Singleline Create-File Tests

- [x] newf-test1: Startup state is `NEUTRAL`.
- [x] newf-test2: Startup focus/cursor is `singleline_input` at index `0`.
- [x] newf-test3: `NEUTRAL` + character input transitions to `NEW`.
- [x] newf-test4: `NEUTRAL` + Down-arrow transitions to `NEW`.
- [x] newf-test5: `NEUTRAL` + focus move singleline->editor transitions to `NEW`.
- [x] newf-test6: `NEUTRAL` + file-tree open existing file transitions to `EDIT`.
- [x] newf-test7: `EDIT` + `+` transitions to `NEUTRAL` and applies clear/focus side effects.
- [x] newf-test8: `+` in `NEUTRAL` and `NEW` is no-op.
- [x] newf-test9: Create success transitions `NEW` -> `EDIT`.
- [x] newf-test10: Empty buffer create name matches `notitle-YYYYMMDDHHMMSSSSS.txt` pattern.
- [x] newf-test11: Created file path is under `${APP_HOME}/data/user_document/YYYY/MM/dd`.
- [x] newf-test12: Collision suffix logic uses `_2`, `_3`, ... before `.txt`.
- [x] newf-test13: Non-empty ASCII buffer maps to `<buffer>.txt`.
- [x] newf-test14: Non-empty multibyte buffer maps to `<buffer>.txt` (UTF-8 safe).
- [x] newf-test15: Invalid filename characters are replaced with `_`.
- [x] newf-test16: Filename stem is trimmed to max 64 characters.
- [x] newf-test17: Current editing full-path is set when entering `EDIT`.
- [x] newf-test18: In `EDIT`, buffer update raises rename event and updates tracked full-path.
- [x] newf-test19: Rename action is not raised when state is not `EDIT`.
- [x] newf-test20: Create action is raised only for `NEUTRAL` -> `NEW`.
- [x] newf-test21: Create-event throttle enforces >1 second interval.
- [x] newf-test22: Event queue preserves FIFO ordering for create/rename events.
- [x] newf-test23: Dedicated worker consumes events from MPSC queue safely under concurrent producers.
- [x] newf-test24: IME/multibyte finalize path raises event only after composition commit.
- [x] newf-test25: Replace old collision-force-sync test with inverse expectation:
  - collision-resolved filename updates filesystem path only, and does not force-update `singleline_input` value.
- [x] newf-test26: Replace old sanitization-force-sync test with inverse expectation:
  - sanitized filename updates filesystem path only, and does not force-update `singleline_input` value.
- [x] newf-test31: In `NEUTRAL` create flow, collision suffix is applied on disk and existing target content remains unchanged.
- [x] newf-test32: In `EDIT` rename flow, collision suffix is applied and existing target content remains unchanged (no overwrite).
- [x] newf-test33: Forced singleline stem rewrite remains disabled for rename resolution (collision + sanitization paths).
- [x] newf-test34: Regressions absent for autosave pre-switch flush flows (`req-aus6/7/8`) after req-newf32.

### Singleline Create-File Verification

- [x] verify-newf1: Run focused new-file workflow tests (`cargo test newf_test`).
- [x] verify-newf2: Run existing association tests for regression (`cargo test assoc_test`).
- [x] verify-newf3: Run path resolver tests for regression (`cargo test path_test`).
- [x] verify-newf4: Run full test suite (`cargo test`).
- [x] verify-newf5: Run compile check (`cargo check`).
- [x] verify-newf6: Re-run updated inverse collision test (`cargo test file_update_handler::tests::newf_test25_collision_does_not_force_singleline_buffer_stem_update -- --exact`).
- [x] verify-newf7: Re-run updated inverse sanitization test (`cargo test file_update_handler::tests::newf_test26_sanitization_does_not_force_singleline_buffer_stem_update -- --exact`).
- [x] verify-newf8: Run new req-newf32/33 focused tests (`cargo test file_update_handler::tests::newf_test31_req_newf33_create_collision_keeps_existing_file_and_uses_suffix -- --exact`, `cargo test file_update_handler::tests::newf_test32_req_newf33_rename_collision_preserves_existing_target_content -- --exact`, `cargo test file_update_handler::tests::newf_test33_req_newf32_forced_singleline_stem_is_disabled_for_rename_resolution -- --exact`).
- [x] verify-newf9: Run regression suite including autosave interaction (`cargo test newf_test`, `cargo test aus_test`, `cargo test`, `cargo check`).

## Editor Auto-Save Mapping (`req-aus*`)

- [x] req-aus1: User editor buffer updates trigger autosave for current editing full-path
  - Trigger only from user-originated editor changes (exclude programmatic updates).
  - Save target must be the tracked `currently_under_edit_full_path`.

- [x] req-aus2: Route autosave through existing custom event queue/worker thread
  - Reuse existing MPSC single-consumer FIFO event dispatcher in `file_update_handler`.
  - Queue enqueue/dequeue operations remain lock-protected and atomic.

- [x] req-aus3: Enforce `EDIT` state + valid tracked path invariant on autosave raise
  - Before raising autosave event, validate workflow state is `EDIT`.
  - Validate tracked editing path exists; otherwise treat as critical bug (debug assert + trace).

- [x] req-aus4: Add 6-second non-main-thread timer policy for event raising
  - Maintain pinned-time state and delta-time checks atomically.
  - On user edit: arm pinned-time for current cycle and update latest pending payload.
  - Background timer thread (not UI thread) checks elapsed time.
  - If elapsed >= 6 seconds and typing cycle is armed: raise autosave event and reset cycle.
  - Continued typing must produce repeated 6-second cycles; no typing means no event.

- [x] req-aus5: Persist editor content with atomic file update semantics
  - Use temp-file + replace strategy that preserves last-good target on replace failure.
  - Keep autosave file format as raw editor text while using serde-typed payload in event flow.

- [x] req-aus6: Before `+`-triggered NEW flow from `EDIT` with `delta_time < 6s`, force editor content update event and consume it
  - Add pre-transition forced autosave flush in `plus_button` path (`EDIT -> NEUTRAL`).
  - Use existing file workflow autosave event queue path and block until consumed.
  - Add trace checkpoints for raise/consume/failure.

- [x] req-aus7: Before window-close from `EDIT` with `delta_time < 6s`, force editor content update event and consume it
  - Hook forced autosave flush into `window.on_window_should_close(...)` before close returns.
  - Abort close when pre-close flush fails to avoid silent unsaved-data loss.
  - Add trace checkpoints for pre-close raise/consume/failure.

- [x] req-aus8: Before opening another file from file-tree while `EDIT` and `delta_time < 6s`, force editor content update event and consume it
  - Add pre-open forced autosave flush in `open_file(...)` path.
  - Keep target file open flow unchanged after successful flush.
  - Add trace checkpoints for pre-open raise/consume/failure.

### Editor Auto-Save Tests

- [x] aus-test1: Autosave event writes latest editor text to tracked file path.
- [x] aus-test2: Autosave event is no-op when workflow is not `EDIT`.
- [x] aus-test3: Autosave event is no-op when current edit path is missing.
- [x] aus-test4: 6-second timer gate resets and re-arms in repeated cycles.
- [x] aus-test5: Programmatic editor updates do not arm autosave timer.
- [x] aus-test6: Atomic autosave failure preserves previous valid file content.
- [x] aus-test7: Continuous typing does not postpone save indefinitely and preserves 6-second periodic cadence.
- [x] aus-test11: Req-aus6 pre-NEW forced flush writes latest editor text before `EDIT -> NEUTRAL`.
- [x] aus-test12: Req-aus8 pre-open forced flush writes fileA before switching to fileB.
- [x] aus-test13: Req-aus7 pre-close forced flush writes latest editor text without path transition.

### Editor Auto-Save Verification

- [x] verify-aus1: Run focused autosave tests (`cargo test aus_test`).
- [x] verify-aus2: Run existing new-file workflow tests for regression (`cargo test newf_test`).
- [x] verify-aus3: Run existing association tests for regression (`cargo test assoc_test`).
- [x] verify-aus4: Run full unit tests (`cargo test`).
- [x] verify-aus5: Run compile check (`cargo check`).
- [x] verify-aus6: Run req-aus6 targeted test (`cargo test file_update_handler::tests::aus_test11_req_aus6_pre_new_file_flushes_before_edit_to_neutral -- --exact`).
- [x] verify-aus7: Run req-aus7 targeted test (`cargo test file_update_handler::tests::aus_test13_req_aus7_pre_close_flushes_without_path_transition -- --exact`).
- [x] verify-aus8: Run req-aus8 targeted test (`cargo test file_update_handler::tests::aus_test12_req_aus8_pre_open_file_flushes_previous_file_before_switch -- --exact`).

## File Update Handler Refactor Mapping

- [x] ref-fuh1: Rename module file and wire module declaration
  - Rename `src/singleline_create_file.rs` to `src/file_update_handler.rs`.
  - Update `src/main.rs` module declaration to `mod file_update_handler;`.

- [x] ref-fuh2: Update all module path references
  - Replace `crate::singleline_create_file::...` with `crate::file_update_handler::...` across `src/*.rs`.
  - Ensure there are no remaining references to `singleline_create_file` in source code.

- [x] ref-fuh3: Keep service-boundary extraction scope
  - Move file-update domain/service logic only:
  - file create (singleline event path),
  - file rename (singleline event path),
  - file content update/autosave (editor event path).
  - Keep UI subscriptions/focus-cursor transfer logic in `src/app.rs`.

- [x] ref-fuh4: Move autosave coordinator internals to `file_update_handler.rs`
  - Move `EditorAutoSaveState` and `EditorAutoSaveCoordinator`.
  - Move autosave timing constants and helper methods (`mark_user_edit`, `on_edit_path_changed`, `pop_due_payload`).
  - Keep existing req-aus periodic semantics unchanged.

- [x] ref-fuh5: Add autosave worker entrypoint in `file_update_handler.rs`
  - Add explicit non-main-thread worker spawn API for autosave timer/event loop.
  - Keep `try_autosave_in_edit(...)` dispatch path unchanged (MPSC/FIFO queue).
  - Keep step-2/step-3/step-5 trace behavior compatible with current debug flow.

- [x] ref-fuh6: Preserve type naming compatibility in this pass
  - Module/file name is changed to `file_update_handler`.
  - Existing public workflow type names stay as-is to avoid unnecessary churn.

- [x] ref-fuh7: Keep create/rename/autosave behavior stable
  - No behavior regressions for `req-newf*` and `req-aus*`.
  - No behavior regressions for existing singleline-editor association flows.

- [x] ref-fuh8: Sync docs/checklist references
  - Update checklist and related requirement notes that mention `singleline_create_file` to `file_update_handler` where appropriate.
  - Migration note: module file path and call-site paths are now `file_update_handler`; workflow type names are intentionally preserved in this pass.

### File Update Handler Refactor Tests

- [x] ref-fuh-test1: `cargo test newf_test` passes after module rename/extraction.
- [x] ref-fuh-test2: `cargo test aus_test` passes after module rename/extraction.
- [x] ref-fuh-test3: `cargo test assoc_test` passes (regression gate).
- [x] ref-fuh-test4: `cargo test` full suite passes.
- [x] ref-fuh-test5: `cargo check` passes.
- [x] ref-fuh-test6: static grep confirms no `singleline_create_file` references remain in `src/*.rs`.

### File Update Handler Refactor Verification

- [x] ref-fuh-verify1: `src/main.rs` declares `mod file_update_handler;` and no longer declares `mod singleline_create_file;`.
- [x] ref-fuh-verify2: `src/file_update_handler.rs` contains create/rename/autosave workflow and worker logic.
- [x] ref-fuh-verify3: `src/app.rs` no longer defines autosave coordinator structs/constants.
- [x] ref-fuh-verify4: Existing debug trace checkpoints still appear in `debug_assoc_trace.log` during manual smoke test.

