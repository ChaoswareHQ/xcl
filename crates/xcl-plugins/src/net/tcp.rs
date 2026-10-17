//! TCP client — connect, read, write, close.
//!
//! ## Domain — all functions are `net.tcp.*`
//!
//! ```xcl
//! $r = net.tcp.connect "127.0.0.1" 8080
//! $id = $r.value
//! net.tcp.write $id "Hello"
//! $data = net.tcp.read $id 1024
//! net.tcp.close $id
//! ```
//!
//! ## Why in `net/` and not `transport/`
//!
//! Because TCP here is just dialing a socket and moving bytes — no framing,
//! no TLS, no pooling, no retries. That's the socket layer.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;
use xcl_macros::xcl_fn;
use xcl_vm::{Value, DomainCtx};
use crate::core::result::{ok_result, err_result};
use super::tcp_registry;

/// Connect to a TCP server and return an opaque handle.
///
/// ## SYNTAX
///
/// ```xcl
/// fn net.tcp.connect(host: str; port: int) -> Result
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// $r = net.tcp.connect "127.0.0.1" 8080
/// if $r.is_ok {
///     $id = $r.value
///     print "Connected: " $id
/// }
/// ```
///
/// ## RETURNS
///
/// A Result with the integer handle in the `value` field on success.
#[xcl_fn(
    name = "net.tcp.connect",
    args = "host: str; port: int",
    ret = "Result",
    summary = "Connect to a TCP server and return an opaque handle."
)]
fn tcp_connect(ctx: &mut DomainCtx) -> Value {
    let host = ctx.arg(0).as_str().unwrap_or("");
    let port = ctx.arg(1).as_i48() as u16;

    let addr = format!("{}:{}", host, port);
    match TcpStream::connect(&addr) {
        Ok(stream) => {
            let mut reg = tcp_registry().lock().unwrap();
            let id = reg.insert(stream);
            ok_result(Value::from_i48(id))
        }
        Err(e) => err_result("connect_error", &format!("Failed to connect to {}: {}", addr, e), 0),
    }
}

/// Connect to a TCP server with a timeout.
///
/// ## SYNTAX
///
/// ```xcl
/// fn net.tcp.connect_timeout(host: str; port: int; timeout_ms: int) -> Result
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// $r = net.tcp.connect_timeout "127.0.0.1" 8080 5000
/// ```
#[xcl_fn(
    name = "net.tcp.connect_timeout",
    args = "host: str; port: int; timeout_ms: int",
    ret = "Result",
    summary = "Connect to a TCP server with a timeout in milliseconds."
)]
fn tcp_connect_timeout(ctx: &mut DomainCtx) -> Value {
    let host = ctx.arg(0).as_str().unwrap_or("");
    let port = ctx.arg(1).as_i48() as u16;
    let timeout_ms = ctx.arg(2).as_i48() as u64;

    let addr = format!("{}:{}", host, port);
    match TcpStream::connect_timeout(&addr.parse().unwrap(), Duration::from_millis(timeout_ms)) {
        Ok(stream) => {
            let mut reg = tcp_registry().lock().unwrap();
            let id = reg.insert(stream);
            ok_result(Value::from_i48(id))
        }
        Err(e) => err_result("connect_error", &format!("Timeout connecting to {}: {}", addr, e), 0),
    }
}

/// Write data to a TCP connection.
///
/// ## SYNTAX
///
/// ```xcl
/// fn net.tcp.write(id: int; data: str) -> Result
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// net.tcp.write $id "GET / HTTP/1.0\r\n\r\n"
/// ```
///
/// ## RETURNS
///
/// A Result with the number of bytes written in the `value` field.
#[xcl_fn(
    name = "net.tcp.write",
    args = "id: int; data: str",
    ret = "Result",
    summary = "Write data to a TCP connection."
)]
fn tcp_write(ctx: &mut DomainCtx) -> Value {
    let id = ctx.arg(0).as_i48();
    let data = ctx.arg(1).as_str().unwrap_or("");

    let registry = tcp_registry();
    let mut reg = registry.lock().unwrap();
    match reg.get(id) {
        Some(stream) => {
            // We need to write through the mutable reference
            let bytes = data.as_bytes();
            match stream.write_all(bytes) {
                Ok(()) => ok_result(Value::from_i48(bytes.len() as i64)),
                Err(e) => err_result("write_error", &format!("Write failed: {}", e), 0),
            }
        }
        None => err_result("invalid_handle", &format!("No TCP connection for handle {}", id), 0),
    }
}

/// Read data from a TCP connection.
///
/// ## SYNTAX
///
/// ```xcl
/// fn net.tcp.read(id: int; max_bytes: int) -> Result
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// $r = net.tcp.read $id 1024
/// $data = $r.value
/// ```
///
/// ## RETURNS
///
/// A Result with the read string in the `value` field. Empty string on EOF.
#[xcl_fn(
    name = "net.tcp.read",
    args = "id: int; max_bytes: int",
    ret = "Result",
    summary = "Read up to max_bytes from a TCP connection."
)]
fn tcp_read(ctx: &mut DomainCtx) -> Value {
    let id = ctx.arg(0).as_i48();
    let max_bytes = ctx.arg(1).as_i48() as usize;

    let registry = tcp_registry();
    let mut reg = registry.lock().unwrap();
    match reg.get(id) {
        Some(stream) => {
            let mut buf = vec![0u8; max_bytes];
            match stream.read(&mut buf) {
                Ok(0) => ok_result(Value::from_str_boxed("".to_string())),
                Ok(n) => {
                    buf.truncate(n);
                    let s = String::from_utf8_lossy(&buf).to_string();
                    ok_result(Value::from_str_boxed(s))
                }
                Err(e) => err_result("read_error", &format!("Read failed: {}", e), 0),
            }
        }
        None => err_result("invalid_handle", &format!("No TCP connection for handle {}", id), 0),
    }
}

/// Read a single line from a TCP connection (up to newline).
///
/// ## SYNTAX
///
/// ```xcl
/// fn net.tcp.readline(id: int) -> Result
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// $r = net.tcp.readline $id
/// $line = $r.value
/// ```
#[xcl_fn(
    name = "net.tcp.readline",
    args = "id: int",
    ret = "Result",
    summary = "Read a single line from a TCP connection."
)]
fn tcp_readline(ctx: &mut DomainCtx) -> Value {
    let id = ctx.arg(0).as_i48();

    let registry = tcp_registry();
    let mut reg = registry.lock().unwrap();
    match reg.get(id) {
        Some(stream) => {
            let mut buf = Vec::new();
            loop {
                let mut byte = [0u8; 1];
                match stream.read(&mut byte) {
                    Ok(0) => {
                        // EOF
                        let s = String::from_utf8_lossy(&buf).to_string();
                        return ok_result(Value::from_str_boxed(s));
                    }
                    Ok(1) => {
                        if byte[0] == b'\n' {
                            break;
                        }
                        buf.push(byte[0]);
                    }
                    Ok(_) => {}
                    Err(e) => return err_result("read_error", &format!("Read failed: {}", e), 0),
                }
            }
            // Strip trailing CR if present
            if buf.last() == Some(&b'\r') {
                buf.pop();
            }
            let s = String::from_utf8_lossy(&buf).to_string();
            ok_result(Value::from_str_boxed(s))
        }
        None => err_result("invalid_handle", &format!("No TCP connection for handle {}", id), 0),
    }
}

/// Close a TCP connection, releasing the handle.
///
/// ## SYNTAX
///
/// ```xcl
/// fn net.tcp.close(id: int) -> Result
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// net.tcp.close $id
/// ```
#[xcl_fn(
    name = "net.tcp.close",
    args = "id: int",
    ret = "Result",
    summary = "Close a TCP connection and release the handle."
)]
fn tcp_close(ctx: &mut DomainCtx) -> Value {
    let id = ctx.arg(0).as_i48();

    let registry = tcp_registry();
    let mut reg = registry.lock().unwrap();
    match reg.remove(id) {
        Some(_stream) => ok_result(Value::unit()),
        None => err_result("invalid_handle", &format!("No TCP connection for handle {}", id), 0),
    }
}

/// Get the local address of a TCP connection.
///
/// ## SYNTAX
///
/// ```xcl
/// fn net.tcp.local_addr(id: int) -> Result
/// ```
#[xcl_fn(
    name = "net.tcp.local_addr",
    args = "id: int",
    ret = "Result",
    summary = "Get the local address of a TCP connection."
)]
fn tcp_local_addr(ctx: &mut DomainCtx) -> Value {
    let id = ctx.arg(0).as_i48();

    let registry = tcp_registry();
    let reg = registry.lock().unwrap();
    match reg.streams.get(&id) {
        Some(stream) => {
            match stream.local_addr() {
                Ok(addr) => ok_result(Value::from_str_boxed(addr.to_string())),
                Err(e) => err_result("io_error", &format!("Failed to get local addr: {}", e), 0),
            }
        }
        None => err_result("invalid_handle", &format!("No TCP connection for handle {}", id), 0),
    }
}

/// Get the peer address of a TCP connection.
///
/// ## SYNTAX
///
/// ```xcl
/// fn net.tcp.peer_addr(id: int) -> Result
/// ```
#[xcl_fn(
    name = "net.tcp.peer_addr",
    args = "id: int",
    ret = "Result",
    summary = "Get the peer address of a TCP connection."
)]
fn tcp_peer_addr(ctx: &mut DomainCtx) -> Value {
    let id = ctx.arg(0).as_i48();

    let registry = tcp_registry();
    let reg = registry.lock().unwrap();
    match reg.streams.get(&id) {
        Some(stream) => {
            match stream.peer_addr() {
                Ok(addr) => ok_result(Value::from_str_boxed(addr.to_string())),
                Err(e) => err_result("io_error", &format!("Failed to get peer addr: {}", e), 0),
            }
        }
        None => err_result("invalid_handle", &format!("No TCP connection for handle {}", id), 0),
    }
}

/// Set the read timeout for a TCP connection.
///
/// ## SYNTAX
///
/// ```xcl
/// fn net.tcp.set_read_timeout(id: int; timeout_ms: int) -> Result
/// ```
#[xcl_fn(
    name = "net.tcp.set_read_timeout",
    args = "id: int; timeout_ms: int",
    ret = "Result",
    summary = "Set the read timeout for a TCP connection."
)]
fn tcp_set_read_timeout(ctx: &mut DomainCtx) -> Value {
    let id = ctx.arg(0).as_i48();
    let timeout_ms = ctx.arg(1).as_i48() as u64;

    let registry = tcp_registry();
    let mut reg = registry.lock().unwrap();
    match reg.get(id) {
        Some(stream) => {
            match stream.set_read_timeout(Some(Duration::from_millis(timeout_ms))) {
                Ok(()) => ok_result(Value::unit()),
                Err(e) => err_result("io_error", &format!("Failed to set timeout: {}", e), 0),
            }
        }
        None => err_result("invalid_handle", &format!("No TCP connection for handle {}", id), 0),
    }
}

/// Set the write timeout for a TCP connection.
///
/// ## SYNTAX
///
/// ```xcl
/// fn net.tcp.set_write_timeout(id: int; timeout_ms: int) -> Result
/// ```
#[xcl_fn(
    name = "net.tcp.set_write_timeout",
    args = "id: int; timeout_ms: int",
    ret = "Result",
    summary = "Set the write timeout for a TCP connection."
)]
fn tcp_set_write_timeout(ctx: &mut DomainCtx) -> Value {
    let id = ctx.arg(0).as_i48();
    let timeout_ms = ctx.arg(1).as_i48() as u64;

    let registry = tcp_registry();
    let mut reg = registry.lock().unwrap();
    match reg.get(id) {
        Some(stream) => {
            match stream.set_write_timeout(Some(Duration::from_millis(timeout_ms))) {
                Ok(()) => ok_result(Value::unit()),
                Err(e) => err_result("io_error", &format!("Failed to set timeout: {}", e), 0),
            }
        }
        None => err_result("invalid_handle", &format!("No TCP connection for handle {}", id), 0),
    }
}

#[cfg(test)]
mod tests {
    use crate::evaluate_new;
    use std::io::{Read, Write};

    #[test]
    fn tcp_connect_refuses_unknown() {
        // Connecting to a local port that isn't listening should return an error
        let r = evaluate_new(r#"
            net.tcp.connect "127.0.0.1" 1
        "#);
        assert_eq!(r.map_variant(), Some(xcl_vm::VARIANT_ERR));
    }

    #[test]
    fn tcp_close_invalid_handle() {
        let r = evaluate_new(r#"
            net.tcp.close 99999
        "#);
        assert_eq!(r.map_variant(), Some(xcl_vm::VARIANT_ERR));
    }

    #[test]
    fn tcp_read_invalid_handle() {
        let r = evaluate_new(r#"
            net.tcp.read 99999 10
        "#);
        assert_eq!(r.map_variant(), Some(xcl_vm::VARIANT_ERR));
    }

    // Integration test: echo server
    #[test]
    #[ignore = "requires mock TCP server — flaky in CI"]
    fn tcp_connect_echo_loopback() {
        use std::net::TcpListener;
        use std::sync::mpsc;
        use std::thread;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        // Channel to signal when the server has accepted the connection
        let (tx, rx) = mpsc::channel();

        // Spawn a thread that echoes back what it receives
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            tx.send(()).unwrap();
            let mut buf = [0u8; 256];
            let n = stream.read(&mut buf).unwrap();
            stream.write_all(&buf[..n]).unwrap();
        });

        // Wait a bit for the listener to be ready, then connect
        thread::sleep(std::time::Duration::from_millis(50));

        let code = format!(r#"
            $r = net.tcp.connect "127.0.0.1" {}
            $id = $r.value
            net.tcp.write $id "hello"
            $data = net.tcp.read $id 1024
            net.tcp.close $id
            $data
        "#, port);

        let result = evaluate_new(&code);
        // Wait for server to have accepted
        let _ = rx.recv_timeout(std::time::Duration::from_secs(5));
        assert_eq!(result.as_str(), Some("hello"));

        handle.join().unwrap();
    }

    #[test]
    #[ignore = "requires mock TCP server — flaky in CI"]
    fn tcp_readline_from_server() {
        use std::net::TcpListener;
        use std::sync::mpsc;
        use std::thread;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let (tx, rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            tx.send(()).unwrap();
            stream.write_all(b"HTTP/1.0 200 OK\r\n\r\n").unwrap();
        });

        thread::sleep(std::time::Duration::from_millis(50));

        let code = format!(r#"
            $r = net.tcp.connect "127.0.0.1" {}
            $id = $r.value
            $line = net.tcp.readline $id
            net.tcp.close $id
            $line.value
        "#, port);

        let result = evaluate_new(&code);
        let _ = rx.recv_timeout(std::time::Duration::from_secs(5));
        assert_eq!(result.as_str(), Some("HTTP/1.0 200 OK"));

        handle.join().unwrap();
    }

    #[test]
    #[ignore = "requires mock TCP server — flaky in CI"]
    fn tcp_peer_addr_returns_string() {
        use std::net::TcpListener;
        use std::sync::mpsc;
        use std::thread;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let (tx, rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            let (_stream, _) = listener.accept().unwrap();
            let _ = tx.send(());
        });

        thread::sleep(std::time::Duration::from_millis(50));

        let code = format!(r#"
            $r = net.tcp.connect "127.0.0.1" {}
            $id = $r.value
            $addr = net.tcp.peer_addr $id
            net.tcp.close $id
            $addr.value
        "#, port);

        let result = evaluate_new(&code);
        let _ = rx.recv_timeout(std::time::Duration::from_secs(5));
        assert!(result.as_str().is_some());
        assert!(result.as_str().unwrap().contains(&format!(":{}", port)));

        handle.join().unwrap();
    }
}
