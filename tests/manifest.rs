//! The manifest calls real subcommands of the binary it builds.
#[test]
fn entries_call_real_subcommands() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/herdr-plugin.toml")).unwrap();
    let m: toml::Table = text.parse().unwrap();
    assert_eq!(m["id"].as_str(), Some("herd.gamepad"));
    let real = ["serve", "ensure", "setup", "mapping", "open-mapping"];
    let mut entries = vec![];
    for key in ["panes", "actions"] { entries.extend(m[key].as_array().unwrap().iter().cloned()); }
    assert!(!entries.is_empty());
    for e in &entries {
        let cmd: Vec<&str> = e["command"].as_array().unwrap().iter().map(|c| c.as_str().unwrap()).collect();
        assert_eq!(cmd[0], "./bin/herd-gamepad", "{e:?}");
        assert!(real.contains(&cmd[1]), "{e:?}");
    }
    let panes = m["panes"].as_array().unwrap();
    let daemon = panes.iter().find(|p| p["id"].as_str() == Some("daemon")).unwrap();
    assert_eq!(daemon["command"][1].as_str(), Some("serve"));
    assert_eq!(daemon["placement"].as_str(), Some("tab"), "the daemon lives in its own tab, out of the layout");
    let mapping = panes.iter().find(|p| p["id"].as_str() == Some("mapping")).unwrap();
    assert_eq!(mapping["placement"].as_str(), Some("overlay"));
    let actions = m["actions"].as_array().unwrap();
    for sub in ["ensure", "setup", "open-mapping"] {
        assert!(actions.iter().any(|a| a["command"][1].as_str() == Some(sub)), "an action runs {sub}");
    }
    let build = m["build"].as_array().unwrap();
    assert!(build.iter().any(|b| b["command"].as_array().unwrap().iter().any(|c| c.as_str() == Some("scripts/build.sh"))));
    assert!(m["platforms"].as_array().unwrap().iter().any(|p| p.as_str() == Some("macos")));
}
