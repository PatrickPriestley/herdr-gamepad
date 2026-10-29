//! The manifest calls real subcommands of the binary it builds.
#[test]
fn entries_call_real_subcommands() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/herdr-plugin.toml")).unwrap();
    let m: toml::Table = text.parse().unwrap();
    assert_eq!(m["id"].as_str(), Some("herd.gamepad"));
    let real = ["serve", "ensure", "stop", "setup", "mapping", "open-mapping", "logs", "open-logs", "close-menu", "actions-menu"];
    let mut entries = vec![];
    for key in ["panes", "actions"] { entries.extend(m[key].as_array().unwrap().iter().cloned()); }
    assert!(!entries.is_empty());
    for e in &entries {
        let cmd: Vec<&str> = e["command"].as_array().unwrap().iter().map(|c| c.as_str().unwrap()).collect();
        assert_eq!(cmd[0], "./bin/herd-gamepad", "{e:?}");
        assert!(real.contains(&cmd[1]), "{e:?}");
    }
    let panes = m["panes"].as_array().unwrap();
    assert!(panes.iter().all(|p| p["id"].as_str() != Some("daemon")), "background operation has no daemon pane");
    let logs = panes.iter().find(|p| p["id"].as_str() == Some("logs")).unwrap();
    assert_eq!(logs["placement"].as_str(), Some("overlay"));
    let mapping = panes.iter().find(|p| p["id"].as_str() == Some("mapping")).unwrap();
    assert_eq!(mapping["placement"].as_str(), Some("overlay"));
    let close = panes.iter().find(|p| p["id"].as_str() == Some("close-menu"));
    assert!(close.is_some(), "explicit close actions have a popup entrypoint");
    assert_eq!(close.unwrap()["placement"].as_str(), Some("popup"));
    let menu = panes.iter().find(|p| p["id"].as_str() == Some("actions-menu")).unwrap();
    assert_eq!(menu["placement"].as_str(), Some("popup"));
    let actions = m["actions"].as_array().unwrap();
    for sub in ["serve", "stop", "setup", "open-mapping", "open-logs"] {
        assert!(actions.iter().any(|a| a["command"][1].as_str() == Some(sub)), "an action runs {sub}");
    }
    let build = m["build"].as_array().unwrap();
    assert!(build.iter().any(|b| b["command"].as_array().unwrap().iter().any(|c| c.as_str() == Some("scripts/build.sh"))));
    assert!(m["platforms"].as_array().unwrap().iter().any(|p| p.as_str() == Some("macos")));
}

#[test]
fn start_is_a_managed_background_action_without_automatic_ui() {
    let m: toml::Table = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/herdr-plugin.toml")).unwrap().parse().unwrap();
    let actions = m["actions"].as_array().unwrap();
    let start = actions.iter().find(|a| a["id"].as_str() == Some("start")).unwrap();
    assert_eq!(start["command"][1].as_str(), Some("serve"));
    assert!(m.get("startup").is_none());
    assert!(m.get("events").is_none());
}
