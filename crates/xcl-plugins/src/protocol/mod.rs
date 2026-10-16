//! Application-level protocols for xcl-plugins.
//!
//! Provides higher-level protocols built on top of transport: FTP, email (SMTP/IMAP/POP3),
//! Chrome DevTools Protocol (CDP), and Model Context Protocol (MCP).

pub mod ftp;
pub mod smtp;
pub mod imap;
pub mod pop3;
pub mod cdp;
pub mod mcp;
