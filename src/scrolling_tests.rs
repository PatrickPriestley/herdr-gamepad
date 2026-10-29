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
fn scroll_repeats_are_bounded_and_target_is_bound_to_gesture() {
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
fn cancellation_during_focus_read_never_sends_scroll() {
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

fn pane_reply(offset: u64, max: u64) -> serde_json::Value {
    serde_json::json!({"type":"pane_info", "pane":{
        "pane_id":"selected", "focused":true,
        "scroll":{"offset_from_bottom":offset,"max_offset_from_bottom":max,"viewport_rows":24}
    }})
}

/// The fixture records requests and stops when the synchronous client returns.
/// It never waits for an expected request that a refusing client need not send.
fn with_backend<R>(
    replies: Vec<serde_json::Value>,
    run: impl FnOnce(&mut SocketBackend) -> R,
) -> (R, Vec<serde_json::Value>) {
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixListener;
    use std::sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    };
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let path = std::env::temp_dir().join(format!(
        "scroll-history-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let listener = UnixListener::bind(&path).unwrap();
    listener.set_nonblocking(true).unwrap();
    let stopped = Arc::new(AtomicBool::new(false));
    let stop = stopped.clone();
    let thread = std::thread::spawn(move || {
        let mut requests = Vec::new();
        while !stop.load(Ordering::SeqCst) {
            let (mut socket, _) = match listener.accept() {
                Ok(pair) => pair,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(1));
                    continue;
                }
                Err(error) => panic!("accept: {error}"),
            };
            socket.set_nonblocking(false).unwrap();
            let _ = socket.set_read_timeout(Some(Duration::from_secs(1)));
            let mut line = String::new();
            if BufReader::new(&mut socket)
                .read_line(&mut line)
                .unwrap_or(0)
                == 0
            {
                continue;
            }
            let request: serde_json::Value = serde_json::from_str(&line).unwrap();
            let result = replies
                .get(requests.len())
                .cloned()
                .unwrap_or(serde_json::json!({"type":"unexpected"}));
            let _ = writeln!(
                socket,
                "{}",
                serde_json::json!({"id":request["id"],"result":result})
            );
            requests.push(request);
        }
        requests
    });
    let answer = run(&mut SocketBackend {
        path: Some(path.clone()),
    });
    stopped.store(true, Ordering::SeqCst);
    let requests = thread.join().unwrap();
    std::fs::remove_file(path).unwrap();
    (answer, requests)
}

#[test]
fn socket_backend_binds_only_the_selected_pane() {
    let (result, requests) = with_backend(
        vec![serde_json::json!({"type":"pane_list", "panes":[
            {"pane_id":"background","focused":false},{"pane_id":"selected","focused":true}
        ]})],
        |backend| backend.focused(),
    );
    assert_eq!(result, Ok("selected".into()));
    assert_eq!(requests[0]["method"], "pane.list");
    assert_eq!(requests[0]["params"], serde_json::json!({}));
}

#[test]
fn socket_backend_uses_fresh_offsets_for_up_and_down() {
    let (result, requests) = with_backend(
        vec![
            pane_reply(4, 20),
            pane_reply(7, 20),
            pane_reply(12, 20),
            pane_reply(11, 20),
        ],
        |backend| {
            backend.scroll("selected", 3, &|| true)?;
            backend.scroll("selected", -1, &|| true)
        },
    );
    assert_eq!(result, Ok(()));
    assert_eq!(
        requests
            .iter()
            .map(|r| r["method"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["pane.get", "pane.scroll", "pane.get", "pane.scroll"]
    );
    assert_eq!(
        requests[0]["params"],
        serde_json::json!({"pane_id":"selected"})
    );
    assert_eq!(
        requests[1]["params"],
        serde_json::json!({"pane_id":"selected","offset_from_bottom":7})
    );
    assert_eq!(
        requests[3]["params"],
        serde_json::json!({"pane_id":"selected","offset_from_bottom":11})
    );
}

#[test]
fn scrollback_updates_clamp_without_overflow_or_underflow() {
    for (offset, max, steps, next) in [
        (9, 10, 3, 10),
        (1, 10, -3, 0),
        (u64::MAX - 1, u64::MAX, 3, u64::MAX),
    ] {
        let (result, requests) = with_backend(
            vec![pane_reply(offset, max), pane_reply(next, max)],
            |backend| backend.scroll("selected", steps, &|| true),
        );
        assert_eq!(result, Ok(()));
        assert_eq!(requests[1]["params"]["offset_from_bottom"], next);
    }
}

#[test]
fn scrollback_boundaries_do_not_send_redundant_updates() {
    for (offset, steps) in [(10, 3), (0, -3)] {
        let (result, requests) = with_backend(vec![pane_reply(offset, 10)], |backend| {
            backend.scroll("selected", steps, &|| true)
        });
        assert_eq!(result, Ok(()));
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0]["method"], "pane.get");
    }
}

#[test]
fn no_history_and_invalid_metrics_refuse_without_mutation() {
    use serde_json::json;
    let mut cases = vec![
        (pane_reply(0, 0), "No terminal scrollback"),
        (pane_reply(11, 10), "Invalid scroll metrics"),
    ];
    for metrics in [
        json!(null),
        json!({}),
        json!({"offset_from_bottom":-1,"max_offset_from_bottom":10}),
        json!({"offset_from_bottom":0,"max_offset_from_bottom":"10"}),
    ] {
        let mut reply = pane_reply(0, 10);
        reply["pane"]["scroll"] = metrics;
        cases.push((reply, "Invalid scroll metrics"));
    }
    for (reply, expected) in cases {
        let (result, requests) = with_backend(vec![reply], |backend| {
            backend.scroll("selected", 1, &|| true)
        });
        assert!(result.unwrap_err().contains(expected));
        assert_eq!(requests.len(), 1);
    }
}

#[test]
fn stale_focus_wrong_identity_and_malformed_pane_results_refuse() {
    use serde_json::json;
    let mut not_focused = pane_reply(4, 10);
    not_focused["pane"]["focused"] = json!(false);
    let mut other = pane_reply(4, 10);
    other["pane"]["pane_id"] = json!("other");
    for (reply, expected) in [
        (not_focused, "stale_pane_target"),
        (other, "Unexpected scroll pane"),
        (json!({"type":"wrong"}), "Unexpected pane info response"),
        (json!({"type":"pane_info"}), "Unexpected scroll pane"),
        (
            json!({"type":"pane_info","pane":{"pane_id":"selected"}}),
            "stale_pane_target",
        ),
    ] {
        let (result, requests) = with_backend(vec![reply], |backend| {
            backend.scroll("selected", 1, &|| true)
        });
        assert!(result.unwrap_err().contains(expected));
        assert_eq!(requests.len(), 1);
    }
}

#[test]
fn scroll_response_must_identify_the_same_pane_and_valid_metrics() {
    use serde_json::json;
    let mut other = pane_reply(5, 10);
    other["pane"]["pane_id"] = json!("other");
    let mut invalid = pane_reply(5, 10);
    invalid["pane"]["scroll"] = json!(null);
    for reply in [
        json!({"type":"ok"}),
        json!({"type":"pane_info"}),
        other,
        invalid,
        pane_reply(11, 10),
    ] {
        let (result, requests) = with_backend(vec![pane_reply(4, 10), reply], |backend| {
            backend.scroll("selected", 1, &|| true)
        });
        assert!(result.is_err());
        assert_eq!(requests.len(), 2);
    }
}

#[test]
fn cancelled_gesture_before_read_or_after_lookup_sends_no_update() {
    use std::cell::Cell;
    let mut backend = SocketBackend {
        path: Some("/nonexistent/cancelled-scrollback".into()),
    };
    assert_eq!(backend.scroll("selected", 1, &|| false), Ok(()));
    let calls = Cell::new(0);
    let (result, requests) = with_backend(vec![pane_reply(4, 10)], |backend| {
        backend.scroll("selected", 1, &|| {
            calls.set(calls.get() + 1);
            calls.get() <= 2
        })
    });
    assert_eq!(result, Ok(()));
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0]["method"], "pane.get");
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
                "unknown variant `pane.scroll`, expected {}",
                "known_method,".repeat(1000)
            ),
            "This Herdr build does not expose terminal scrollback",
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
        "gamepad-scrollback-reply-{}-{}.sock",
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
