use std::io;
use std::process;

fn main() {
    let app_paths = match papyru2::path_resolver::AppPaths::resolve() {
        Ok(paths) => paths,
        Err(error) => {
            eprintln!("papyru2_textfile_import: path resolver failed: {error}");
            process::exit(2);
        }
    };

    let mut stdout = io::stdout();
    let mut stderr = io::stderr();
    let exit_code = papyru2::textfile_import::run_cli_with_app_paths(
        std::env::args_os(),
        &app_paths,
        &mut stdout,
        &mut stderr,
    );
    process::exit(exit_code);
}
