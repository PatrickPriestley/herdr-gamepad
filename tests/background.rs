//! Exercise the real CLI and cross-process lock/stop protocol without controller hardware.
#[allow(dead_code)]
#[path = "../src/lifecycle.rs"]
mod lifecycle;
use std::{fs, process::{Command, Stdio}, time::{Duration, Instant}};

#[test]
fn fixture_worker() {
    let Some(state) = std::env::var_os("GAMEPAD_TEST_WORKER_STATE") else { return; };
    let daemon = lifecycle::Daemon::acquire(std::path::Path::new(&state)).unwrap().unwrap();
    println!("worker ready");
    while !daemon.cancellation().cancelled() { std::thread::sleep(Duration::from_millis(10)); }
}

#[test]
fn cli_startup_errors_logs_and_cross_process_stop() {
    let state = std::env::temp_dir().join(format!("gamepad-cli-{}", std::process::id()));
    fs::create_dir_all(&state).unwrap();
    let cli = |arg: &str| {
        Command::new(env!("CARGO_BIN_EXE_herd-gamepad")).arg(arg)
            .env("HERDR_PLUGIN_STATE_DIR", &state).env("HERDR_PLUGIN_CONFIG_DIR", state.join("config"))
            .stdin(Stdio::null()).output().unwrap()
    };
    assert!(!cli("serve").status.success(), "missing model must fail startup");
    let logs = cli("logs");
    assert!(logs.status.success());
    assert!(String::from_utf8_lossy(&logs.stdout).contains("whisper model missing"));
    assert!(String::from_utf8_lossy(&cli("stop").stdout).contains("already stopped"));

    let mut worker = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "fixture_worker", "--nocapture"])
        .env("GAMEPAD_TEST_WORKER_STATE", &state).stdout(Stdio::piped()).spawn().unwrap();
    use std::io::BufRead;
    let mut ready = false;
    let mut reader = std::io::BufReader::new(worker.stdout.take().unwrap());
    for line in (&mut reader).lines() {
        if line.unwrap().contains("worker ready") { ready = true; break; }
    }
    assert!(ready);
    assert!(lifecycle::Daemon::acquire(&state).unwrap().is_none());
    let stopped = cli("stop");
    assert!(stopped.status.success(), "{}", String::from_utf8_lossy(&stopped.stderr));
    assert!(String::from_utf8_lossy(&stopped.stdout).contains("herd-gamepad stopped"));
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(status) = worker.try_wait().unwrap() { assert!(status.success()); break; }
        if Instant::now() >= deadline { let _ = worker.kill(); let _ = worker.wait(); panic!("worker did not exit"); }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(String::from_utf8_lossy(&cli("stop").stdout).contains("already stopped"));
    fs::remove_dir_all(state).unwrap();
}
