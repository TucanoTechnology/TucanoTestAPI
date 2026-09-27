//! Graceful shutdown over the real binary (#411): SIGTERM drains the work
//! the server was actively doing, closes idle keep-alives, and the process
//! exits on its own — no SIGKILL needed to stop this service cleanly.

#![cfg(unix)]

use std::{
    io::{Read as _, Write as _},
    net::TcpStream,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

const BINARY: &str = env!("CARGO_BIN_EXE_tucano-test");

/// Owns the test server and reaps it on every path: a graceful exit is the
/// happy path, kill-and-reap on drop is the fallback, so a failed assertion
/// never leaves an orphan holding the port.
struct Server {
    child: Child,
}

impl Server {
    fn start(directory: &std::path::Path, port: u16) -> Self {
        let mut child = Command::new(BINARY)
            .env("TUCANO_DATA_DIR", directory)
            .env("PORT", port.to_string())
            .env("TUCANO_LOG", "error")
            .env_remove("TUCANO_CONFIG_FILE")
            .stdin(Stdio::null())
            .spawn()
            .expect("start the server");
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match child.try_wait() {
                Ok(Some(status)) => panic!("the server exited before binding: {status}"),
                Err(error) => panic!("cannot poll the server: {error}"),
                Ok(None) => {
                    if TcpStream::connect(("127.0.0.1", port)).is_ok() {
                        return Self { child };
                    }
                    if Instant::now() > deadline {
                        panic!("the server never bound port {port}");
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    fn term(&self) {
        rustix::process::kill_process(
            rustix::process::Pid::from_raw(i32::try_from(self.child.id()).expect("pid fits"))
                .expect("valid pid"),
            rustix::process::Signal::TERM,
        )
        .expect("SIGTERM");
    }

    fn wait_exit(&mut self) -> std::process::ExitStatus {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if let Some(status) = self.child.try_wait().expect("wait") {
                return status;
            }
            if Instant::now() >= deadline {
                panic!("the server never exited after SIGTERM");
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Seed one project over a closing connection so the drain tests have a
/// document to address.
fn seed(port: u16) {
    let mut conn = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    let body = r#"{"name":"drained"}"#;
    conn.write_all(
        format!(
            "POST /projects HTTP/1.1\r\nhost: localhost\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        )
        .as_bytes(),
    )
    .expect("seed request");
    let mut response = String::new();
    conn.read_to_string(&mut response).expect("seed response");
    assert!(response.contains("201"), "{response}");
}

/// An in-flight request the server is actively WORKING (a write parked on
/// the advisory lock, body fully received) must still receive its real
/// answer through the drain — and that late success must reach disk.
#[test]
fn a_sigterm_drains_the_request_the_handler_is_still_working() {
    let port = 32_111;
    let directory = tempfile::TempDir::new().expect("temp dir");
    let mut server = Server::start(directory.path(), port);
    seed(port);

    // Hold the advisory lock like a busy second replica, then PUT: fully
    // received, mid-handler, parked in the 5 s deadline.
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(directory.path().join(".tucano.lock"))
        .expect("lock file");
    fs2::FileExt::lock_exclusive(&lock).expect("hold the lock");
    let mut conn = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    let body = r#"{"description":"landed after the drain started"}"#;
    conn.write_all(
        format!(
            "PUT /projects/drained.json HTTP/1.1\r\nhost: localhost\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: keep-alive\r\n\r\n{body}",
            body.len()
        )
        .as_bytes(),
    )
    .expect("contended PUT");
    std::thread::sleep(Duration::from_millis(100)); // it is now parked

    server.term();
    fs2::FileExt::unlock(&lock).expect("release");

    conn.set_read_timeout(Some(Duration::from_secs(10)))
        .expect("timeout");
    let mut response = String::new();
    conn.read_to_string(&mut response)
        .expect("drained response");
    assert!(
        response.contains("200"),
        "the parked write must complete through the drain, got:\n{response}"
    );

    let status = server.wait_exit();
    assert!(status.success(), "graceful exit must be clean: {status}");
    let stored = std::fs::read_to_string(directory.path().join("projects/drained/project.json"))
        .expect("the drained write reached disk");
    assert!(
        stored.contains("landed after the drain started"),
        "{stored}"
    );
    assert!(
        TcpStream::connect(("127.0.0.1", port)).is_err(),
        "the listener must be gone after the drain"
    );
}

/// An idle keep-alive connection must be closed, not left hanging, once the
/// drain starts: clients and orchestrators learn the instance is going.
#[test]
fn a_sigterm_closes_idle_keep_alive_connections() {
    let port = 32_112;
    let directory = tempfile::TempDir::new().expect("temp dir");
    let mut server = Server::start(directory.path(), port);

    let mut conn = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    conn.write_all(b"GET /health HTTP/1.1\r\nhost: localhost\r\nconnection: keep-alive\r\n\r\n")
        .expect("health request");
    let mut first = vec![0_u8; 4096];
    let n = conn.read(&mut first).expect("health response");
    assert!(
        String::from_utf8_lossy(&first[..n]).contains("200"),
        "health answered normally"
    );

    server.term();
    conn.set_read_timeout(Some(Duration::from_secs(10)))
        .expect("timeout");
    // Nothing more is coming on an idle drained connection: EOF, not a hang.
    let mut rest = Vec::new();
    conn.read_to_end(&mut rest)
        .expect("idle connection closes during the drain");
    assert!(
        rest.is_empty(),
        "a drained idle connection sends nothing more: {rest:?}"
    );

    let status = server.wait_exit();
    assert!(status.success(), "{status}");
}
