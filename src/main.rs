//! herd-gamepad: drive Herdr with an Xbox controller.

mod engine;
#[cfg(test)]
mod engine_tests;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.first().map(String::as_str) {
        Some("version") | Some("--version") => { println!("herd-gamepad {}", env!("CARGO_PKG_VERSION")); 0 }
        _ => { eprintln!("usage: herd-gamepad version"); 2 }
    };
    std::process::exit(code);
}
