//! herd-gamepad: drive Herdr with a game controller.

mod menu;
mod menu_render;
#[cfg(test)] mod menu_tests;
#[cfg(test)] mod menu_render_tests;

mod feedback;
mod config;
mod settings;
mod menu_worker;
mod menu_runtime;
mod lifecycle;
mod logging;
#[cfg(test)] mod lifecycle_tests;
#[cfg(test)] mod logging_tests;
mod engine;
mod herdr;
mod pad;
mod serve;
mod voice;
#[cfg(test)] mod config_tests;
#[cfg(test)] mod engine_tests;
#[cfg(test)] mod herdr_tests;
#[cfg(test)] mod pad_tests;
#[cfg(test)] mod serve_tests;
#[cfg(test)] mod voice_tests;

use herdr::Runner;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.first().map(String::as_str) {
        Some("close-menu" | "actions-menu") => run(feedback::render_from_env),
        Some("serve") => run(serve::serve),
        Some("stop") => run(serve::stop),
        Some("logs") => run(serve::logs),
        Some("open-logs") => run(|| herdr::Cli::default().run(&["herdr", "plugin", "pane", "open", "--plugin", config::PLUGIN_ID, "--entrypoint", "logs"]).map(|_| ())),
        Some("ensure") => run(serve::ensure),
        Some("setup") => run(serve::setup),
        Some("probe") => run(pad::probe),
        Some("mapping") => run(serve::mapping),
        Some("open-mapping") => run(|| herdr::Cli::default().run(&["herdr", "plugin", "pane", "open", "--plugin", config::PLUGIN_ID, "--entrypoint", "mapping"]).map(|_| ())),
        Some("version") | Some("--version") => { println!("herd-gamepad {}", env!("CARGO_PKG_VERSION")); 0 }
        _ => { eprintln!("usage: herd-gamepad serve|ensure|stop|setup|probe|actions-menu|close-menu|mapping|open-mapping|logs|open-logs|version"); 2 }
    };
    std::process::exit(code);
}

fn run(f: impl FnOnce() -> Result<(), String>) -> i32 {
    match f() { Ok(()) => 0, Err(e) => { crate::logging::event(format_args!("herd-gamepad: {e}")); 1 } }
}
