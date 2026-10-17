//! Network layer — sockets, HTTPS, and TLS.
//!
//! ## Rule of thumb
//!
//! If your code opens a socket and moves bytes, it belongs here.
//! If it speaks a protocol (FTP verbs, SMTP DATA phase), it belongs in `protocol/`.
//!
//! ## Design
//!
//! Every connection is identified by an opaque integer handle. The handle maps
//! to a `TcpStream` or `UdpSocket` stored in a global registry. Functions return
//! `Result` values with the handle in the `value` field on success.
//!
//! ## Example
//!
//! ```xcl
//! $r = net.tcp.connect "127.0.0.1" 8080
//! $id = $r.value
//! net.tcp.write $id "GET / HTTP/1.0\r\n\r\n"
//! $data = net.tcp.read $id 1024
//! net.tcp.close $id
//! ```

pub mod tcp;
pub mod udp;
pub mod http;
pub mod dns;
pub mod tls;

use std::collections::HashMap;
use std::net::TcpStream;
use std::sync::{Mutex, OnceLock};

/// Global registry mapping integer handles to TcpStream connections.
static TCP_REGISTRY: OnceLock<Mutex<TcpHandleRegistry>> = OnceLock::new();

pub struct TcpHandleRegistry {
    pub next_id: i64,
    pub streams: HashMap<i64, TcpStream>,
}

impl TcpHandleRegistry {
    fn new() -> Self {
        Self {
            next_id: 1,
            streams: HashMap::new(),
        }
    }

    pub fn insert(&mut self, stream: TcpStream) -> i64 {
        let id = self.next_id;
        self.next_id += 1;
        self.streams.insert(id, stream);
        id
    }

    pub fn get(&mut self, id: i64) -> Option<&mut TcpStream> {
        self.streams.get_mut(&id)
    }

    pub fn remove(&mut self, id: i64) -> Option<TcpStream> {
        self.streams.remove(&id)
    }
}

pub fn tcp_registry() -> &'static Mutex<TcpHandleRegistry> {
    TCP_REGISTRY.get_or_init(|| Mutex::new(TcpHandleRegistry::new()))
}
