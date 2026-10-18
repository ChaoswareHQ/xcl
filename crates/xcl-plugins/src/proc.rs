//! Process spawning and management domain.
//!
//! Provides functions for spawning external applications, waiting for them to complete,
//! and managing their lifecycle.

use std::collections::HashMap;
use xcl_macros::xcl_fn;
use xcl_vm::{Value, HandleKind, DomainCtx, errors::universal};
use crate::core::result::{ok_result, err_result};

/// A handle to a spawned child process.
///
/// Represents a running external process that can be waited on, killed, or queried.
/// Created by `proc.spawn` or `proc.run`.
///
/// ## EXAMPLES
///
/// ```xcl
/// // Spawn a process
/// $result = proc.spawn "echo" ["Hello"]
/// $process = $result.value
///
/// // Wait for it to finish
/// $status = proc.wait $process
/// io.print "Exit code: ${$status.value}"
/// ```
///
/// ## NOTES
///
/// - Process handles are opaque values; access them only through proc functions.
/// - Use `proc.wait` to wait for completion and get the exit code.
/// - Use `proc.kill` to terminate a running process.
#[derive(xcl_macros::Xcl)]
#[xcl(domain = "proc", kind = "Opaque")]
pub struct ProcessType;

/// Spawn a new child process.
///
/// Starts an external application with the given command and arguments. Returns a
/// process handle that can be used to wait for completion or kill the process.
/// The process runs asynchronously; use `proc.wait` to wait for it to finish.
///
/// ## SYNTAX
///
/// ```xcl
/// fn spawn($command: str; $args: arr) -> Result
/// ```
///
/// ## PARAMETERS
///
/// `$command`: The executable command to run (e.g., `"echo"`, `"ls"`, `"python"`).
///
/// `$args`: An array of string arguments to pass to the command.
///
/// ## EXAMPLES
///
/// ```xcl
/// # Spawn a simple command
/// $result = proc.spawn "echo" ["Hello, world!"]
/// if $result.is_ok {
///     $process = $result.value
///     $status = proc.wait $process
///     io.print "Exit code: ${$status.value}"
/// }
/// ```
///
/// ```xcl
/// # Spawn with multiple arguments
/// $result = proc.spawn "ls" ["-la", "/tmp"]
/// ```
///
/// ## INPUT TYPES
///
/// - `str`
/// - `arr`
///
/// ## OUTPUT TYPES
///
/// - `Result`
///
/// ## NOTES
///
/// - Returns `Err` if the command cannot be found or executed.
/// - The process runs asynchronously; use `proc.wait` to wait for completion.
/// - On Windows, the command is looked up in PATH.
#[xcl_fn(name = "proc.spawn", args = "command: str; args: arr", ret = "Result")]
pub fn proc_spawn(ctx: &mut DomainCtx) -> Value {
    let command = ctx.arg(0).as_str().unwrap_or("");
    let args_val = ctx.arg(1);

    let args: Vec<String> = if let Some(arr) = args_val.as_array() {
        arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect()
    } else {
        Vec::new()
    };

    match std::process::Command::new(command).args(&args).spawn() {
        Ok(child) => {
            let pid = child.id();
            ok_result(Value::from_handle(HandleKind::Process { child, pid }))
        }
        Err(e) => err_result(universal::OTHER, &format!("proc.spawn: {command}: {e}"), e.raw_os_error().map(|c| c as i64).unwrap_or(0)),
    }
}

/// Run a command and wait for it to complete.
///
/// Convenience function that spawns a process and waits for it to finish in one call.
/// Returns the exit code and captured output.
///
/// ## SYNTAX
///
/// ```xcl
/// fn run($command: str; $args: arr) -> Result
/// ```
///
/// ## PARAMETERS
///
/// `$command`: The executable command to run.
///
/// `$args`: An array of string arguments to pass to the command.
///
/// ## EXAMPLES
///
/// ```xcl
/// # Run a command and get the result
/// $result = proc.run "echo" ["Hello"]
/// if $result.is_ok {
///     $output = $result.value
///     io.print "Exit code: ${$output.exit_code}"
///     io.print "Output: ${$output.stdout}"
/// }
/// ```
///
/// ## INPUT TYPES
///
/// - `str`
/// - `arr`
///
/// ## OUTPUT TYPES
///
/// - `Result`
///
/// ## NOTES
///
/// - Blocks until the process completes.
/// - Captures stdout and stderr from the process.
/// - Returns a dict with `exit_code`, `stdout`, and `stderr` fields.
#[xcl_fn(name = "proc.run", args = "command: str; args: arr", ret = "Result")]
pub fn proc_run(ctx: &mut DomainCtx) -> Value {
    let command = ctx.arg(0).as_str().unwrap_or("");
    let args_val = ctx.arg(1);

    let args: Vec<String> = if let Some(arr) = args_val.as_array() {
        arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect()
    } else {
        Vec::new()
    };

    match std::process::Command::new(command).args(&args).output() {
        Ok(output) => {
            let mut result = HashMap::new();
            result.insert("exit_code".to_string(), Value::from_i48(output.status.code().unwrap_or(-1) as i64));
            result.insert("stdout".to_string(), Value::from_str_boxed(String::from_utf8_lossy(&output.stdout).to_string()));
            result.insert("stderr".to_string(), Value::from_str_boxed(String::from_utf8_lossy(&output.stderr).to_string()));
            ok_result(Value::from_map_boxed(result))
        }
        Err(e) => err_result(universal::OTHER, &format!("proc.run: {command}: {e}"), e.raw_os_error().map(|c| c as i64).unwrap_or(0)),
    }
}

/// Wait for a spawned process to complete.
///
/// Blocks until the given process handle's child process finishes and returns the
/// exit status.
///
/// ## SYNTAX
///
/// ```xcl
/// fn wait($process: Process) -> Result
/// ```
///
/// ## PARAMETERS
///
/// `$process`: A process handle obtained from `proc.spawn`.
///
/// ## EXAMPLES
///
/// ```xcl
/// $result = proc.spawn "sleep" ["2"]
/// $process = $result.value
/// $status = proc.wait $process
/// io.print "Exit code: ${$status.value}"
/// ```
///
/// ## INPUT TYPES
///
/// - `Process`
///
/// ## OUTPUT TYPES
///
/// - `Result`
///
/// ## NOTES
///
/// - Blocks until the process completes.
/// - Returns the exit code as an integer.
/// - If the process was killed, the exit code may be -1 or platform-specific.
#[xcl_fn(name = "proc.wait", args = "process: Process", ret = "Result")]
pub fn proc_wait(ctx: &mut DomainCtx) -> Value {
    let handle = ctx.arg(0);
    let Some(hkind) = handle.as_handle_mut() else {
        return err_result(universal::OTHER, "proc.wait: not a process handle", -1);
    };

    match hkind {
        HandleKind::Process { child, .. } => {
            match child.wait() {
                Ok(status) => {
                    let code = status.code().unwrap_or(-1) as i64;
                    ok_result(Value::from_i48(code))
                }
                Err(e) => err_result(universal::OTHER, &format!("proc.wait: {e}"), e.raw_os_error().map(|c| c as i64).unwrap_or(0)),
            }
        }
        _ => err_result(universal::OTHER, "proc.wait: handle is not a process", -1),
    }
}

/// Kill a running process.
///
/// Forcefully terminates the given process. Returns the exit status if available.
///
/// ## SYNTAX
///
/// ```xcl
/// fn kill($process: Process) -> Result
/// ```
///
/// ## PARAMETERS
///
/// `$process`: A process handle obtained from `proc.spawn`.
///
/// ## EXAMPLES
///
/// ```xcl
/// $result = proc.spawn "sleep" ["100"]
/// $process = $result.value
/// proc.kill $process
/// ```
///
/// ## INPUT TYPES
///
/// - `Process`
///
/// ## OUTPUT TYPES
///
/// - `Result`
///
/// ## NOTES
///
/// - Forcefully terminates the process using SIGKILL (Unix) or TerminateProcess (Windows).
/// - After killing, the process handle should not be reused.
#[xcl_fn(name = "proc.kill", args = "process: Process", ret = "Result")]
pub fn proc_kill(ctx: &mut DomainCtx) -> Value {
    let handle = ctx.arg(0);
    let Some(hkind) = handle.as_handle_mut() else {
        return err_result(universal::OTHER, "proc.kill: not a process handle", -1);
    };

    match hkind {
        HandleKind::Process { child, .. } => {
            match child.kill() {
                Ok(_) => ok_result(Value::unit()),
                Err(e) => err_result(universal::OTHER, &format!("proc.kill: {e}"), e.raw_os_error().map(|c| c as i64).unwrap_or(0)),
            }
        }
        _ => err_result(universal::OTHER, "proc.kill: handle is not a process", -1),
    }
}

/// Get the process ID (PID) of a spawned process.
///
/// Returns the operating system process ID of the given process handle.
///
/// ## SYNTAX
///
/// ```xcl
/// fn pid($process: Process) -> int
/// ```
///
/// ## PARAMETERS
///
/// `$process`: A process handle obtained from `proc.spawn`.
///
/// ## EXAMPLES
///
/// ```xcl
/// $result = proc.spawn "sleep" ["10"]
/// $process = $result.value
/// $pid = proc.pid $process
/// io.print "Process ID: $pid"
/// ```
///
/// ## INPUT TYPES
///
/// - `Process`
///
/// ## OUTPUT TYPES
///
/// - `int`
///
/// ## NOTES
///
/// - The PID can be used with external tools to monitor or manage the process.
#[xcl_fn(name = "proc.pid", args = "process: Process", ret = "int")]
pub fn proc_pid(ctx: &mut DomainCtx) -> Value {
    let handle = ctx.arg(0);
    let Some(hkind) = handle.as_handle() else {
        return Value::from_i48(-1);
    };

    match hkind {
        HandleKind::Process { pid, .. } => Value::from_i48(*pid as i64),
        _ => Value::from_i48(-1),
    }
}

/// Get the current process ID.
///
/// Returns the process ID of the currently running XCL program.
///
/// ## SYNTAX
///
/// ```xcl
/// fn get_pid() -> int
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// $pid = proc.get_pid
/// io.print "My PID: $pid"
/// ```
///
/// ## OUTPUT TYPES
///
/// - `int`
#[xcl_fn(name = "proc.get_pid", ret = "int")]
pub fn proc_get_pid(_ctx: &mut DomainCtx) -> Value {
    Value::from_i48(std::process::id() as i64)
}

/// Spawn a process with captured output.
///
/// Starts a process and captures its stdout and stderr, returning the output
/// without blocking. The process handle can be used to wait for completion.
///
/// ## SYNTAX
///
/// ```xcl
/// fn spawn_capture($command: str; $args: arr) -> Result
/// ```
///
/// ## PARAMETERS
///
/// `$command`: The executable command to run.
///
/// `$args`: An array of string arguments to pass to the command.
///
/// ## EXAMPLES
///
/// ```xcl
/// $result = proc.spawn_capture "echo" ["Hello"]
/// $process = $result.value
/// $status = proc.wait $process
/// ```
///
/// ## INPUT TYPES
///
/// - `str`
/// - `arr`
///
/// ## OUTPUT TYPES
///
/// - `Result`
///
/// ## NOTES
///
/// - Captures stdout and stderr using pipes.
/// - Use `proc.wait` to wait for completion.
#[xcl_fn(name = "proc.spawn_capture", args = "command: str; args: arr", ret = "Result")]
pub fn proc_spawn_capture(ctx: &mut DomainCtx) -> Value {
    let command = ctx.arg(0).as_str().unwrap_or("");
    let args_val = ctx.arg(1);

    let args: Vec<String> = if let Some(arr) = args_val.as_array() {
        arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect()
    } else {
        Vec::new()
    };

    match std::process::Command::new(command)
        .args(&args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(child) => {
            let pid = child.id();
            ok_result(Value::from_handle(HandleKind::Process { child, pid }))
        }
        Err(e) => err_result(universal::OTHER, &format!("proc.spawn_capture: {command}: {e}"), e.raw_os_error().map(|c| c as i64).unwrap_or(0)),
    }
}
