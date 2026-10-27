//! herd-gamepad: drive Herdr with an Xbox controller.

mod config;
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
        Some("serve") => run(serve::serve),
        Some("ensure") => run(serve::ensure),
        Some("setup") => run(serve::setup),
        Some("probe") => run(pad::probe),
        Some("mapping") => { print!("{}", serve::MAPPING); let _ = std::io::stdin().read_line(&mut String::new()); 0 }
        Some("open-mapping") => run(|| herdr::Cli::default().run(&["herdr", "plugin", "pane", "open", "--plugin", config::PLUGIN_ID, "--entrypoint", "mapping"]).map(|_| ())),
        Some("version") | Some("--version") => { println!("herd-gamepad {}", env!("CARGO_PKG_VERSION")); 0 }
        _ => { eprintln!("usage: herd-gamepad serve|ensure|setup|probe|mapping|open-mapping|version"); 2 }
    };
    std::process::exit(code);
}

fn run(f: impl FnOnce() -> Result<(), String>) -> i32 {
    match f() { Ok(()) => 0, Err(e) => { eprintln!("herd-gamepad: {e}"); 1 } }
}
