//! Bounded JSON-line requests to Herdr's local API socket. Each call owns its connection;
//! a timed-out write is never retried (it may already have reached the pane).
use serde_json::{json, Value};
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::{ffi::OsStrExt, net::UnixStream};
use std::path::Path;
use std::time::{Duration, Instant};

const DEADLINE: Duration = Duration::from_millis(300);
const LIMIT: usize = 1024 * 1024;
const ID: &str = "gamepad-scroll";

pub fn call(socket: &Path, method: &str, params: Value) -> Result<Value, String> {
    call_if_current(socket, method, params, &|| true)?
        .ok_or_else(|| "Herdr request cancelled".into())
}

/// Cancel before dispatch when a gesture ends. Once the complete request is written,
/// read its actual result: it may already have affected the pane and must not be retried.
pub fn call_if_current(
    socket: &Path,
    method: &str,
    params: Value,
    still_current: &dyn Fn() -> bool,
) -> Result<Option<Value>, String> {
    exchange(socket, method, params, still_current, DEADLINE)
}

fn exchange(
    socket: &Path,
    method: &str,
    params: Value,
    still_current: &dyn Fn() -> bool,
    budget: Duration,
) -> Result<Option<Value>, String> {
    if !still_current() {
        return Ok(None);
    }
    let deadline = Instant::now() + budget;
    let mut stream = connect(socket, deadline)?;
    let mut request = serde_json::to_vec(&json!({"id":ID,"method":method,"params":params}))
        .map_err(|e| e.to_string())?;
    request.push(b'\n');
    let mut remaining = request.as_slice();
    while !remaining.is_empty() {
        check_deadline(deadline)?;
        if !still_current() {
            return Ok(None);
        }
        match stream.write(remaining) {
            Ok(0) => return Err("Herdr socket closed during write".into()),
            Ok(n) => remaining = &remaining[n..],
            Err(e) if retryable(&e) => pause(deadline)?,
            Err(e) => return Err(format!("Herdr socket write: {e}")),
        }
    }
    let mut reply = Vec::new();
    let mut chunk = [0; 8192];
    loop {
        check_deadline(deadline)?;
        match stream.read(&mut chunk) {
            Ok(0) => return Err("Herdr socket closed before reply".into()),
            Ok(n) => {
                let end = chunk[..n].iter().position(|b| *b == b'\n');
                let count = end.unwrap_or(n);
                if reply.len() + count > LIMIT {
                    return Err("Herdr reply too large".into());
                }
                reply.extend_from_slice(&chunk[..count]);
                if end.is_some() {
                    return decode(&reply, method).map(Some);
                }
            }
            Err(e) if retryable(&e) => pause(deadline)?,
            Err(e) => return Err(format!("Herdr socket read: {e}")),
        }
    }
}

fn decode(bytes: &[u8], method: &str) -> Result<Value, String> {
    let value: Value =
        serde_json::from_slice(bytes).map_err(|e| format!("Invalid Herdr reply: {e}"))?;
    if value.get("id").and_then(Value::as_str) != Some(ID) {
        return Err("Herdr reply ID mismatch".into());
    }
    if let Some(error) = value.get("error") {
        return Err(format!("Herdr refused {method}: {error}"));
    }
    value
        .get("result")
        .filter(|v| v.is_object())
        .cloned()
        .ok_or_else(|| "Missing Herdr result".into())
}

fn retryable(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
    )
}
fn check_deadline(deadline: Instant) -> Result<(), String> {
    if Instant::now() >= deadline {
        Err("Herdr scroll request timed out".into())
    } else {
        Ok(())
    }
}
fn pause(deadline: Instant) -> Result<(), String> {
    check_deadline(deadline)?;
    std::thread::sleep(Duration::from_millis(2));
    Ok(())
}

fn connect(path: &Path, deadline: Instant) -> Result<UnixStream, String> {
    let bytes = path.as_os_str().as_bytes();
    // sockaddr_un is a C POD; zero initialization terminates the pathname.
    let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    if bytes.is_empty() || bytes.contains(&0) || bytes.len() >= address.sun_path.len() {
        return Err("Invalid Herdr socket path".into());
    }
    address.sun_family = libc::AF_UNIX as libc::sa_family_t;
    for (dest, source) in address.sun_path.iter_mut().zip(bytes) {
        *dest = *source as libc::c_char;
    }
    // Own the descriptor immediately so every error path closes it.
    let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
    if fd < 0 {
        return Err(format!("Herdr socket: {}", io::Error::last_os_error()));
    }
    let stream = unsafe { UnixStream::from_raw_fd(fd) };
    // Prevent the socket from leaking into the daemon's CLI/recorder children.
    if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
        return Err(format!(
            "Herdr socket flags: {}",
            io::Error::last_os_error()
        ));
    }
    stream
        .set_nonblocking(true)
        .map_err(|e| format!("Herdr socket mode: {e}"))?;
    let result = unsafe {
        libc::connect(
            fd,
            (&address as *const libc::sockaddr_un).cast(),
            std::mem::size_of_val(&address) as libc::socklen_t,
        )
    };
    if result == 0 {
        return Ok(stream);
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() != Some(libc::EINPROGRESS) {
        return Err(format!("Herdr socket connect: {error}"));
    }
    loop {
        check_deadline(deadline)?;
        let mut poll = libc::pollfd {
            fd: stream.as_raw_fd(),
            events: libc::POLLOUT,
            revents: 0,
        };
        let result = unsafe { libc::poll(&mut poll, 1, 2) };
        if result < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(format!("Herdr socket poll: {error}"));
        }
        if result == 0 {
            continue;
        }
        if let Some(error) = stream.take_error().map_err(|e| e.to_string())? {
            return Err(format!("Herdr socket connect: {error}"));
        }
        return Ok(stream);
    }
}

#[cfg(test)]
pub(crate) fn call_with_budget(
    socket: &Path,
    method: &str,
    params: Value,
    budget: Duration,
) -> Result<Value, String> {
    exchange(socket, method, params, &|| true, budget)?
        .ok_or_else(|| "Herdr request cancelled".into())
}
