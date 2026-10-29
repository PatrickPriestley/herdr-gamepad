use crate::scroll_rpc::call;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Server {
    path: PathBuf,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Server {
    fn new(reply: impl FnOnce(Value) -> Vec<u8> + Send + 'static) -> Self {
        let path = std::env::temp_dir().join(format!(
            "scroll-rpc-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let listener = UnixListener::bind(&path).unwrap();
        listener.set_nonblocking(true).unwrap();
        let thread = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(2);
            let mut stream = loop {
                match listener.accept() {
                    Ok((s, _)) => break s,
                    Err(e)
                        if e.kind() == std::io::ErrorKind::WouldBlock
                            && Instant::now() < deadline =>
                    {
                        std::thread::sleep(Duration::from_millis(2))
                    }
                    _ => return,
                }
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(1)))
                .unwrap();
            let mut line = String::new();
            BufReader::new(&stream).read_line(&mut line).unwrap();
            let request = serde_json::from_str(&line).unwrap();
            let _ = stream.write_all(&reply(request));
        });
        Self {
            path,
            thread: Some(thread),
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        if let Some(thread) = self.thread.take() {
            let result = thread.join();
            if !std::thread::panicking() {
                result.unwrap();
            }
        }
        let _ = std::fs::remove_file(&self.path);
    }
}
fn response(request: Value, result: Value) -> Vec<u8> {
    format!("{}\n", json!({"id":request["id"],"result":result})).into_bytes()
}
#[test]
fn scroll_rpc_sends_exact_request_and_reads_result() {
    let server = Server::new(|request| {
        assert_eq!(request["method"], "pane.scroll_wheel");
        assert_eq!(
            request["params"],
            json!({"pane_id":"p_1","direction":"up","lines":2,"focused_only":true})
        );
        response(request, json!({"type":"ok"}))
    });
    assert_eq!(
        call(
            &server.path,
            "pane.scroll_wheel",
            json!({"pane_id":"p_1","direction":"up","lines":2,"focused_only":true})
        ),
        Ok(json!({"type":"ok"}))
    );
}
#[test]
fn scroll_rpc_rejects_errors_ids_malformed_and_eof() {
    for reply in [
        b"{\"id\":\"wrong\",\"result\":{}}\n".to_vec(),
        b"invalid\n".to_vec(),
        b"{}\n".to_vec(),
        b"".to_vec(),
        b"{\"id\":\"gamepad-scroll\",\"error\":{\"code\":\"unknown_method\",\"message\":\"unsupported\"}}\n".to_vec(),
    ] {
        let server = Server::new(move |_| reply);
        assert!(call(&server.path, "pane.list", json!({})).is_err());
    }
}
#[test]
fn scroll_rpc_bounds_silent_and_oversized_servers() {
    let server = Server::new(|_| {
        std::thread::sleep(Duration::from_millis(600));
        Vec::new()
    });
    let start = Instant::now();
    assert!(call(&server.path, "pane.list", json!({}))
        .unwrap_err()
        .contains("timed out"));
    assert!(start.elapsed() < Duration::from_millis(550));
    let server = Server::new(|_| vec![b'x'; 1024 * 1024 + 1]);
    assert!(call(&server.path, "pane.list", json!({}))
        .unwrap_err()
        .contains("too large"));
}
#[test]
fn scroll_rpc_rejects_invalid_paths_and_missing_socket() {
    assert!(call(
        std::path::Path::new("/tmp/nonexistent-gamepad-rpc.sock"),
        "pane.list",
        json!({})
    )
    .is_err());
    assert!(call(
        std::path::Path::new(&"a".repeat(200)),
        "pane.list",
        json!({})
    )
    .is_err());
    use std::os::unix::ffi::OsStrExt;
    let invalid = std::path::Path::new(std::ffi::OsStr::from_bytes(b"/tmp/a\0b"));
    assert!(call(invalid, "pane.list", json!({})).is_err());
}

#[test]
fn scroll_rpc_requires_object_result_and_preserves_error_detail() {
    for result in [json!(null), json!("ok"), json!([])] {
        let server = Server::new(move |request| response(request, result));
        assert!(call(&server.path, "pane.list", json!({}))
            .unwrap_err()
            .contains("Missing Herdr result"));
    }
    let server = Server::new(|request| {
        format!(
            "{}\n",
            json!({"id":request["id"],"error":{"code":"unknown_method","message":"not supported"}})
        )
        .into_bytes()
    });
    assert!(call(&server.path, "pane.scroll_wheel", json!({}))
        .unwrap_err()
        .contains("unknown_method"));
}

#[test]
fn scroll_rpc_accepts_large_bounded_reply() {
    let server = Server::new(|request| response(request, json!({"text":"a".repeat(900_000)})));
    assert_eq!(
        call(&server.path, "pane.list", json!({})).unwrap()["text"]
            .as_str()
            .unwrap()
            .len(),
        900_000
    );
}

#[test]
fn scroll_rpc_checks_gesture_after_connect_before_writing() {
    use crate::scroll_rpc::call_if_current;
    use std::cell::Cell;
    use std::io::Read;
    let path = std::env::temp_dir().join(format!("scroll-cancel-{}", std::process::id()));
    let listener = UnixListener::bind(&path).unwrap();
    let thread = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let mut bytes = Vec::new();
        let _ = stream.read_to_end(&mut bytes);
        bytes
    });
    let checks = Cell::new(0);
    let result = call_if_current(&path, "pane.scroll_wheel", json!({}), &|| {
        checks.set(checks.get() + 1);
        checks.get() == 1
    });
    let bytes = thread.join().unwrap();
    let _ = std::fs::remove_file(path);
    assert_eq!(result, Ok(None));
    assert!(bytes.is_empty(), "cancelled request reached server");
}

#[test]
fn scroll_rpc_cancelled_gesture_never_connects() {
    assert_eq!(
        crate::scroll_rpc::call_if_current(
            std::path::Path::new("/tmp/missing-cancelled-scroll"),
            "pane.scroll_wheel",
            json!({}),
            &|| false
        ),
        Ok(None)
    );
}

#[test]
fn scroll_rpc_keeps_the_result_after_a_request_was_sent() {
    use std::sync::{atomic::AtomicBool, Arc};
    let current = Arc::new(AtomicBool::new(true));
    let peer = current.clone();
    let server = Server::new(move |request| {
        peer.store(false, Ordering::SeqCst);
        format!(
            "{}\n",
            json!({"id":request["id"],"error":{"code":"unknown_method"}})
        )
        .into_bytes()
    });
    let result =
        crate::scroll_rpc::call_if_current(&server.path, "pane.scroll_wheel", json!({}), &|| {
            current.load(Ordering::SeqCst)
        });
    assert!(result.unwrap_err().contains("unknown_method"));
}
