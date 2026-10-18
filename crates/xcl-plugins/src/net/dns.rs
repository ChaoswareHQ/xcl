//! DNS resolver — thin wrapper around the OS resolver.
//!
//! ## Domain — all functions are `net.dns.*`
//!
//! ```xcl
//! $r = net.dns.resolve "example.com"
//! $ip = $r.value
//! ```
//!
//! ## Why in `net/` and not `transport/`
//!
//! Because DNS here is just asking the OS to resolve a hostname — one call,
//! no state, no transport encoding. That's the socket layer.
//!
//! `transport.doh` and `transport.dot` will provide DNS-over-HTTPS and
//! DNS-over-TCP when they land — they build wire-format DNS queries and
//! send them over specific transports.

use std::collections::HashMap;
use std::net::ToSocketAddrs;
use xcl_macros::xcl_fn;
use xcl_vm::{Value, DomainCtx};
use crate::core::result::{ok_result, err_result};

/// Resolve a hostname to IP addresses using the OS resolver.
///
/// ## SYNTAX
///
/// ```xcl
/// fn net.dns.resolve(host: str) -> Result
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// $r = net.dns.resolve "example.com"
/// if $r.is_ok {
///     $ips = $r.value   # array of IP strings
///     print $ips[0]
/// }
/// ```
///
/// ## RETURNS
///
/// A Result with an array of IP address strings in the `value` field.
/// Returns the first resolved address if multiple exist.
#[xcl_fn(name = "net.dns.resolve")]
fn dns_resolve(ctx: &mut DomainCtx) -> Value {
    let host = ctx.arg(0).as_str().unwrap_or("");

    // Try to resolve using ToSocketAddrs
    // We append a port to make it a valid socket address
    let addr_str = format!("{}:80", host);
    match addr_str.to_socket_addrs() {
        Ok(addrs) => {
            let mut ips = Vec::new();
            for addr in addrs {
                ips.push(Value::from_str_boxed(addr.ip().to_string()));
            }
            if ips.is_empty() {
                err_result("not_found", &format!("No addresses found for {}", host), 0)
            } else {
                ok_result(Value::from_array_boxed(ips))
            }
        }
        Err(e) => err_result("resolve_error", &format!("Failed to resolve {}: {}", host, e), 0),
    }
}

/// Resolve a hostname to a single IP address (first result).
///
/// ## SYNTAX
///
/// ```xcl
/// fn net.dns.resolve_first(host: str) -> Result
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// $r = net.dns.resolve_first "example.com"
/// $ip = $r.value   # "93.184.216.34"
/// ```
#[xcl_fn(name = "net.dns.resolve_first")]
fn dns_resolve_first(ctx: &mut DomainCtx) -> Value {
    let host = ctx.arg(0).as_str().unwrap_or("");

    let addr_str = format!("{}:80", host);
    match addr_str.to_socket_addrs() {
        Ok(mut addrs) => {
            if let Some(addr) = addrs.next() {
                ok_result(Value::from_str_boxed(addr.ip().to_string()))
            } else {
                err_result("not_found", &format!("No addresses found for {}", host), 0)
            }
        }
        Err(e) => err_result("resolve_error", &format!("Failed to resolve {}: {}", host, e), 0),
    }
}

/// Check if a hostname resolves (returns boolean).
///
/// ## SYNTAX
///
/// ```xcl
/// fn net.dns.check(host: str) -> Result
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// $r = net.dns.check "example.com"
/// if $r.value {
///     print "host exists"
/// }
/// ```
#[xcl_fn(name = "net.dns.check")]
fn dns_check(ctx: &mut DomainCtx) -> Value {
    let host = ctx.arg(0).as_str().unwrap_or("");

    let addr_str = format!("{}:80", host);
    match addr_str.to_socket_addrs() {
        Ok(mut addrs) => {
            let exists = addrs.next().is_some();
            ok_result(Value::bool_(exists))
        }
        Err(_) => ok_result(Value::bool_(false)),
    }
}

/// Resolve a hostname and return detailed address information.
///
/// ## SYNTAX
///
/// ```xcl
/// fn net.dns.lookup(host: str) -> Result
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// $r = net.dns.lookup "example.com"
/// $info = $r.value
/// print $info.ip      # "93.184.216.34"
/// print $info.port    # 80
/// print $info.version # "ipv4" or "ipv6"
/// ```
///
/// ## RETURNS
///
/// A Result with a map `{ip: str, port: int, version: str}` in the `value` field.
#[xcl_fn(name = "net.dns.lookup")]
fn dns_lookup(ctx: &mut DomainCtx) -> Value {
    let host = ctx.arg(0).as_str().unwrap_or("");

    let addr_str = format!("{}:80", host);
    match addr_str.to_socket_addrs() {
        Ok(mut addrs) => {
            if let Some(addr) = addrs.next() {
                let version = if addr.ip().is_ipv4() { "ipv4" } else { "ipv6" };
                let mut map = HashMap::new();
                map.insert("ip".to_string(), Value::from_str_boxed(addr.ip().to_string()));
                map.insert("port".to_string(), Value::from_i48(addr.port() as i64));
                map.insert("version".to_string(), Value::from_str_boxed(version.to_string()));
                ok_result(Value::from_map_boxed(map))
            } else {
                err_result("not_found", &format!("No addresses found for {}", host), 0)
            }
        }
        Err(e) => err_result("resolve_error", &format!("Failed to resolve {}: {}", host, e), 0),
    }
}

#[cfg(test)]
mod tests {
    use crate::evaluate_new;

    #[test]
    fn dns_resolve_loopback() {
        let r = evaluate_new(r#"
            net.dns.resolve "127.0.0.1"
        "#);
        assert_eq!(r.map_variant(), Some(xcl_vm::VARIANT_OK));
        let ips = r.as_map().unwrap().get("value").unwrap().as_array().unwrap();
        assert!(!ips.is_empty(), "Should resolve 127.0.0.1");
    }

    #[test]
    fn dns_resolve_first_loopback() {
        let r = evaluate_new(r#"
            net.dns.resolve_first "127.0.0.1"
        "#);
        assert_eq!(r.map_variant(), Some(xcl_vm::VARIANT_OK));
        assert!(r.as_map().unwrap().get("value").unwrap().as_str().is_some());
    }

    #[test]
    fn dns_check_loopback() {
        let r = evaluate_new(r#"
            net.dns.check "127.0.0.1"
        "#);
        assert_eq!(r.map_variant(), Some(xcl_vm::VARIANT_OK));
        assert!(r.as_map().unwrap().get("value").unwrap().as_bool());
    }

    #[test]
    fn dns_lookup_loopback() {
        let r = evaluate_new(r#"
            net.dns.lookup "127.0.0.1"
        "#);
        assert_eq!(r.map_variant(), Some(xcl_vm::VARIANT_OK));
        let info = r.as_map().unwrap().get("value").unwrap().as_map().unwrap();
        assert!(info.contains_key("ip"));
        assert!(info.contains_key("port"));
        assert!(info.contains_key("version"));
    }

    #[test]
    fn dns_resolve_invalid_host() {
        let r = evaluate_new(r#"
            net.dns.resolve ".invalid.host.name.that.does.not.exist.xyz"
        "#);
        assert_eq!(r.map_variant(), Some(xcl_vm::VARIANT_ERR));
    }

    #[test]
    fn dns_check_invalid_host() {
        let r = evaluate_new(r#"
            net.dns.check ".invalid.host.name.that.does.not.exist.xyz"
        "#);
        assert_eq!(r.map_variant(), Some(xcl_vm::VARIANT_OK));
        assert!(!r.as_map().unwrap().get("value").unwrap().as_bool());
    }

    #[test]
    fn dns_resolve_localhost() {
        let r = evaluate_new(r#"
            net.dns.resolve "localhost"
        "#);
        assert_eq!(r.map_variant(), Some(xcl_vm::VARIANT_OK));
        let ips = r.as_map().unwrap().get("value").unwrap().as_array().unwrap();
        assert!(!ips.is_empty(), "Should resolve localhost");
    }
}
