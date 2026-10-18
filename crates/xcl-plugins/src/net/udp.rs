//! UDP socket — bind, send, receive, close.
//!
//! ## Domain — all functions are `net.udp.*`
//!
//! ```xcl
//! $r = net.udp.bind "127.0.0.1" 0
//! $id = $r.value
//! net.udp.send $id "127.0.0.1" 9999 "hello"
//! $r = net.udp.recv $id 1024
//! $data = $r.value
//! net.udp.close $id
//! ```
//!
//! ## Why in `net/` and not `transport/`
//!
//! Because UDP here is just creating a datagram socket and sending/receiving
//! packets — no framing, no reliability, no pooling. That's the socket layer.

use std::collections::HashMap;
use std::net::UdpSocket;
use std::sync::{Mutex, OnceLock};
use xcl_macros::xcl_fn;
use xcl_vm::{Value, DomainCtx};
use crate::core::result::{ok_result, err_result};

static UDP_REGISTRY: OnceLock<Mutex<UdpHandleRegistry>> = OnceLock::new();

struct UdpHandleRegistry {
    next_id: i64,
    sockets: HashMap<i64, UdpSocket>,
}

impl UdpHandleRegistry {
    fn new() -> Self {
        Self {
            next_id: 1000,
            sockets: HashMap::new(),
        }
    }

    fn insert(&mut self, socket: UdpSocket) -> i64 {
        let id = self.next_id;
        self.next_id += 1;
        self.sockets.insert(id, socket);
        id
    }

    fn get(&mut self, id: i64) -> Option<&mut UdpSocket> {
        self.sockets.get_mut(&id)
    }

    fn remove(&mut self, id: i64) -> Option<UdpSocket> {
        self.sockets.remove(&id)
    }
}

fn udp_registry() -> &'static Mutex<UdpHandleRegistry> {
    UDP_REGISTRY.get_or_init(|| Mutex::new(UdpHandleRegistry::new()))
}

/// Bind a UDP socket to a local address.
///
/// ## SYNTAX
///
/// ```xcl
/// fn net.udp.bind(host: str; port: int) -> Result
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// $r = net.udp.bind "127.0.0.1" 0
/// $id = $r.value
/// ```
///
/// ## RETURNS
///
/// A Result with the integer handle in the `value` field on success.
#[xcl_fn(name = "net.udp.bind")]
fn udp_bind(ctx: &mut DomainCtx) -> Value {
    let host = ctx.arg(0).as_str().unwrap_or("127.0.0.1");
    let port = ctx.arg(1).as_i48() as u16;

    let addr = format!("{}:{}", host, port);
    match UdpSocket::bind(&addr) {
        Ok(socket) => {
            let mut reg = udp_registry().lock().unwrap();
            let id = reg.insert(socket);
            ok_result(Value::from_i48(id))
        }
        Err(e) => err_result("bind_error", &format!("Failed to bind UDP to {}: {}", addr, e), 0),
    }
}

/// Send data to a remote address via UDP.
///
/// ## SYNTAX
///
/// ```xcl
/// fn net.udp.send(id: int; host: str; port: int; data: str) -> Result
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// net.udp.send $id "127.0.0.1" 9999 "hello"
/// ```
///
/// ## RETURNS
///
/// A Result with the number of bytes sent in the `value` field.
#[xcl_fn(name = "net.udp.send")]
fn udp_send(ctx: &mut DomainCtx) -> Value {
    let id = ctx.arg(0).as_i48();
    let host = ctx.arg(1).as_str().unwrap_or("");
    let port = ctx.arg(2).as_i48() as u16;
    let data = ctx.arg(3).as_str().unwrap_or("");

    let addr = format!("{}:{}", host, port);
    let registry = udp_registry();
    let mut reg = registry.lock().unwrap();
    match reg.get(id) {
        Some(socket) => {
            match socket.send_to(data.as_bytes(), &addr) {
                Ok(n) => ok_result(Value::from_i48(n as i64)),
                Err(e) => err_result("send_error", &format!("UDP send failed: {}", e), 0),
            }
        }
        None => err_result("invalid_handle", &format!("No UDP socket for handle {}", id), 0),
    }
}

/// Receive data from a UDP socket.
///
/// ## SYNTAX
///
/// ```xcl
/// fn net.udp.recv(id: int; max_bytes: int) -> Result
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// $r = net.udp.recv $id 1024
/// $data = $r.value.data
/// $from = $r.value.from
/// ```
///
/// ## RETURNS
///
/// A Result with a map `{data: str, from: str}` in the `value` field.
#[xcl_fn(name = "net.udp.recv")]
fn udp_recv(ctx: &mut DomainCtx) -> Value {
    let id = ctx.arg(0).as_i48();
    let max_bytes = ctx.arg(1).as_i48() as usize;

    let registry = udp_registry();
    let mut reg = registry.lock().unwrap();
    match reg.get(id) {
        Some(socket) => {
            let mut buf = vec![0u8; max_bytes];
            match socket.recv_from(&mut buf) {
                Ok((n, addr)) => {
                    buf.truncate(n);
                    let data = String::from_utf8_lossy(&buf).to_string();
                    let mut map = HashMap::new();
                    map.insert("data".to_string(), Value::from_str_boxed(data));
                    map.insert("from".to_string(), Value::from_str_boxed(addr.to_string()));
                    ok_result(Value::from_map_boxed(map))
                }
                Err(e) => err_result("recv_error", &format!("UDP recv failed: {}", e), 0),
            }
        }
        None => err_result("invalid_handle", &format!("No UDP socket for handle {}", id), 0),
    }
}

/// Close a UDP socket, releasing the handle.
///
/// ## SYNTAX
///
/// ```xcl
/// fn net.udp.close(id: int) -> Result
/// ```
#[xcl_fn(name = "net.udp.close")]
fn udp_close(ctx: &mut DomainCtx) -> Value {
    let id = ctx.arg(0).as_i48();

    let registry = udp_registry();
    let mut reg = registry.lock().unwrap();
    match reg.remove(id) {
        Some(_socket) => ok_result(Value::unit()),
        None => err_result("invalid_handle", &format!("No UDP socket for handle {}", id), 0),
    }
}

/// Get the local address of a UDP socket.
///
/// ## SYNTAX
///
/// ```xcl
/// fn net.udp.local_addr(id: int) -> Result
/// ```
#[xcl_fn(name = "net.udp.local_addr")]
fn udp_local_addr(ctx: &mut DomainCtx) -> Value {
    let id = ctx.arg(0).as_i48();

    let registry = udp_registry();
    let reg = registry.lock().unwrap();
    match reg.sockets.get(&id) {
        Some(socket) => {
            match socket.local_addr() {
                Ok(addr) => ok_result(Value::from_str_boxed(addr.to_string())),
                Err(e) => err_result("io_error", &format!("Failed to get local addr: {}", e), 0),
            }
        }
        None => err_result("invalid_handle", &format!("No UDP socket for handle {}", id), 0),
    }
}

#[cfg(test)]
mod tests {
    use crate::evaluate_new;

    #[test]
    fn udp_bind_returns_handle() {
        let r = evaluate_new(r#"
            net.udp.bind "127.0.0.1" 0
        "#);
        assert_eq!(r.map_variant(), Some(xcl_vm::VARIANT_OK));
    }

    #[test]
    fn udp_close_invalid_handle() {
        let r = evaluate_new(r#"
            net.udp.close 99999
        "#);
        assert_eq!(r.map_variant(), Some(xcl_vm::VARIANT_ERR));
    }

    #[test]
    fn udp_send_recv_loopback() {
        use std::thread;

        // Bind a receiver socket first using std::net directly
        let receiver = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = receiver.local_addr().unwrap().port();

        let receiver_addr = receiver.local_addr().unwrap();
        let handle = thread::spawn(move || {
            let mut buf = [0u8; 256];
            let (n, _addr) = receiver.recv_from(&mut buf).unwrap();
            // Echo back
            receiver.send_to(&buf[..n], &receiver_addr).unwrap();
        });

        thread::sleep(std::time::Duration::from_millis(50));

        let code = format!(r#"
            $r = net.udp.bind "127.0.0.1" 0
            $id = $r.value
            net.udp.send $id "127.0.0.1" {} "hello"
            $recv = net.udp.recv $id 1024
            net.udp.close $id
            $recv.value.data
        "#, port);

        let result = evaluate_new(&code);
        assert_eq!(result.as_str(), Some("hello"));

        handle.join().unwrap();
    }

    #[test]
    #[ignore = "requires UDP socket — flaky in CI"]
    fn udp_local_addr_returns_string() {
        let r = evaluate_new(r#"
            $r = net.udp.bind "127.0.0.1" 0
            $id = $r.value
            $addr = net.udp.local_addr $id
            net.udp.close $id
            $addr.value
        "#);
        assert!(r.as_str().is_some());
        assert!(r.as_str().unwrap().starts_with("127.0.0.1:"));
    }
}
