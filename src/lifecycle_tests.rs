use crate::lifecycle::*;
use std::{fs, time::{Duration, Instant}};
use std::sync::atomic::{AtomicUsize, Ordering};
static NEXT: AtomicUsize = AtomicUsize::new(0);
fn state() -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("gamepad-life-{}-{}",std::process::id(),NEXT.fetch_add(1,Ordering::Relaxed)));
    fs::create_dir_all(&p).unwrap(); p
}

#[test]
fn singleton_stop_acknowledges_exit_and_restart_ignores_old_requests() {
    let p=state();
    let daemon=Daemon::acquire(&p).unwrap().unwrap();
    assert!(Daemon::acquire(&p).unwrap().is_none());
    let cancel=daemon.cancellation();
    assert!(!cancel.cancelled());
    let old=fs::read_to_string(p.join("daemon.pid")).unwrap();
    let other=p.clone();
    let stopper=std::thread::spawn(move || stop(&other,Duration::from_secs(2)));
    let until=Instant::now()+Duration::from_secs(1);
    while !cancel.cancelled() && Instant::now()<until { std::thread::sleep(Duration::from_millis(5)); }
    assert!(cancel.cancelled(), "Stop must reach the current daemon");
    assert!(!stopper.is_finished(), "Stop waits for the daemon lock to be released");
    drop(daemon);
    assert_eq!(stopper.join().unwrap().unwrap(), true);
    let next=Daemon::acquire(&p).unwrap().unwrap();
    fs::write(p.join(format!("daemon.stop.{}",old.trim())), "").unwrap();
    assert!(!next.cancellation().cancelled(), "a stale request must not stop the new generation");
    drop(next);
    assert_eq!(stop(&p,Duration::from_millis(50)).unwrap(), false);
    fs::remove_dir_all(p).unwrap();
}

#[test]
fn stop_refuses_a_legacy_pid_and_reports_a_stuck_worker() {
    let p=state();
    let daemon=Daemon::acquire(&p).unwrap().unwrap();
    let token=fs::read_to_string(p.join("daemon.pid")).unwrap();
    fs::write(p.join("daemon.pid"), "12345").unwrap();
    assert!(stop(&p,Duration::from_millis(30)).unwrap_err().contains("legacy"));
    assert!(!daemon.cancellation().cancelled());
    fs::write(p.join("daemon.pid"),token).unwrap();
    assert!(stop(&p,Duration::from_millis(30)).unwrap_err().contains("timed out"));
    assert!(daemon.cancellation().cancelled());
    drop(daemon); fs::remove_dir_all(p).unwrap();
}

#[test]
fn lock_io_errors_are_not_treated_as_running() {
    let p=state(); fs::create_dir(p.join("daemon.pid")).unwrap();
    assert!(Daemon::acquire(&p).is_err());
    fs::remove_dir_all(p).unwrap();
}

#[test]
fn start_serializes_token_publication_with_stop() {
    use std::os::unix::io::AsRawFd;
    let p = state();
    let control = fs::File::create(p.join("daemon.control")).unwrap();
    assert_eq!(unsafe { libc::flock(control.as_raw_fd(), libc::LOCK_EX) }, 0);
    let other = p.clone();
    let starter = std::thread::spawn(move || Daemon::acquire(&other).unwrap().unwrap());
    std::thread::sleep(Duration::from_millis(50));
    let waited = !starter.is_finished();
    drop(control);
    drop(starter.join().unwrap());
    fs::remove_dir_all(p).unwrap();
    assert!(waited, "Start must not publish a token while Stop holds the control lock");
}
