//! herd-gamepad: drive Herdr with an Xbox controller.

mod engine;
#[cfg(test)]
mod engine_tests;
mod herdr;
#[cfg(test)]
mod herdr_tests;
mod config;
#[cfg(test)]
mod config_tests;
mod voice;
#[cfg(test)]
mod voice_tests;
mod pad;
#[cfg(test)]
mod pad_tests;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.first().map(String::as_str) {
        Some("version") | Some("--version") => { println!("herd-gamepad {}", env!("CARGO_PKG_VERSION")); 0 }
        Some("probe") => run(pad::probe),
        _ => { eprintln!("usage: herd-gamepad version"); 2 }
    };
    std::process::exit(code);
}

fn run(f: impl FnOnce() -> Result<(), String>) -> i32 {
    match f() { Ok(()) => 0, Err(e) => { eprintln!("herd-gamepad: {e}"); 1 } }
}
