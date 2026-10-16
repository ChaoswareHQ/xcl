//! System and OS kernel interface for xcl-std.
//!
//! Provides low-level access to operating system functionality including
//! syscalls, process management, threads, signals, time, file descriptors,
//! sockets, and environment variables.
//!
//! Platform-specific implementations are in the `os/` subdirectory.

pub mod syscall;
pub mod process;
pub mod thread;
pub mod signal;
pub mod time;
pub mod fs;
pub mod socket;
pub mod env;
pub mod os;
