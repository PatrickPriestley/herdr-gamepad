use crate::logging::*;
use std::fs;
#[test]
fn rotation_retains_recent_logs_and_bounds_disk_usage() {
    let p=std::env::temp_dir().join(format!("gamepad-log-{}",std::process::id()));
    fs::create_dir_all(&p).unwrap();
    let mut log=RollingLog::new(&p,128).unwrap();
    log.append(&"a".repeat(100)).unwrap();
    log.append(&"b".repeat(100)).unwrap();
    log.append(&"c".repeat(300)).unwrap();
    let text=tail(&p).unwrap();
    assert!(text.contains("bbb") && text.contains("ccc"), "{text}");
    assert!(!text.contains("aaa"));
    assert!(fs::metadata(p.join("daemon.log")).unwrap().len()<=128);
    assert!(fs::metadata(p.join("daemon.log.1")).unwrap().len()<=128);
    fs::remove_dir_all(p).unwrap();
}
