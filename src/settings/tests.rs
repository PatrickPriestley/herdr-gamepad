use super::*;
fn fixture() -> std::path::PathBuf {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let d = std::env::temp_dir().join(format!(
        "settings-{}-{}",
        std::process::id(),
        N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    fs::create_dir_all(&d).unwrap();
    d.join("config.toml")
}
#[test]
fn save_preserves_comments_custom_duration_and_uses_new_version() {
    let p = fixture();
    let text = "# header\n[voice]\nbutton = 'rstick' # binding\nmax_seconds = 73 # custom\n[unknown]\nthing = 9\n";
    fs::write(&p, text).unwrap();
    let (old, _) = load(&p, p.parent().unwrap()).unwrap();
    let mut new = old.clone();
    new.voice_button = Btn::Menu;
    new.labels = LabelStyle::Xbox;
    let (saved, cfg) = save(&p, p.parent().unwrap(), &old, &new).unwrap();
    let actual = fs::read_to_string(&p).unwrap();
    assert!(actual.contains("# binding"));
    assert!(actual.contains("max_seconds = 73 # custom"));
    assert!(actual.contains("thing = 9"));
    assert!(actual.starts_with("# header"));
    assert_ne!(saved.version, old.version);
    assert_eq!(cfg.voice_button, Btn::Menu);
    assert_eq!(cfg.labels, LabelStyle::Xbox);
    new = saved.clone();
    new.max_seconds = 30;
    let (next, _) = save(&p, p.parent().unwrap(), &saved, &new).unwrap();
    assert_eq!(next.max_seconds, 30);
    assert_ne!(next.version, saved.version);
    fs::remove_dir_all(p.parent().unwrap()).unwrap();
}
#[test]
fn absence_and_empty_conflict_and_final_compare_protects_external_edit() {
    let p = fixture();
    let (old, _) = load(&p, p.parent().unwrap()).unwrap();
    fs::write(&p, "").unwrap();
    assert!(matches!(
        save(&p, p.parent().unwrap(), &old, &old),
        Err(SaveError::Conflict(_))
    ));
    let (old, _) = load(&p, p.parent().unwrap()).unwrap();
    let mut new = old.clone();
    new.labels = LabelStyle::Xbox;
    let result = save_before_commit(&p, p.parent().unwrap(), &old, &new, || {
        fs::write(&p, "# external").unwrap()
    });
    assert!(matches!(result, Err(SaveError::Conflict(_))));
    assert_eq!(fs::read_to_string(&p).unwrap(), "# external");
    fs::remove_dir_all(p.parent().unwrap()).unwrap();
}
#[test]
fn unsupported_edits_fail_without_touching_original() {
    let p = fixture();
    let (old, _) = load(&p, p.parent().unwrap()).unwrap();
    let mut new = old.clone();
    new.voice_button = Btn::A;
    assert!(save(&p, p.parent().unwrap(), &old, &new).is_err());
    new.voice_button = Btn::Menu;
    new.max_seconds = 0;
    assert!(save(&p, p.parent().unwrap(), &old, &new).is_err());
    assert!(!p.exists());
    fs::remove_dir_all(p.parent().unwrap()).unwrap();
}
#[test]
fn malformed_container_is_reported_without_overwriting() {
    for text in ["voice=4", "ui=[]"] {
        let p = fixture();
        fs::write(&p, text).unwrap();
        let (old, _) = load(&p, p.parent().unwrap()).unwrap();
        let mut new = old.clone();
        new.voice_button = Btn::Menu;
        new.labels = LabelStyle::Xbox;
        assert!(save(&p, p.parent().unwrap(), &old, &new).is_err());
        assert_eq!(fs::read_to_string(&p).unwrap(), text);
        fs::remove_dir_all(p.parent().unwrap()).unwrap();
    }
}
#[test]
fn save_preserves_permissions_and_competing_writer_lock_refuses() {
    use std::os::unix::fs::PermissionsExt;
    let p = fixture();
    fs::write(&p, "[voice]\nmax_seconds=73\n").unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o640)).unwrap();
    let (old, _) = load(&p, p.parent().unwrap()).unwrap();
    let mut new = old.clone();
    new.labels = LabelStyle::PlayStation;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(p.parent().unwrap().join("config.lock"))
        .unwrap();
    assert_eq!(
        unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
        0
    );
    assert!(
        matches!(save(&p,p.parent().unwrap(),&old,&new),Err(SaveError::Failed(ref e)) if e.contains("writer"))
    );
    assert_eq!(load(&p, p.parent().unwrap()).unwrap().0, old);
    drop(lock);
    let (saved, _) = save(&p, p.parent().unwrap(), &old, &new).unwrap();
    assert_eq!(saved.labels, LabelStyle::PlayStation);
    assert_eq!(
        fs::metadata(&p).unwrap().permissions().mode() & 0o777,
        0o640
    );
    fs::remove_dir_all(p.parent().unwrap()).unwrap();
}
#[test]
fn source_errors_are_not_absent_defaults_and_lstick_roundtrips() {
    let p = fixture();
    fs::create_dir(&p).unwrap();
    assert!(load(&p, p.parent().unwrap()).is_err());
    fs::remove_dir(&p).unwrap();
    let (old, _) = load(&p, p.parent().unwrap()).unwrap();
    let mut new = old.clone();
    new.voice_button = Btn::LStick;
    let (saved, cfg) = save(&p, p.parent().unwrap(), &old, &new).unwrap();
    assert_eq!(saved.voice_button, Btn::LStick);
    assert_eq!(cfg.voice_button, Btn::LStick);
    fs::remove_dir_all(p.parent().unwrap()).unwrap();
}

#[test]
fn symlink_config_save_refuses_without_replacing_link_or_changing_target() {
    use std::os::unix::fs::symlink;
    let p = fixture();
    let target = p.parent().unwrap().join("dotfiles.toml");
    let text = "# managed dotfile\n[voice]\nbutton = 'rstick'\n";
    fs::write(&target, text).unwrap();
    symlink(&target, &p).unwrap();
    let (old, _) = load(&p, p.parent().unwrap()).unwrap();
    let mut new = old.clone();
    new.voice_button = Btn::Menu;
    let before_commit_called = std::cell::Cell::new(false);
    let result = save_before_commit(&p, p.parent().unwrap(), &old, &new, || {
        before_commit_called.set(true)
    });
    assert!(matches!(result, Err(SaveError::Failed(message)) if message.contains("symlink")));
    assert!(!before_commit_called.get());
    assert!(fs::symlink_metadata(&p).unwrap().file_type().is_symlink());
    assert_eq!(fs::read_link(&p).unwrap(), target);
    assert_eq!(fs::read_to_string(&target).unwrap(), text);
    fs::remove_dir_all(p.parent().unwrap()).unwrap();
}

#[test]
fn dangling_config_symlink_is_not_treated_as_an_absent_writable_file() {
    use std::os::unix::fs::symlink;
    let p = fixture();
    let target = p.parent().unwrap().join("missing-dotfile.toml");
    symlink(&target, &p).unwrap();
    let (old, _) = load(&p, p.parent().unwrap()).unwrap();
    let mut new = old.clone();
    new.labels = LabelStyle::Xbox;
    assert!(
        matches!(save(&p, p.parent().unwrap(), &old, &new), Err(SaveError::Failed(message)) if message.contains("symlink"))
    );
    assert_eq!(fs::read_link(&p).unwrap(), target);
    assert!(!target.exists());
    fs::remove_dir_all(p.parent().unwrap()).unwrap();
}

#[test]
fn symlink_introduced_before_commit_is_refused_even_when_contents_match() {
    use std::os::unix::fs::symlink;
    let p = fixture();
    let target = p.parent().unwrap().join("dotfiles.toml");
    let text = "[voice]\nbutton = 'rstick'\n";
    fs::write(&p, text).unwrap();
    fs::write(&target, text).unwrap();
    let (old, _) = load(&p, p.parent().unwrap()).unwrap();
    let mut new = old.clone();
    new.voice_button = Btn::Menu;
    let result = save_before_commit(&p, p.parent().unwrap(), &old, &new, || {
        fs::remove_file(&p).unwrap();
        symlink(&target, &p).unwrap();
    });
    assert!(matches!(result, Err(SaveError::Failed(message)) if message.contains("symlink")));
    assert_eq!(fs::read_link(&p).unwrap(), target);
    assert_eq!(fs::read_to_string(&target).unwrap(), text);
    assert!(
        !fs::read_dir(p.parent().unwrap()).unwrap().any(|entry| entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".tmp"))
    );
    fs::remove_dir_all(p.parent().unwrap()).unwrap();
}

#[test]
fn destination_metadata_errors_are_reported_instead_of_allowed_as_absence() {
    let p = fixture();
    fs::write(&p, "not a directory").unwrap();
    let invalid = p.join("nested-config.toml");
    let error = ensure_writable_destination(&invalid).unwrap_err();
    assert!(error.contains("config.toml destination"));
    assert_eq!(fs::read_to_string(&p).unwrap(), "not a directory");
    fs::remove_dir_all(p.parent().unwrap()).unwrap();
}
