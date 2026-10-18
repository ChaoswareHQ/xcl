//! Raw socket operations for xcl-plugins.
//!
//! Provides low-level socket creation, binding, listening, and I/O.

use xcl_macros::xcl_fn;
use xcl_vm::Value;

/// Socket domain constants.
const AF_INET: i32 = 2;
const AF_INET6: i32 = 10;
const AF_UNIX: i32 = 1;

/// Socket type constants.
const SOCK_STREAM: i32 = 1;
const SOCK_DGRAM: i32 = 2;

/// Create a new socket.
///
/// # Arguments
/// - `domain` (int): Address family (AF_INET=2, AF_INET6=10, AF_UNIX=1)
/// - `socket_type` (int): Socket type (SOCK_STREAM=1, SOCK_DGRAM=2)
/// - `protocol` (int): Protocol (0 for default)
///
/// # Returns
/// Integer socket file descriptor
///
/// # Example
/// ```xcl
/// socket = sys.socket(2, 1, 0)  // TCP IPv4 socket
/// ```
#[xcl_fn]
pub fn socket(domain: i64, socket_type: i64, protocol: i64) -> Value {
    #[cfg(unix)]
    {
        let fd = unsafe { libc::socket(domain as i32, socket_type as i32, protocol as i32) };
        if fd < 0 {
            Value::from_str_boxed("Failed to create socket".to_string())
        } else {
            Value::from_i48(fd)
        }
    }
    #[cfg(not(unix))]
    {
        Value::from_str_boxed("Raw sockets are not available on this platform".to_string())
    }
}

/// Bind a socket to an address.
///
/// # Arguments
/// - `sockfd` (int): Socket file descriptor
/// - `addr` (str): Address string in "host:port" format
///
/// # Returns
/// Result indicating success or failure
///
/// # Example
/// ```xcl
/// sys.bind(sockfd, "0.0.0.0:8080")
/// ```
#[xcl_fn]
pub fn bind(sockfd: i64, addr: &str) -> Value {
    #[cfg(unix)]
    {
        let parts: Vec<&str> = addr.rsplitn(2, ':').collect();
        if parts.len() != 2 {
            return Value::from_str_boxed("Invalid address format, expected 'host:port'".to_string());
        }

        let port: u16 = match parts[0].parse() {
            Ok(p) => p,
            Err(_) => return Value::from_str_boxed("Invalid port number".to_string()),
        };

        let mut addr_struct: libc::sockaddr_in = unsafe { std::mem::zeroed() };
        addr_struct.sin_family = AF_INET as u16;
        addr_struct.sin_port = port.to_be();

        // Simple IP parsing for common cases
        let ip_str = parts[1];
        if ip_str == "0.0.0.0" || ip_str == "*" {
            addr_struct.sin_addr.s_addr = libc::INADDR_ANY.to_be();
        } else if ip_str == "127.0.0.1" {
            addr_struct.sin_addr.s_addr = libc::htonl(0x7f000001);
        } else {
            // Try to parse IP address
            let octets: Vec<u32> = ip_str.split('.').filter_map(|s| s.parse().ok()).collect();
            if octets.len() == 4 {
                let ip = (octets[0] << 24) | (octets[1] << 16) | (octets[2] << 8) | octets[3];
                addr_struct.sin_addr.s_addr = ip.to_be();
            } else {
                return Value::from_str_boxed("Invalid IP address".to_string());
            }
        }

        let result = unsafe {
            libc::bind(
                sockfd as i32,
                &addr_struct as *const libc::sockaddr_in as *const libc::sockaddr,
                std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
            )
        };

        if result == 0 {
            Value::from_i48(0)
        } else {
            Value::from_str_boxed("Failed to bind socket".to_string())
        }
    }
    #[cfg(not(unix))]
    {
        Value::from_str_boxed("Raw sockets are not available on this platform".to_string())
    }
}

/// Listen for incoming connections on a socket.
///
/// # Arguments
/// - `sockfd` (int): Socket file descriptor
/// - `backlog` (int): Maximum pending connections queue size
///
/// # Returns
/// Result indicating success or failure
///
/// # Example
/// ```xcl
/// sys.listen(sockfd, 128)
/// ```
#[xcl_fn]
pub fn listen(sockfd: i64, backlog: i64) -> Value {
    #[cfg(unix)]
    {
        let result = unsafe { libc::listen(sockfd as i32, backlog as i32) };
        if result == 0 {
            Value::from_i48(0)
        } else {
            Value::from_str_boxed("Failed to listen on socket".to_string())
        }
    }
    #[cfg(not(unix))]
    {
        Value::from_str_boxed("Raw sockets are not available on this platform".to_string())
    }
}

/// Accept an incoming connection on a listening socket.
///
/// # Arguments
/// - `sockfd` (int): Listening socket file descriptor
///
/// # Returns
/// Integer new socket file descriptor for the accepted connection
///
/// # Example
/// ```xcl
/// client = sys.accept(sockfd)
/// ```
#[xcl_fn]
pub fn accept(sockfd: i64) -> Value {
    #[cfg(unix)]
    {
        let mut addr: libc::sockaddr = unsafe { std::mem::zeroed() };
        let mut addr_len: libc::socklen_t = std::mem::size_of::<libc::sockaddr>() as libc::socklen_t;

        let client_fd = unsafe { libc::accept(sockfd as i32, &mut addr, &mut addr_len) };
        if client_fd < 0 {
            Value::from_str_boxed("Failed to accept connection".to_string())
        } else {
            Value::from_i48(client_fd)
        }
    }
    #[cfg(not(unix))]
    {
        Value::from_str_boxed("Raw sockets are not available on this platform".to_string())
    }
}

/// Connect a socket to a remote address.
///
/// # Arguments
/// - `sockfd` (int): Socket file descriptor
/// - `addr` (str): Address string in "host:port" format
///
/// # Returns
/// Result indicating success or failure
///
/// # Example
/// ```xcl
/// sys.connect(sockfd, "127.0.0.1:8080")
/// ```
#[xcl_fn]
pub fn connect(sockfd: i64, addr: &str) -> Value {
    #[cfg(unix)]
    {
        let parts: Vec<&str> = addr.rsplitn(2, ':').collect();
        if parts.len() != 2 {
            return Value::from_str_boxed("Invalid address format".to_string());
        }

        let port: u16 = match parts[0].parse() {
            Ok(p) => p,
            Err(_) => return Value::from_str_boxed("Invalid port number".to_string()),
        };

        let mut addr_struct: libc::sockaddr_in = unsafe { std::mem::zeroed() };
        addr_struct.sin_family = AF_INET as u16;
        addr_struct.sin_port = port.to_be();

        let ip_str = parts[1];
        if ip_str == "127.0.0.1" || ip_str == "localhost" {
            addr_struct.sin_addr.s_addr = libc::htonl(0x7f000001);
        } else {
            let octets: Vec<u32> = ip_str.split('.').filter_map(|s| s.parse().ok()).collect();
            if octets.len() == 4 {
                let ip = (octets[0] << 24) | (octets[1] << 16) | (octets[2] << 8) | octets[3];
                addr_struct.sin_addr.s_addr = ip.to_be();
            } else {
                return Value::from_str_boxed("Invalid IP address".to_string());
            }
        }

        let result = unsafe {
            libc::connect(
                sockfd as i32,
                &addr_struct as *const libc::sockaddr_in as *const libc::sockaddr,
                std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
            )
        };

        if result == 0 {
            Value::from_i48(0)
        } else {
            Value::from_str_boxed("Failed to connect".to_string())
        }
    }
    #[cfg(not(unix))]
    {
        Value::from_str_boxed("Raw sockets are not available on this platform".to_string())
    }
}

/// Send data on a socket.
///
/// # Arguments
/// - `sockfd` (int): Socket file descriptor
/// - `data` (str): Data to send
///
/// # Returns
/// Integer number of bytes sent
///
/// # Example
/// ```xcl
/// sys.send(sockfd, "Hello, World!")
/// ```
#[xcl_fn]
pub fn send(sockfd: i64, data: &str) -> Value {
    #[cfg(unix)]
    {
        let bytes_sent = unsafe {
            libc::send(
                sockfd as i32,
                data.as_ptr() as *const libc::c_void,
                data.len(),
                0,
            )
        };
        if bytes_sent < 0 {
            Value::from_str_boxed("Failed to send data".to_string())
        } else {
            Value::from_i48(bytes_sent)
        }
    }
    #[cfg(not(unix))]
    {
        Value::from_str_boxed("Raw sockets are not available on this platform".to_string())
    }
}

/// Receive data from a socket.
///
/// # Arguments
/// - `sockfd` (int): Socket file descriptor
/// - `size` (int): Maximum bytes to receive
///
/// # Returns
/// String containing received data
///
/// # Example
/// ```xcl
/// data = sys.recv(sockfd, 4096)
/// ```
#[xcl_fn]
pub fn recv(sockfd: i64, size: i64) -> Value {
    #[cfg(unix)]
    {
        let mut buf = vec![0u8; size as usize];
        let bytes_recv = unsafe {
            libc::recv(
                sockfd as i32,
                buf.as_mut_ptr() as *mut libc::c_void,
                size as usize,
                0,
            )
        };
        if bytes_recv < 0 {
            Value::from_str_boxed("Failed to receive data".to_string())
        } else {
            buf.truncate(bytes_recv as usize);
            Value::from_str_boxed(String::from_utf8_lossy(&buf).to_string())
        }
    }
    #[cfg(not(unix))]
    {
        Value::from_str_boxed("Raw sockets are not available on this platform".to_string())
    }
}

/// Close a socket.
///
/// # Arguments
/// - `sockfd` (int): Socket file descriptor
///
/// # Returns
/// Result indicating success or failure
///
/// # Example
/// ```xcl
/// sys.close_socket(sockfd)
/// ```
#[xcl_fn]
pub fn close_socket(sockfd: i64) -> Value {
    #[cfg(unix)]
    {
        let result = unsafe { libc::close(sockfd as i32) };
        if result == 0 {
            Value::from_i48(0)
        } else {
            Value::from_str_boxed("Failed to close socket".to_string())
        }
    }
    #[cfg(not(unix))]
    {
        Value::from_str_boxed("Raw sockets are not available on this platform".to_string())
    }
}

/// Set socket option.
///
/// # Arguments
/// - `sockfd` (int): Socket file descriptor
/// - `level` (int): Option level (SOL_SOCKET=1, IPPROTO_TCP=6)
/// - `option_name` (int): Option name (SO_REUSEADDR=2, TCP_NODELAY=1)
/// - `option_value` (int): Option value
///
/// # Returns
/// Result indicating success or failure
///
/// # Example
/// ```xcl
/// sys.setsockopt(sockfd, 1, 2, 1)  // SO_REUSEADDR
/// ```
#[xcl_fn]
pub fn setsockopt(sockfd: i64, level: i64, option_name: i64, option_value: i64) -> Value {
    #[cfg(unix)]
    {
        let result = unsafe {
            libc::setsockopt(
                sockfd as i32,
                level as i32,
                option_name as i32,
                &option_value as *const i64 as *const libc::c_void,
                std::mem::size_of::<i64>() as libc::socklen_t,
            )
        };
        if result == 0 {
            Value::from_i48(0)
        } else {
            Value::from_str_boxed("Failed to set socket option".to_string())
        }
    }
    #[cfg(not(unix))]
    {
        Value::from_str_boxed("Raw sockets are not available on this platform".to_string())
    }
}

/// Get socket file descriptor information.
///
/// # Arguments
/// - `sockfd` (int): Socket file descriptor
///
/// # Returns
/// Map containing socket information
///
/// # Example
/// ```xcl
/// sys.socket_info(sockfd)
/// ```
#[xcl_fn]
pub fn socket_info(sockfd: i64) -> Value {
    #[cfg(unix)]
    {
        let mut addr: libc::sockaddr_in = unsafe { std::mem::zeroed() };
        let mut addr_len: libc::socklen_t = std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t;

        let result = unsafe {
            libc::getsockname(
                sockfd as i32,
                &mut addr as *mut libc::sockaddr_in as *mut libc::sockaddr,
                &mut addr_len,
            )
        };

        let mut info = std::collections::HashMap::new();
        info.insert("fd".to_string(), Value::from_i48(sockfd));

        if result == 0 {
            let port = u16::from_be(addr.sin_port);
            info.insert("port".to_string(), Value::from_i48(port as i64));
            info.insert("family".to_string(), Value::from_i48(addr.sin_family as i64));
        }

        Value::from_map(info)
    }
    #[cfg(not(unix))]
    {
        Value::from_str_boxed("Raw sockets are not available on this platform".to_string())
    }
}
