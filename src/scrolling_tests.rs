use crate::engine::{Btn, Input};
use crate::scrolling::*;
use std::time::{Duration, Instant};

#[test]
fn stick_dead_zone_hysteresis_and_speed() {
    let mut stick = Stick::default();
    let now = Instant::now();
    stick.observe(&Input::ScrollAxis(34));
    assert!(stick.intent(now).is_none());
    stick.observe(&Input::ScrollAxis(35));
    assert_eq!(stick.intent(now).unwrap().steps, 1);
    stick.observe(&Input::ScrollAxis(25));
    assert_eq!(stick.intent(now).unwrap().steps, 1);
    stick.observe(&Input::ScrollAxis(20));
    assert!(stick.intent(now).is_none());
    stick.observe(&Input::ScrollAxis(-60));
    assert_eq!(stick.intent(now).unwrap().steps, -2);
    stick.observe(&Input::ScrollAxis(-85));
    assert_eq!(stick.intent(now).unwrap().steps, -3);
}

#[test]
fn interrupted_tilt_requires_neutral_but_idle_buttons_do_not() {
    let now = Instant::now();
    let mut stick = Stick::default();
    stick.observe(&Input::Press(Btn::A));
    stick.observe(&Input::ScrollAxis(100));
    let first = stick.intent(now).unwrap().epoch;
    stick.observe(&Input::Press(Btn::RStick));
    assert!(stick.intent(now).is_none());
    stick.observe(&Input::ScrollAxis(95));
    assert!(stick.intent(now).is_none());
    stick.observe(&Input::ScrollAxis(0));
    stick.observe(&Input::ScrollAxis(-100));
    assert_ne!(stick.intent(now).unwrap().epoch, first);
    stick.suppress();
    assert!(stick.intent(now).is_none());
}

#[test]
fn disconnected_stick_requires_a_new_neutral_sample() {
    let now = Instant::now();
    let mut stick = Stick::default();
    stick.observe(&Input::ScrollAxis(100));
    stick.observe(&Input::Disconnected);
    stick.observe(&Input::Connected);
    stick.observe(&Input::ScrollAxis(100));
    assert!(stick.intent(now).is_none());
    stick.observe(&Input::ScrollAxis(0));
    stick.observe(&Input::ScrollAxis(100));
    assert!(stick.intent(now).is_some());
}

#[test]
fn intent_expires_when_the_controller_loop_stalls() {
    let now = Instant::now();
    let mut stick = Stick::default();
    stick.observe(&Input::ScrollAxis(100));
    let intent = stick.intent(now).unwrap();
    assert_eq!(intent.expires, now + Duration::from_millis(100));
}

use std::sync::Mutex;
#[derive(Default)]
struct Fake {
    reads: usize,
    calls: Vec<(String, i8)>,
    failure: Option<String>,
}
impl Backend for Fake {
    fn focused(&mut self) -> Result<String, String> {
        self.reads += 1;
        Ok("pane1".into())
    }
    fn scroll(&mut self, pane: &str, steps: i8, _: &dyn Fn() -> bool) -> Result<(), String> {
        self.calls.push((pane.into(), steps));
        self.failure.clone().map_or(Ok(()), Err)
    }
}
fn desired(epoch: u64, steps: i8, now: Instant) -> Intent {
    Intent {
        epoch,
        steps,
        expires: now + Duration::from_millis(100),
    }
}
#[test]
fn wheel_repeats_are_bounded_and_target_is_bound_to_gesture() {
    let now = Instant::now();
    let state = Mutex::new(Shared {
        last_epoch: Some(1),
        intent: Some(desired(1, 2, now)),
        ..Shared::default()
    });
    let mut pump = Pump::default();
    let mut backend = Fake::default();
    pump.tick(&state, &mut backend, now);
    assert_eq!(backend.calls, [("pane1".into(), 2)]);
    state
        .lock()
        .unwrap()
        .publish(Some(desired(1, -3, now + Duration::from_millis(99))));
    pump.tick(&state, &mut backend, now + Duration::from_millis(99));
    assert_eq!(backend.calls.len(), 1);
    pump.tick(&state, &mut backend, now + Duration::from_millis(100));
    assert_eq!(backend.calls[1], ("pane1".into(), -3));
    assert_eq!(backend.reads, 1);
    // No catch-up burst after the daemon stopped publishing fresh intent.
    pump.tick(&state, &mut backend, now + Duration::from_secs(2));
    assert_eq!(backend.calls.len(), 2);
    state
        .lock()
        .unwrap()
        .publish(Some(desired(2, 1, now + Duration::from_secs(2))));
    pump.tick(&state, &mut backend, now + Duration::from_secs(2));
    assert_eq!(backend.reads, 2);
    assert_eq!(backend.calls.len(), 3);
}
#[test]
fn an_error_stops_the_gesture_and_reports_once() {
    let now = Instant::now();
    let state = Mutex::new(Shared {
        last_epoch: Some(1),
        intent: Some(desired(1, 1, now)),
        ..Shared::default()
    });
    let mut pump = Pump::default();
    let mut backend = Fake {
        failure: Some("unknown_method".into()),
        ..Fake::default()
    };
    pump.tick(&state, &mut backend, now);
    assert_eq!(state.lock().unwrap().failed, Some(1));
    assert!(state
        .lock()
        .unwrap()
        .notice
        .take()
        .unwrap()
        .contains("Herdr"));
    state
        .lock()
        .unwrap()
        .publish(Some(desired(1, 1, now + Duration::from_millis(200))));
    pump.tick(&state, &mut backend, now + Duration::from_millis(200));
    assert_eq!(backend.calls.len(), 1);
    assert!(state.lock().unwrap().notice.is_none());
}
#[test]
fn cancellation_during_focus_read_never_sends_wheel() {
    let now = Instant::now();
    let state = Mutex::new(Shared {
        last_epoch: Some(1),
        intent: Some(desired(1, 1, now)),
        ..Shared::default()
    });
    struct Cancelling<'a>(&'a Mutex<Shared>);
    impl Backend for Cancelling<'_> {
        fn focused(&mut self) -> Result<String, String> {
            self.0.lock().unwrap().publish(None);
            Ok("pane1".into())
        }
        fn scroll(&mut self, _: &str, _: i8, _: &dyn Fn() -> bool) -> Result<(), String> {
            panic!("stale gesture dispatched")
        }
    }
    let mut backend = Cancelling(&state);
    Pump::default().tick(&state, &mut backend, now);
    assert!(state.lock().unwrap().intent.is_none());
}

#[test]
fn a_new_gesture_during_focus_lookup_cannot_receive_the_old_scroll() {
    let now = Instant::now();
    let state = Mutex::new(Shared {
        last_epoch: Some(1),
        intent: Some(desired(1, 1, now)),
        ..Shared::default()
    });
    struct Changing<'a>(&'a Mutex<Shared>, Instant);
    impl Backend for Changing<'_> {
        fn focused(&mut self) -> Result<String, String> {
            self.0.lock().unwrap().publish(Some(desired(2, -1, self.1)));
            Ok("old-pane".into())
        }
        fn scroll(&mut self, _: &str, _: i8, _: &dyn Fn() -> bool) -> Result<(), String> {
            panic!("old target crossed gestures")
        }
    }
    Pump::default().tick(&state, &mut Changing(&state, now), now);
    assert_eq!(state.lock().unwrap().intent.unwrap().epoch, 2);
}
#[test]
fn stopped_expired_and_failed_intents_do_not_dispatch() {
    let now = Instant::now();
    let state = Mutex::new(Shared {
        last_epoch: Some(1),
        intent: Some(desired(1, 1, now)),
        ..Shared::default()
    });
    let mut pump = Pump::default();
    let mut backend = Fake::default();
    pump.tick(&state, &mut backend, now);
    assert_eq!(backend.calls.len(), 1);
    let later = now + Duration::from_millis(100);
    pump.tick(&state, &mut backend, later);
    assert_eq!(backend.calls.len(), 1, "expiry is inclusive");
    state.lock().unwrap().publish(Some(desired(2, 1, later)));
    state.lock().unwrap().stopped = true;
    pump.tick(&state, &mut backend, later);
    assert_eq!(backend.calls.len(), 1);
    state.lock().unwrap().stopped = false;
    pump.tick(&state, &mut backend, later);
    assert_eq!(backend.calls.len(), 2);
}
#[test]
fn failure_from_an_old_gesture_does_not_disarm_its_successor() {
    let now = Instant::now();
    let state = Mutex::new(Shared {
        last_epoch: Some(1),
        intent: Some(desired(1, 1, now)),
        ..Shared::default()
    });
    struct Failing<'a>(&'a Mutex<Shared>, Instant);
    impl Backend for Failing<'_> {
        fn focused(&mut self) -> Result<String, String> {
            self.0.lock().unwrap().publish(Some(desired(2, 1, self.1)));
            Err("old failure".into())
        }
        fn scroll(&mut self, _: &str, _: i8, _: &dyn Fn() -> bool) -> Result<(), String> {
            panic!("focus failed")
        }
    }
    Pump::default().tick(&state, &mut Failing(&state, now), now);
    assert_eq!(state.lock().unwrap().intent.unwrap().epoch, 2);
    assert!(state.lock().unwrap().failed.is_none());
    assert!(state.lock().unwrap().notice.is_none());
}
#[test]
fn repeated_axis_samples_keep_the_gesture_identity() {
    let now = Instant::now();
    let mut stick = Stick::default();
    stick.observe(&Input::ScrollAxis(35));
    let first = stick.intent(now).unwrap();
    for (axis, steps) in [(59, 1), (60, 2), (84, 2), (85, 3), (21, 1)] {
        stick.observe(&Input::ScrollAxis(axis));
        let intent = stick.intent(now).unwrap();
        assert_eq!(intent.steps, steps);
        assert_eq!(intent.epoch, first.epoch);
    }
}

#[test]
fn socket_backend_selects_focus_and_sends_guarded_wheel_steps() {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixListener;
    let dir = std::env::temp_dir().join(format!("gamepad-wheel-contract-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("rpc.sock");
    let listener = UnixListener::bind(&path).unwrap();
    let thread = std::thread::spawn(move || {
        for expected in ["pane.list", "pane.scroll_wheel", "pane.scroll_wheel"] {
            let (mut socket, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(&mut socket).read_line(&mut line).unwrap();
            let request: serde_json::Value = serde_json::from_str(&line).unwrap();
            assert_eq!(request["method"], expected);
            let result = if expected == "pane.list" {
                serde_json::json!({"type":"pane_list", "panes":[
                    {"pane_id":"background","focused":false}, {"pane_id":"selected","focused":true}
                ]})
            } else {
                assert_eq!(request["params"]["pane_id"], "selected");
                assert_eq!(request["params"]["focused_only"], true);
                match request["params"]["direction"].as_str().unwrap() {
                    "up" => assert_eq!(request["params"]["lines"], 3),
                    "down" => assert_eq!(request["params"]["lines"], 1),
                    other => panic!("unexpected direction {other}"),
                }
                serde_json::json!({"type":"ok"})
            };
            writeln!(
                socket,
                "{}",
                serde_json::json!({"id":request["id"], "result":result})
            )
            .unwrap();
        }
    });
    let mut backend = SocketBackend {
        path: Some(path.clone()),
    };
    assert_eq!(backend.focused().unwrap(), "selected");
    backend.scroll("selected", 3, &|| true).unwrap();
    backend.scroll("selected", -1, &|| true).unwrap();
    thread.join().unwrap();
    std::fs::remove_file(path).unwrap();
    std::fs::remove_dir(dir).unwrap();
}

#[test]
fn missing_socket_configuration_is_an_action_local_error() {
    let mut backend = SocketBackend { path: None };
    assert!(backend.focused().unwrap_err().contains("HERDR_SOCKET_PATH"));
    assert!(backend
        .scroll("pane", 1, &|| true)
        .unwrap_err()
        .contains("HERDR_SOCKET_PATH"));
}

#[test]
fn unsupported_and_stale_errors_have_short_specific_notices() {
    for (error, expected) in [
        (
            format!(
                "unknown variant `pane.scroll_wheel`, expected {}",
                "known_method,".repeat(1000)
            ),
            "This Herdr build does not support joystick scrolling",
        ),
        ("stale_pane_target".into(), "Pane focus changed"),
        ("unavailable".repeat(1000), "Joystick scrolling stopped"),
    ] {
        let now = Instant::now();
        let state = Mutex::new(Shared {
            last_epoch: Some(1),
            intent: Some(desired(1, 1, now)),
            ..Shared::default()
        });
        let mut fake = Fake {
            failure: Some(error),
            ..Fake::default()
        };
        Pump::default().tick(&state, &mut fake, now);
        let notice = state.lock().unwrap().notice.take().unwrap();
        assert!(notice.starts_with(expected), "{notice}");
        assert!(notice.chars().count() <= 240);
    }
}

#[test]
fn expired_gesture_does_not_even_query_focus() {
    let now = Instant::now();
    let state = Mutex::new(Shared {
        last_epoch: Some(1),
        intent: Some(Intent {
            epoch: 1,
            steps: 1,
            expires: now,
        }),
        ..Shared::default()
    });
    let mut pump = Pump::default();
    let mut backend = Fake::default();
    pump.tick(&state, &mut backend, now);
    assert_eq!(backend.reads, 0);
    state.lock().unwrap().publish(Some(desired(1, 1, now)));
    pump.tick(&state, &mut backend, now);
    assert_eq!(backend.reads, 1);
    assert_eq!(backend.calls.len(), 1);
}

fn backend_reply(result: serde_json::Value, focused: bool) -> Result<String, String> {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixListener;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let path = std::env::temp_dir().join(format!(
        "gamepad-wheel-reply-{}-{}.sock",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    ));
    let listener = UnixListener::bind(&path).unwrap();
    let thread = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        let mut line = String::new();
        BufReader::new(&mut socket).read_line(&mut line).unwrap();
        let request: serde_json::Value = serde_json::from_str(&line).unwrap();
        writeln!(
            socket,
            "{}",
            serde_json::json!({"id":request["id"],"result":result})
        )
        .unwrap();
    });
    let mut backend = SocketBackend {
        path: Some(path.clone()),
    };
    let answer = if focused {
        backend.focused()
    } else {
        backend.scroll("pane", 1, &|| true).map(|()| "ok".into())
    };
    thread.join().unwrap();
    std::fs::remove_file(path).unwrap();
    answer
}
#[test]
fn missing_or_invalid_focus_and_unexpected_success_are_rejected() {
    use serde_json::json;
    for result in [
        json!({"type":"wrong", "panes":[{"pane_id":"pane", "focused":true}]}),
        json!({"type":"pane_list", "panes":[]}),
        json!({"type":"pane_list", "panes":[{"pane_id":"pane", "focused":false}]}),
        json!({"type":"pane_list", "panes":[{"pane_id":"", "focused":true}]}),
    ] {
        assert!(backend_reply(result, true).is_err());
    }
    assert!(backend_reply(json!({"type":"pane_list"}), false).is_err());
    assert_eq!(backend_reply(json!({"type":"ok"}), false).unwrap(), "ok");
}

#[test]
fn backend_can_recheck_suppression_immediately_before_writing() {
    let now = Instant::now();
    let state = Mutex::new(Shared {
        last_epoch: Some(1),
        intent: Some(desired(1, 1, now)),
        ..Shared::default()
    });
    struct Suppressing<'a>(&'a Mutex<Shared>, bool);
    impl Backend for Suppressing<'_> {
        fn focused(&mut self) -> Result<String, String> {
            Ok("pane".into())
        }
        fn scroll(&mut self, _: &str, _: i8, current: &dyn Fn() -> bool) -> Result<(), String> {
            assert!(current());
            self.0.lock().unwrap().publish(None);
            self.1 = true;
            assert!(!current(), "suppressed gesture still writable");
            Ok(())
        }
    }
    let mut backend = Suppressing(&state, false);
    Pump::default().tick(&state, &mut backend, now);
    assert!(backend.1);
}
#[test]
fn a_sent_request_failure_is_reported_after_release_but_not_after_a_successor() {
    for successor in [false, true] {
        let now = Instant::now();
        let state = Mutex::new(Shared {
            last_epoch: Some(1),
            intent: Some(desired(1, 1, now)),
            ..Shared::default()
        });
        struct Failing<'a>(&'a Mutex<Shared>, Instant, bool);
        impl Backend for Failing<'_> {
            fn focused(&mut self) -> Result<String, String> {
                Ok("pane".into())
            }
            fn scroll(&mut self, _: &str, _: i8, _: &dyn Fn() -> bool) -> Result<(), String> {
                let mut state = self.0.lock().unwrap();
                if self.2 {
                    state.publish(Some(desired(2, 1, self.1)));
                }
                state.publish(None);
                Err("unknown_method".into())
            }
        }
        Pump::default().tick(&state, &mut Failing(&state, now, successor), now);
        let state = state.lock().unwrap();
        assert_eq!(state.notice.is_some(), !successor);
        assert_eq!(state.failed, if successor { None } else { Some(1) });
    }
}

#[test]
fn backend_recheck_rejects_a_successor_gesture_and_socket_skips_cancelled_connect() {
    let now = Instant::now();
    let state = Mutex::new(Shared {
        last_epoch: Some(1),
        intent: Some(desired(1, 1, now)),
        ..Shared::default()
    });
    struct Successor<'a>(&'a Mutex<Shared>, Instant, bool);
    impl Backend for Successor<'_> {
        fn focused(&mut self) -> Result<String, String> {
            Ok("pane".into())
        }
        fn scroll(&mut self, _: &str, _: i8, current: &dyn Fn() -> bool) -> Result<(), String> {
            assert!(current());
            self.0.lock().unwrap().publish(Some(desired(2, 1, self.1)));
            self.2 = true;
            assert!(
                !current(),
                "successor gesture reused old write authorization"
            );
            Ok(())
        }
    }
    let mut backend = Successor(&state, now, false);
    Pump::default().tick(&state, &mut backend, now);
    assert!(backend.2);
    let mut backend = SocketBackend {
        path: Some("/nonexistent/gamepad-cancelled.sock".into()),
    };
    assert!(backend.scroll("pane", 1, &|| false).is_ok());
}
