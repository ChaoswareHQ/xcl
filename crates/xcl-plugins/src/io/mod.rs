use std::collections::HashMap;
use std::io::{BufRead, Read, Write, Seek, SeekFrom};
use xcl_macros::{xcl_fn, xcl_type};
use xcl_vm::{Value, HandleKind, DomainCtx, errors::{universal, filesystem}};
use crate::core::result::{ok_result, err_result};

pub mod output;
pub mod input;

/// A handle to an open file.
///
/// Represents an open file that supports reading, writing, and seeking operations.
/// Created by `io.open` with a specified path and mode. Use `io.close` to release
/// the handle when finished.
///
/// ## EXAMPLES
///
/// ```xcl
/// $result = io.open "data.txt" "r"
/// $file = $result.value
/// $content = io.read_all_from $file
/// io.close $file
/// ```
///
/// ## NOTES
///
/// - File handles are opaque values; access them only through io functions.
/// - Always close file handles when done to free system resources.
#[xcl_type(name = "File", domain = "io", kind = "Opaque")]
pub struct FileType;

/// A stream handle for standard I/O.
///
/// Represents stdin, stdout, or stderr as a readable/writable stream.
/// Access via `io.stdin`, `io.stdout`, or `io.stderr`.
///
/// ## EXAMPLES
///
/// ```xcl
/// $out = io.stdout
/// $err = io.stderr
/// ```
///
/// ## NOTES
///
/// - Stream handles are pre-opened; no need to close them.
/// - Use `io.is_tty` to check if a stream is connected to a terminal.
#[xcl_type(name = "Stream", domain = "io", kind = "Opaque")]
pub struct StreamType;

/// Map std::io::ErrorKind to XCL error kinds.
fn map_error_kind(e: &std::io::Error) -> &'static str {
    use std::io::ErrorKind as K;
    match e.kind() {
        K::NotFound            => universal::NOT_FOUND,
        K::PermissionDenied    => universal::PERMISSION_DENIED,
        K::InvalidInput        => universal::INVALID_INPUT,
        K::InvalidData         => universal::INVALID_DATA,
        K::TimedOut            => universal::TIMED_OUT,
        K::Interrupted         => universal::INTERRUPTED,
        K::WouldBlock          => universal::WOULD_BLOCK,
        K::OutOfMemory         => universal::OUT_OF_MEMORY,
        K::Unsupported         => universal::UNSUPPORTED,
        K::NotADirectory       => filesystem::NOT_A_DIRECTORY,
        K::IsADirectory        => filesystem::IS_A_DIRECTORY,
        K::DirectoryNotEmpty   => filesystem::DIRECTORY_NOT_EMPTY,
        K::NotSeekable         => filesystem::NOT_SEEKABLE,
        K::ReadOnlyFilesystem  => filesystem::READ_ONLY_FILESYSTEM,
        K::InvalidFilename     => filesystem::INVALID_FILENAME,
        K::AlreadyExists       => filesystem::ALREADY_EXISTS,
        K::WriteZero           => filesystem::WRITE_ZERO,
        K::ResourceBusy        => filesystem::RESOURCE_BUSY,
        K::ExecutableFileBusy  => filesystem::EXECUTABLE_FILE_BUSY,
        K::ArgumentListTooLong => filesystem::ARGUMENT_LIST_TOO_LONG,
        K::StorageFull         => filesystem::FILESYSTEM_FULL,
        K::QuotaExceeded       => filesystem::QUOTA_EXCEEDED,
        _                      => universal::OTHER,
    }
}

#[inline]
fn make_err_io(path: &str, e: std::io::Error) -> Value {
    let code = e.raw_os_error().map(|c| c as i64).unwrap_or(0);
    err_result(map_error_kind(&e), &format!("{path}: {e}"), code)
}

/// Open a file with the specified mode.
///
/// Opens an existing file or creates a new one depending on the mode. Returns a file
/// handle that can be used for subsequent read, write, and seek operations.
///
/// ## SYNTAX
///
/// ```xcl
/// fn open($path: str; $mode: str) -> Result
/// ```
///
/// ## PARAMETERS
///
/// `$path`: The file system path to the file to open.
///
/// `$mode`: A string specifying how to open the file. Supported modes:
/// - `"r"` — Read only. File must exist.
/// - `"w"` — Write only. Creates or truncates the file.
/// - `"a"` — Append only. Creates the file if it doesn't exist.
/// - `"b"` — Create. Creates the file if it doesn't exist.
///
/// ## EXAMPLES
///
/// ```xcl
/// $result = io.open "data.txt" "r"
/// if result.is_ok $result {
///     $file = $result.value
///     $content = io.read_all_from $file
///     io.close $file
/// }
/// ```
///
/// ```xcl
/// # Open for writing (creates or overwrites)
/// $result = io.open "output.txt" "w"
/// ```
///
/// ```xcl
/// # Open for appending
/// $result = io.open "log.txt" "a"
/// ```
///
/// ## INPUT TYPES
///
/// - `str`
///
/// ## OUTPUT TYPES
///
/// - `Result`
///
/// ## NOTES
///
/// - Returns `Err` if the file cannot be opened (e.g., permission denied, path not found).
/// - Always close file handles with `io.close()` when done.
#[xcl_fn(name = "io.open", args = "path: str; mode: str", ret = "Result")]
pub fn io_open(path: &str, mode: &str) -> Value {
    let file = open_with_mode(path, mode);
    match file {
        Ok(f) => ok_result(Value::from_handle(HandleKind::File { file: f, path: path.to_string() })),
        Err(e) => make_err_io(path, e),
    }
}

fn open_with_mode(path: &str, mode: &str) -> std::io::Result<std::fs::File> {
    use std::fs::OpenOptions;
    let mut opts = OpenOptions::new();
    for c in mode.chars() {
        match c {
            'r' => { opts.read(true); }
            'w' => { opts.write(true).create(true).truncate(true); }
            'a' => { opts.write(true).create(true).append(true); }
            'b' => { opts.create(true); }
            _ => {}
        }
    }
    opts.open(path)
}

/// Read the entire contents of a file as a UTF-8 string.
///
/// Reads the complete file into memory and returns it as a UTF-8 encoded string.
/// The file is automatically opened and closed after reading.
///
/// ## SYNTAX
///
/// ```xcl
/// fn read($path: str) -> Result
/// ```
///
/// ## PARAMETERS
///
/// `$path`: The file system path to the file to read.
///
/// ## EXAMPLES
///
/// ```xcl
/// $result = io.read "config.json"
/// if result.is_ok $result {
///     $content = $result.value
///     io.print "File has ${$content.len()} characters"
/// } else {
///     io.print "Error: ${$result.error.message}"
/// }
/// ```
///
/// ## INPUT TYPES
///
/// - `str`
///
/// ## OUTPUT TYPES
///
/// - `Result`
///
/// ## NOTES
///
/// - Returns `Err` if the file doesn't exist or cannot be read.
/// - The entire file is loaded into memory; use `io.read_from()` for large files.
/// - File must be valid UTF-8; use `io.read_bytes()` for binary data.
#[xcl_fn(name = "io.read", args = "path: str", ret = "Result")]
pub fn io_read(path: &str) -> Value {
    match std::fs::read_to_string(path) {
        Ok(s) => ok_result(Value::from_str_boxed(s)),
        Err(e) => make_err_io(path, e),
    }
}

/// Read the entire contents of a file as raw bytes.
///
/// Reads the complete file into memory and returns it as an array of byte values (0-255).
/// Useful for reading binary files like images, executables, or compressed data.
///
/// ## SYNTAX
///
/// ```xcl
/// fn read_bytes($path: str) -> Result
/// ```
///
/// ## PARAMETERS
///
/// `$path`: The file system path to the file to read.
///
/// ## EXAMPLES
///
/// ```xcl
/// $result = io.read_bytes "image.png"
/// if result.is_ok $result {
///     $bytes = $result.value
///     io.print "File has ${$bytes.len()} bytes"
/// }
/// ```
///
/// ## INPUT TYPES
///
/// - `str`
///
/// ## OUTPUT TYPES
///
/// - `Result`
///
/// ## NOTES
///
/// - Returns `Err` if the file doesn't exist or cannot be read.
/// - Each byte is returned as an integer value from 0 to 255.
#[xcl_fn(name = "io.read_bytes", args = "path: str", ret = "Result")]
pub fn io_read_bytes(path: &str) -> Value {
    match std::fs::read(path) {
        Ok(bytes) => {
            let items: Vec<Value> = bytes.iter().map(|b| Value::from_i48(*b as i64)).collect();
            ok_result(Value::from_array_boxed(items))
        }
        Err(e) => make_err_io(path, e),
    }
}

/// Read and parse a `.env` file securely, returning key-value pairs as a dict.
///
/// Reads a `.env` file and parses it into a dictionary of environment variables.
/// Supports standard `.env` format: `KEY=VALUE` pairs, one per line.
///
/// ## SECURITY
///
/// - Only reads from the specified path; no shell expansion or command injection.
/// - Values are stored as plain strings; no automatic execution or evaluation.
/// - Comments (lines starting with `#`) and empty lines are ignored.
/// - Export prefix (`export KEY=VALUE`) is supported and stripped.
///
/// ## SYNTAX
///
/// ```xcl
/// fn read_env($path: str) -> Result
/// ```
///
/// ## PARAMETERS
///
/// `$path`: The file system path to the `.env` file to read.
///
/// ## EXAMPLES
///
/// ```xcl
/// $result = io.read_env ".env"
/// if result.is_ok $result {
///     $vars = $result.value
///     io.print $vars.DATABASE_URL
///     io.print $vars.API_KEY
/// }
/// ```
///
/// ## FORMAT
///
/// - `KEY=VALUE` — Standard key-value pair
/// - `export KEY=VALUE` — Export prefix (stripped)
/// - `# comment` — Comment line (ignored)
/// - `` — Empty line (ignored)
/// - `KEY="quoted value"` — Quoted value (quotes preserved)
///
/// ## INPUT TYPES
///
/// - `str`
///
/// ## OUTPUT TYPES
///
/// - `Result`
///
/// ## NOTES
///
/// - Returns `Err` if the file doesn't exist or cannot be read.
/// - Duplicate keys: later values override earlier ones.
/// - Keys must be valid environment variable names (alphanumeric and underscore).
/// - Values are not automatically trimmed; use `str.trim` if needed.
#[xcl_fn(name = "io.read_env", args = "path: str", ret = "Result")]
pub fn io_read_env(path: &str) -> Value {
    let content = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => return make_err_io(path, e),
    };

    let mut vars = HashMap::new();

    for line in content.lines() {
        let line = line.trim();

        // Skip empty lines and comments
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Strip optional "export " prefix
        let line = line.strip_prefix("export ").unwrap_or(line).trim();

        // Find the first '=' to split key and value
        let Some((key, value)) = line.split_once('=') else {
            continue; // Skip malformed lines
        };

        let key = key.trim();
        let value = value.trim();

        // Validate key: only allow alphanumeric and underscore
        if key.is_empty() || !key.chars().all(|c| c.is_alphanumeric() || c == '_') {
            continue; // Skip invalid keys
        }

        vars.insert(key.to_string(), Value::from_str_boxed(value.to_string()));
    }

    ok_result(Value::from_map_boxed(vars))
}

/// Write a string to a file, creating or overwriting it.
///
/// Writes the entire string to the specified file path. If the file exists, it is
/// truncated and overwritten. If it doesn't exist, it is created.
///
/// ## SYNTAX
///
/// ```xcl
/// fn write($path: str; $data: str) -> Result
/// ```
///
/// ## PARAMETERS
///
/// `$path`: The file system path where the file will be written.
///
/// `$data`: The string content to write to the file.
///
/// ## EXAMPLES
///
/// ```xcl
/// # Write a new file
/// $result = io.write "hello.txt" "Hello, world!"
/// ```
///
/// ```xcl
/// # Overwrite existing file
/// $result = io.write "data.txt" "New content"
/// ```
///
/// ## INPUT TYPES
///
/// - `str`
///
/// ## OUTPUT TYPES
///
/// - `Result`
///
/// ## NOTES
///
/// - Existing files are truncated; use `io.append()` to add content without overwriting.
/// - Returns `Err` if the path is not writable or the disk is full.
#[xcl_fn(name = "io.write", args = "path: str; data: str", ret = "Result")]
pub fn io_write(path: &str, data: &str) -> Value {
    match std::fs::write(path, data.as_bytes()) {
        Ok(_) => ok_result(Value::unit()),
        Err(e) => make_err_io(path, e),
    }
}

/// Write raw bytes to an open file handle.
///
/// Writes an array of byte values (0-255) to an open file handle. Useful for
/// writing binary data to files.
///
/// ## SYNTAX
///
/// ```xcl
/// fn write_bytes($handle: File; $bytes: arr) -> Result
/// ```
///
/// ## PARAMETERS
///
/// `$handle`: A file handle obtained from `io.open`.
///
/// `$bytes`: An array of integer values (0-255) representing bytes to write.
///
/// ## EXAMPLES
///
/// ```xcl
/// $result = io.open "data.bin" "w"
/// $file = $result.value
/// io.write_bytes $file [72, 101, 108, 108, 111]  # "Hello" in ASCII
/// io.close $file
/// ```
///
/// ## INPUT TYPES
///
/// - `File`
/// - `arr`
///
/// ## OUTPUT TYPES
///
/// - `Result`
///
/// ## NOTES
///
/// - Each integer in the array is written as a single byte.
/// - Values outside 0-255 are truncated to 8 bits.
#[xcl_fn(name = "io.write_bytes", args = "handle: File; data: arr<u8>", ret = "Result")]
pub fn io_write_bytes(ctx: &mut DomainCtx) -> Value {
    let handle = ctx.arg(0);
    let bytes = ctx.arg(1);

    let Some(arr) = bytes.as_array() else {
        return err_result(universal::OTHER, "io.write_bytes: expected array", -1);
    };
    let data: Vec<u8> = arr.iter().map(|v| v.as_i48() as u8).collect();

    let Some(hkind) = handle.as_handle_mut() else {
        return err_result(universal::OTHER, "io.write_bytes: not a handle", -1);
    };

    match hkind {
        HandleKind::File { file, .. } => {
            match file.write_all(&data) {
                Ok(_) => ok_result(Value::unit()),
                Err(e) => err_result(universal::OTHER, &format!("io.write_bytes: {e}"), e.raw_os_error().map(|c| c as i64).unwrap_or(0)),
            }
        }
        HandleKind::Stdout => {
            match std::io::stdout().write_all(&data) {
                Ok(_) => { let _ = std::io::stdout().flush(); ok_result(Value::unit()) }
                Err(e) => err_result(universal::OTHER, &format!("io.write_bytes: {e}"), e.raw_os_error().map(|c| c as i64).unwrap_or(0)),
            }
        }
        _ => err_result(universal::OTHER, "io.write_bytes: handle not writable", -1),
    }
}

/// Append a string to a file, creating it if it doesn't exist.
///
/// Adds the specified string to the end of the file. If the file doesn't exist,
/// it is created with the given content.
///
/// ## SYNTAX
///
/// ```xcl
/// fn append($path: str; $data: str) -> Result
/// ```
///
/// ## PARAMETERS
///
/// `$path`: The file system path to append to.
///
/// `$data`: The string content to append to the end of the file.
///
/// ## EXAMPLES
///
/// ```xcl
/// # Append to a log file
/// io.append "app.log" "2024-01-01: Application started\n"
/// io.append "app.log" "2024-01-01: Processing complete\n"
/// ```
///
/// ## INPUT TYPES
///
/// - `str`
///
/// ## OUTPUT TYPES
///
/// - `Result`
///
/// ## NOTES
///
/// - Unlike `io.write()`, this preserves existing file content.
/// - Returns `Err` if the path is not writable.
#[xcl_fn(name = "io.append", args = "path: str; data: str", ret = "Result")]
pub fn io_append(path: &str, data: &str) -> Value {
    match std::fs::OpenOptions::new().append(true).create(true).open(path) {
        Ok(mut file) => {
            match file.write_all(data.as_bytes()) {
                Ok(_) => ok_result(Value::unit()),
                Err(e) => make_err_io(path, e),
            }
        }
        Err(e) => make_err_io(path, e),
    }
}

/// Read a file and split its contents into lines.
///
/// Reads the entire file and splits it into an array of strings, one per line.
/// Trailing newlines are removed from each line.
///
/// ## SYNTAX
///
/// ```xcl
/// fn lines($path: str) -> Result
/// ```
///
/// ## PARAMETERS
///
/// `$path`: The file system path to the file to read.
///
/// ## EXAMPLES
///
/// ```xcl
/// $result = io.lines "words.txt"
/// if result.is_ok $result {
///     for $line in $result.value {
///         io.print $line
///     }
/// }
/// ```
///
/// ## INPUT TYPES
///
/// - `str`
///
/// ## OUTPUT TYPES
///
/// - `Result`
///
/// ## NOTES
///
/// - Lines are split on `\n` and `\r\n` boundaries.
/// - Trailing newlines are stripped from each line.
/// - The entire file is loaded into memory first.
#[xcl_fn(name = "io.lines", args = "path: str", ret = "Result")]
pub fn io_lines(path: &str) -> Value {
    match std::fs::read_to_string(path) {
        Ok(content) => {
            let lines: Vec<Value> = content.lines().map(|l| Value::from_str_boxed(l.to_string())).collect();
            ok_result(Value::from_array_boxed(lines))
        }
        Err(e) => make_err_io(path, e),
    }
}

/// Copy the contents of one file to another.
///
/// Copies the entire contents of the source file to the destination file.
/// Returns the number of bytes copied on success.
///
/// ## SYNTAX
///
/// ```xcl
/// fn copy($src: str; $dst: str) -> Result
/// ```
///
/// ## PARAMETERS
///
/// `$src`: The file system path of the source file.
///
/// `$dst`: The file system path of the destination file.
///
/// ## EXAMPLES
///
/// ```xcl
/// $result = io.copy "original.txt" "backup.txt"
/// if result.is_ok $result {
///     io.print "Copied ${$result.value} bytes"
/// }
/// ```
///
/// ## INPUT TYPES
///
/// - `str`
///
/// ## OUTPUT TYPES
///
/// - `Result`
///
/// ## NOTES
///
/// - The destination file is created or overwritten.
/// - Returns `Err` if the source file doesn't exist or the destination is not writable.
#[xcl_fn(name = "io.copy", args = "src: str; dst: str", ret = "Result")]
pub fn io_copy(src: &str, dst: &str) -> Value {
    match std::fs::copy(src, dst) {
        Ok(bytes) => ok_result(Value::from_i48(bytes as i64)),
        Err(e) => err_result(universal::OTHER, &format!("io.copy: {src}: {e}"), e.raw_os_error().map(|c| c as i64).unwrap_or(0)),
    }
}

/// Close an open file handle.
///
/// Releases the file handle and any associated system resources. After closing,
/// the handle can no longer be used for read or write operations.
///
/// ## SYNTAX
///
/// ```xcl
/// fn close($handle: File) -> none
/// ```
///
/// ## PARAMETERS
///
/// `$handle`: The file handle to close, obtained from `io.open`.
///
/// ## EXAMPLES
///
/// ```xcl
/// $result = io.open "data.txt" "r"
/// $file = $result.value
/// $content = io.read_all_from $file
/// io.close $file
/// ```
///
/// ## INPUT TYPES
///
/// - `File`
///
/// ## NOTES
///
/// - Always close file handles when done to free system resources.
/// - Operations on a closed handle will return errors.
#[xcl_fn(name = "io.close", args = "handle: File", ret = "none")]
pub fn io_close(ctx: &mut DomainCtx) -> Value {
    let handle = ctx.arg(0);
    let _ = handle.take_handle();
    ctx.unit()
}

/// Flush stdout and stderr buffers.
///
/// Forces any buffered output to be written immediately to the underlying streams.
/// Useful when you need to ensure output is visible before continuing.
///
/// ## SYNTAX
///
/// ```xcl
/// fn flush() -> none
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// # Print progress indicator without newline
/// io.print "." no_newline=true
/// io.flush
/// ```
///
/// ## NOTES
///
/// - Output is typically buffered for performance; flush forces immediate writing.
/// - Both stdout and stderr are flushed.
#[xcl_fn(name = "io.flush", ret = "none")]
pub fn io_flush(_ctx: &mut DomainCtx) -> Value {
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    Value::unit()
}

/// Check if a file handle has reached end-of-file.
///
/// Determines whether the current read position of a file handle has reached
/// the end of the file. Useful for loop-based reading patterns.
///
/// ## SYNTAX
///
/// ```xcl
/// fn eof($handle: File) -> bool
/// ```
///
/// ## PARAMETERS
///
/// `$handle`: The file handle to check.
///
/// ## EXAMPLES
///
/// ```xcl
/// $result = io.open "data.txt" "r"
/// $file = $result.value
/// while (not (io.eof $file)) {
///     $line = io.read_line_from $file
///     io.print $line
/// }
/// io.close $file
/// ```
///
/// ## INPUT TYPES
///
/// - `File`
///
/// ## OUTPUT TYPES
///
/// - `bool`
///
/// ## NOTES
///
/// - Returns `true` for non-file handles.
/// - For stdin, always returns `false`.
#[xcl_fn(name = "io.eof", args = "handle: File", ret = "bool")]
pub fn io_eof(ctx: &mut DomainCtx) -> Value {
    let handle = ctx.arg(0);
    let Some(hkind) = handle.as_handle() else {
        return Value::bool_(true);
    };

    match hkind {
        HandleKind::File { path, .. } => {
            match std::fs::metadata(path) {
                Ok(meta) => {
                    let mut f = match std::fs::File::open(path) {
                        Ok(f) => f,
                        Err(_) => return Value::bool_(false),
                    };
                    Value::bool_(f.stream_position().unwrap_or(0) >= meta.len())
                }
                Err(_) => Value::bool_(false),
            }
        }
        HandleKind::Stdin => Value::bool_(false),
        _ => Value::bool_(true),
    }
}

/// Read all bytes from an open file handle.
///
/// Reads the entire remaining contents of an open file as an array of byte values (0-255).
///
/// ## SYNTAX
///
/// ```xcl
/// fn read_from($handle: File) -> Result
/// ```
///
/// ## PARAMETERS
///
/// `$handle`: A file handle obtained from `io.open`.
///
/// ## EXAMPLES
///
/// ```xcl
/// $result = io.open "data.bin" "r"
/// $file = $result.value
/// $bytes = io.read_from $file
/// io.close $file
/// ```
///
/// ## INPUT TYPES
///
/// - `File`
///
/// ## OUTPUT TYPES
///
/// - `Result`
///
/// ## NOTES
///
/// - Returns `Err` if the handle is not readable.
/// - Each byte is returned as an integer value from 0 to 255.
#[xcl_fn(name = "io.read_from", args = "handle: File", ret = "Result")]
pub fn io_read_from(ctx: &mut DomainCtx) -> Value {
    let handle = ctx.arg(0);
    let Some(hkind) = handle.as_handle() else {
        return err_result(universal::OTHER, "io.read_from: not a handle", -1);
    };

    match hkind {
        HandleKind::File { path, .. } => {
            let mut f = match std::fs::File::open(path) {
                Ok(f) => f,
                Err(e) => return make_err_io(path, e),
            };
            let mut buf = Vec::new();
            match f.read_to_end(&mut buf) {
                Ok(_) => {
                    let items: Vec<Value> = buf.iter().map(|b| Value::from_i48(*b as i64)).collect();
                    ok_result(Value::from_array_boxed(items))
                }
                Err(e) => err_result(universal::OTHER, &format!("io.read_from: {e}"), e.raw_os_error().map(|c| c as i64).unwrap_or(0)),
            }
        }
        _ => err_result(universal::OTHER, "io.read_from: handle not readable", -1),
    }
}

/// Read a single line from an open file handle.
///
/// Reads one line from the current position of an open file handle.
/// The trailing newline is stripped from the returned string.
///
/// ## SYNTAX
///
/// ```xcl
/// fn read_line_from($handle: File) -> Result
/// ```
///
/// ## PARAMETERS
///
/// `$handle`: A file handle obtained from `io.open`.
///
/// ## EXAMPLES
///
/// ```xcl
/// $result = io.open "data.txt" "r"
/// $file = $result.value
/// $line = io.read_line_from $file
/// io.print $line
/// io.close $file
/// ```
///
/// ## INPUT TYPES
///
/// - `File`
///
/// ## OUTPUT TYPES
///
/// - `Result`
///
/// ## NOTES
///
/// - Returns `Err` if the handle is not readable.
/// - Trailing `\n` and `\r` are stripped from the returned string.
#[xcl_fn(name = "io.read_line_from", args = "handle: File", ret = "Result")]
pub fn io_read_line_from(ctx: &mut DomainCtx) -> Value {
    let handle = ctx.arg(0);
    let Some(hkind) = handle.as_handle() else {
        return err_result(universal::OTHER, "io.read_line_from: not a handle", -1);
    };

    match hkind {
        HandleKind::File { path, .. } => {
            let f = match std::fs::File::open(path) {
                Ok(f) => f,
                Err(e) => return make_err_io(path, e),
            };
            let mut reader = std::io::BufReader::new(f);
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(n) if n > 0 => ok_result(Value::from_str_boxed(line.trim_end().to_string())),
                Ok(_) => ok_result(Value::unit()),
                Err(e) => err_result(universal::OTHER, &format!("io.read_line_from: {e}"), e.raw_os_error().map(|c| c as i64).unwrap_or(0)),
            }
        }
        _ => err_result(universal::OTHER, "io.read_line_from: handle not readable", -1),
    }
}

/// Read all text from an open file handle as a UTF-8 string.
///
/// Reads the entire remaining contents of an open file as a UTF-8 encoded string.
///
/// ## SYNTAX
///
/// ```xcl
/// fn read_all_from($handle: File) -> Result
/// ```
///
/// ## PARAMETERS
///
/// `$handle`: A file handle obtained from `io.open`.
///
/// ## EXAMPLES
///
/// ```xcl
/// $result = io.open "data.txt" "r"
/// $file = $result.value
/// $content = io.read_all_from $file
/// io.print $content
/// io.close $file
/// ```
///
/// ## INPUT TYPES
///
/// - `File`
///
/// ## OUTPUT TYPES
///
/// - `Result`
///
/// ## NOTES
///
/// - Returns `Err` if the handle is not readable.
/// - File must be valid UTF-8; use `io.read_from()` for binary data.
#[xcl_fn(name = "io.read_all_from", args = "handle: File", ret = "Result")]
pub fn io_read_all_from(ctx: &mut DomainCtx) -> Value {
    let handle = ctx.arg(0);
    let Some(hkind) = handle.as_handle() else {
        return err_result(universal::OTHER, "io.read_all_from: not a handle", -1);
    };

    match hkind {
        HandleKind::File { path, .. } => {
            match std::fs::read_to_string(path) {
                Ok(s) => ok_result(Value::from_str_boxed(s)),
                Err(e) => make_err_io(path, e),
            }
        }
        _ => err_result(universal::OTHER, "io.read_all_from: handle not readable", -1),
    }
}

/// Write raw bytes to an open file handle.
///
/// Writes an array of byte values (0-255) to an open file handle.
/// Returns the number of bytes written on success.
///
/// ## SYNTAX
///
/// ```xcl
/// fn write_to($handle: File; $bytes: arr) -> Result
/// ```
///
/// ## PARAMETERS
///
/// `$handle`: A file handle obtained from `io.open`.
///
/// `$bytes`: An array of integer values (0-255) representing bytes to write.
///
/// ## EXAMPLES
///
/// ```xcl
/// $result = io.open "data.bin" "w"
/// $file = $result.value
/// $written = io.write_to $file [72, 101, 108, 108, 111]
/// io.close $file
/// ```
///
/// ## INPUT TYPES
///
/// - `File`
/// - `arr`
///
/// ## OUTPUT TYPES
///
/// - `Result`
///
/// ## NOTES
///
/// - Returns the number of bytes written.
/// - Values outside 0-255 are truncated to 8 bits.
#[xcl_fn(name = "io.write_to", args = "handle: File; data: arr<u8>", ret = "Result")]
pub fn io_write_to(ctx: &mut DomainCtx) -> Value {
    let handle = ctx.arg(0);
    let data = ctx.arg(1);

    let Some(arr) = data.as_array() else {
        return err_result(universal::OTHER, "io.write_to: expected array", -1);
    };
    let data_bytes: Vec<u8> = arr.iter().map(|v| v.as_i48() as u8).collect();

    let Some(hkind) = handle.as_handle_mut() else {
        return err_result(universal::OTHER, "io.write_to: not a handle", -1);
    };

    match hkind {
        HandleKind::File { file, .. } => {
            match file.write_all(&data_bytes) {
                Ok(_) => ok_result(Value::from_i48(data_bytes.len() as i64)),
                Err(e) => err_result(universal::OTHER, &format!("io.write_to: {e}"), e.raw_os_error().map(|c| c as i64).unwrap_or(0)),
            }
        }
        _ => err_result(universal::OTHER, "io.write_to: handle not writable", -1),
    }
}

/// Write a string to an open file handle.
///
/// Writes a string to an open file handle at the current position.
/// Returns the number of bytes written on success.
///
/// ## SYNTAX
///
/// ```xcl
/// fn write_str_to($handle: File; $data: str) -> Result
/// ```
///
/// ## PARAMETERS
///
/// `$handle`: A file handle obtained from `io.open`.
///
/// `$data`: The string content to write.
///
/// ## EXAMPLES
///
/// ```xcl
/// $result = io.open "output.txt" "w"
/// $file = $result.value
/// io.write_str_to $file "Hello, world!"
/// io.close $file
/// ```
///
/// ## INPUT TYPES
///
/// - `File`
/// - `str`
///
/// ## OUTPUT TYPES
///
/// - `Result`
///
/// ## NOTES
///
/// - Returns the number of bytes written.
/// - Does not append a trailing newline; use `io.write_line_to()` for that.
#[xcl_fn(name = "io.write_str_to", args = "handle: File; data: str", ret = "Result")]
pub fn io_write_str_to(ctx: &mut DomainCtx) -> Value {
    let handle = ctx.arg(0);
    let data = ctx.arg(1).as_str().unwrap_or("");

    let Some(hkind) = handle.as_handle_mut() else {
        return err_result(universal::OTHER, "io.write_str_to: not a handle", -1);
    };

    match hkind {
        HandleKind::File { file, .. } => {
            match file.write_all(data.as_bytes()) {
                Ok(_) => ok_result(Value::from_i48(data.len() as i64)),
                Err(e) => err_result(universal::OTHER, &format!("io.write_str_to: {e}"), e.raw_os_error().map(|c| c as i64).unwrap_or(0)),
            }
        }
        _ => err_result(universal::OTHER, "io.write_str_to: handle not writable", -1),
    }
}

/// Write a line to an open file handle, appending a newline if missing.
///
/// Writes a string to an open file handle, automatically appending a newline
/// character if the string doesn't already end with one.
///
/// ## SYNTAX
///
/// ```xcl
/// fn write_line_to($handle: File; $data: str) -> Result
/// ```
///
/// ## PARAMETERS
///
/// `$handle`: A file handle obtained from `io.open`.
///
/// `$data`: The string line to write.
///
/// ## EXAMPLES
///
/// ```xcl
/// $result = io.open "log.txt" "a"
/// $file = $result.value
/// io.write_line_to $file "Log entry 1"
/// io.write_line_to $file "Log entry 2"
/// io.close $file
/// ```
///
/// ## INPUT TYPES
///
/// - `File`
/// - `str`
///
/// ## OUTPUT TYPES
///
/// - `Result`
///
/// ## NOTES
///
/// - Automatically appends `\n` if the string doesn't end with one.
/// - Returns the total number of bytes written, including the newline.
#[xcl_fn(name = "io.write_line_to", args = "handle: File; data: str", ret = "Result")]
pub fn io_write_line_to(ctx: &mut DomainCtx) -> Value {
    let handle = ctx.arg(0);
    let data = ctx.arg(1).as_str().unwrap_or("");

    let mut line = data.to_string();
    if !line.ends_with('\n') {
        line.push('\n');
    }

    let Some(hkind) = handle.as_handle_mut() else {
        return err_result(universal::OTHER, "io.write_line_to: not a handle", -1);
    };

    match hkind {
        HandleKind::File { file, .. } => {
            match file.write_all(line.as_bytes()) {
                Ok(_) => ok_result(Value::from_i48(line.len() as i64)),
                Err(e) => err_result(universal::OTHER, &format!("io.write_line_to: {e}"), e.raw_os_error().map(|c| c as i64).unwrap_or(0)),
            }
        }
        _ => err_result(universal::OTHER, "io.write_line_to: handle not writable", -1),
    }
}

/// Seek to a position in an open file.
///
/// Moves the read/write position of an open file handle to the specified byte offset
/// from the start of the file. Returns the new position.
///
/// ## SYNTAX
///
/// ```xcl
/// fn seek($handle: File; $offset: i64) -> Result
/// ```
///
/// ## PARAMETERS
///
/// `$handle`: A file handle obtained from `io.open`.
///
/// `$offset`: The byte offset from the start of the file.
///
/// ## EXAMPLES
///
/// ```xcl
/// $result = io.open "data.txt" "r"
/// $file = $result.value
/// io.seek $file 100  # Skip first 100 bytes
/// $content = io.read_all_from $file
/// io.close $file
/// ```
///
/// ## INPUT TYPES
///
/// - `File`
/// - `i64`
///
/// ## OUTPUT TYPES
///
/// - `Result`
///
/// ## NOTES
///
/// - Offset is always from the start of the file (SEEK_SET).
/// - Returns `Err` if the handle is not seekable.
#[xcl_fn(name = "io.seek", args = "handle: File; position: i64", ret = "Result")]
pub fn io_seek(ctx: &mut DomainCtx) -> Value {
    let handle = ctx.arg(0);
    let offset = ctx.arg(1).as_i48();

    let Some(hkind) = handle.as_handle_mut() else {
        return err_result(universal::OTHER, "io.seek: not a handle", -1);
    };

    match hkind {
        HandleKind::File { file, .. } => {
            match file.seek(SeekFrom::Start(offset as u64)) {
                Ok(pos) => ok_result(Value::from_i48(pos as i64)),
                Err(e) => err_result(universal::OTHER, &format!("io.seek: {e}"), e.raw_os_error().map(|c| c as i64).unwrap_or(0)),
            }
        }
        _ => err_result(universal::OTHER, "io.seek: handle not seekable", -1),
    }
}

/// Get the current read/write position in an open file.
///
/// Returns the current byte offset of the read/write position within an open file.
///
/// ## SYNTAX
///
/// ```xcl
/// fn tell($handle: File) -> Result
/// ```
///
/// ## PARAMETERS
///
/// `$handle`: A file handle obtained from `io.open`.
///
/// ## EXAMPLES
///
/// ```xcl
/// $result = io.open "data.txt" "r"
/// $file = $result.value
/// $pos = io.tell $file
/// io.print "Current position: ${$pos.value}"
/// io.close $file
/// ```
///
/// ## INPUT TYPES
///
/// - `File`
///
/// ## OUTPUT TYPES
///
/// - `Result`
///
/// ## NOTES
///
/// - Position is in bytes from the start of the file.
/// - Returns `Err` if the handle is not seekable.
#[xcl_fn(name = "io.tell", args = "handle: File", ret = "Result")]
pub fn io_tell(ctx: &mut DomainCtx) -> Value {
    let handle = ctx.arg(0);
    let Some(hkind) = handle.as_handle() else {
        return err_result(universal::OTHER, "io.tell: not a handle", -1);
    };

    match hkind {
        HandleKind::File { path, .. } => {
            let mut f = match std::fs::File::open(path) {
                Ok(f) => f,
                Err(e) => return make_err_io(path, e),
            };
            match f.stream_position() {
                Ok(pos) => ok_result(Value::from_i48(pos as i64)),
                Err(e) => err_result(universal::OTHER, &format!("io.tell: {e}"), e.raw_os_error().map(|c| c as i64).unwrap_or(0)),
            }
        }
        _ => err_result(universal::OTHER, "io.tell: handle not seekable", -1),
    }
}

/// Get the standard input handle.
///
/// Returns a handle representing the standard input stream (stdin), which can be
/// used with handle-based I/O functions.
///
/// ## SYNTAX
///
/// ```xcl
/// fn stdin() -> Stream
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// $stdin = io.stdin
/// $isTerminal = io.is_tty $stdin
/// ```
///
/// ## OUTPUT TYPES
///
/// - `Stream`
///
/// ## NOTES
///
/// - The returned handle is pre-opened; no need to close it.
/// - Can be used with `io.is_tty()` to check if input is from a terminal.
#[xcl_fn(name = "io.stdin", ret = "Stream")]
pub fn io_stdin(_ctx: &mut DomainCtx) -> Value {
    Value::from_handle(HandleKind::Stdin)
}

/// Get the standard output handle.
///
/// Returns a handle representing the standard output stream (stdout), which can be
/// used with handle-based I/O functions.
///
/// ## SYNTAX
///
/// ```xcl
/// fn stdout() -> Stream
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// $stdout = io.stdout
/// $isTerminal = io.is_tty $stdout
/// ```
///
/// ## OUTPUT TYPES
///
/// - `Stream`
///
/// ## NOTES
///
/// - The returned handle is pre-opened; no need to close it.
/// - Can be used with `io.is_tty()` to check if output goes to a terminal.
#[xcl_fn(name = "io.stdout", ret = "Stream")]
pub fn io_stdout(_ctx: &mut DomainCtx) -> Value {
    Value::from_handle(HandleKind::Stdout)
}

/// Get the standard error handle.
///
/// Returns a handle representing the standard error stream (stderr), which can be
/// used with handle-based I/O functions.
///
/// ## SYNTAX
///
/// ```xcl
/// fn stderr() -> Stream
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// $stderr = io.stderr
/// $isTerminal = io.is_tty $stderr
/// ```
///
/// ## OUTPUT TYPES
///
/// - `Stream`
///
/// ## NOTES
///
/// - The returned handle is pre-opened; no need to close it.
/// - Use for error messages that should not be redirected with normal output.
#[xcl_fn(name = "io.stderr", ret = "Stream")]
pub fn io_stderr(_ctx: &mut DomainCtx) -> Value {
    Value::from_handle(HandleKind::Stderr)
}

/// Check if a handle is connected to a terminal (TTY).
///
/// Determines whether a handle is connected to an interactive terminal rather than
/// a pipe or file. Useful for conditionally enabling interactive features.
///
/// ## SYNTAX
///
/// ```xcl
/// fn is_tty($handle: Stream) -> bool
/// ```
///
/// ## PARAMETERS
///
/// `$handle`: A stream handle (stdin, stdout, or stderr).
///
/// ## EXAMPLES
///
/// ```xcl
/// if (io.is_tty io.stdin) {
///     io.print "Running interactively"
/// } else {
///     io.print "Reading from pipe"
/// }
/// ```
///
/// ## INPUT TYPES
///
/// - `Stream`
///
/// ## OUTPUT TYPES
///
/// - `bool`
///
/// ## NOTES
///
/// - Returns `false` for file handles.
/// - Returns `false` when input/output is piped or redirected.
#[xcl_fn(name = "io.is_tty", args = "handle: Stream", ret = "bool")]
pub fn io_is_tty(ctx: &mut DomainCtx) -> Value {
    let handle = ctx.arg(0);
    let Some(hkind) = handle.as_handle() else {
        return Value::bool_(false);
    };

    match hkind {
        HandleKind::Stdin => Value::bool_(std::io::IsTerminal::is_terminal(&std::io::stdin())),
        HandleKind::Stdout => Value::bool_(std::io::IsTerminal::is_terminal(&std::io::stdout())),
        HandleKind::Stderr => Value::bool_(std::io::IsTerminal::is_terminal(&std::io::stderr())),
        HandleKind::File { .. } => Value::bool_(false),
        HandleKind::Process { .. } => Value::bool_(false),
    }
}

/// Read a single line from standard input.
///
/// Reads one line from stdin and returns it with the trailing newline stripped.
/// Returns a Result wrapping the line string.
///
/// ## SYNTAX
///
/// ```xcl
/// fn read_line() -> Result
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// $result = io.read_line
/// if result.is_ok $result {
///     io.print "You entered: ${$result.value}"
/// }
/// ```
///
/// ## OUTPUT TYPES
///
/// - `Result`
///
/// ## NOTES
///
/// - Trailing `\n` and `\r` are stripped from the returned string.
/// - Returns `Ok(none)` on EOF.
/// - For prompted input, use `io.input()` instead.
#[xcl_fn(name = "io.read_line", ret = "Result")]
pub fn io_read_line(_ctx: &mut DomainCtx) -> Value {
    let mut s = String::new();
    match std::io::stdin().read_line(&mut s) {
        Ok(n) if n > 0 => ok_result(Value::from_str_boxed(s.trim_end().to_string())),
        Ok(_) => ok_result(Value::unit()),
        Err(e) => err_result(universal::OTHER, "io.read_line: stdin error", e.raw_os_error().map(|c| c as i64).unwrap_or(0)),
    }
}

/// Read the entire contents of a file as a string.
///
/// Shortcut for reading a file without Result wrapping — returns the file contents
/// directly on success, or an error message string on failure.
///
/// ## SYNTAX
///
/// ```xcl
/// fn read_all($path: str) -> str
/// ```
///
/// ## PARAMETERS
///
/// `$path`: The file system path to the file to read.
///
/// ## EXAMPLES
///
/// ```xcl
/// # Simple file reading without error handling
/// $content = io.read_all "config.txt"
/// io.print $content
/// ```
///
/// ## INPUT TYPES
///
/// - `str`
///
/// ## OUTPUT TYPES
///
/// - `str`
///
/// ## NOTES
///
/// - On failure, returns the error message as a string instead of a Result.
/// - For proper error handling, use `io.read()` which returns a Result.
/// - File must be valid UTF-8.
#[xcl_fn(name = "io.read_all", args = "path: str", ret = "str")]
pub fn io_read_all(path: &str) -> Value {
    match std::fs::read_to_string(path) {
        Ok(s) => Value::from_str_boxed(s),
        Err(e) => Value::from_str_boxed(e.to_string()),
    }
}

/// Get the system temporary directory path.
///
/// Returns the path to the system's temporary directory, which can be used for
/// storing temporary files.
///
/// ## SYNTAX
///
/// ```xcl
/// fn temp_dir() -> str
/// ```
///
/// ## EXAMPLES
///
/// ```xcl
/// $tmp = io.temp_dir
/// io.print "Temp directory: $tmp"
/// ```
///
/// ## OUTPUT TYPES
///
/// - `str`
///
/// ## NOTES
///
/// - On Windows, typically returns `C:\Users\<user>\AppData\Local\Temp`.
/// - On Linux/macOS, typically returns `/tmp`.
#[xcl_fn(name = "io.temp_dir", ret = "str")]
pub fn io_temp_dir(_ctx: &mut DomainCtx) -> Value {
    let dir = std::env::temp_dir();
    Value::from_str_boxed(dir.to_string_lossy().to_string())
}

/// Create a temporary file with the given suffix.
///
/// Creates an empty file in the system temp directory with a name based on the
/// process ID and the provided suffix.
///
/// ## SYNTAX
///
/// ```xcl
/// fn temp_file($suffix: str) -> str
/// ```
///
/// ## PARAMETERS
///
/// `$suffix`: The file name suffix (e.g., `".txt"`, `".log"`).
///
/// ## EXAMPLES
///
/// ```xcl
/// $tmpPath = io.temp_file ".txt"
/// io.write $tmpPath "Temporary data"
/// io.print "Created: $tmpPath"
/// ```
///
/// ## INPUT TYPES
///
/// - `str`
///
/// ## OUTPUT TYPES
///
/// - `str`
///
/// ## NOTES
///
/// - The file name includes the process ID for uniqueness.
/// - The file is created empty; write content separately.
/// - Remember to delete temporary files when done.
#[xcl_fn(name = "io.temp_file", args = "suffix: str", ret = "str")]
pub fn io_temp_file(suffix: &str) -> Value {
    let dir = std::env::temp_dir();
    let path = dir.join(format!("xcl_{}_{}", std::process::id(), suffix));
    let _ = std::fs::File::create(&path);
    Value::from_str_boxed(path.to_string_lossy().to_string())
}

/// Convert a value to its string representation.
///
/// Converts any value to its string representation. Handles integers, floats,
/// booleans, and other types by converting them to their display string.
///
/// ## SYNTAX
///
/// ```xcl
/// fn to_string($value: any) -> str
/// ```
///
/// ## PARAMETERS
///
/// `$value`: The value to convert to a string.
///
/// ## EXAMPLES
///
/// ```xcl
/// $numStr = io.to_string 42
/// $floatStr = io.to_string 3.14
/// $boolStr = io.to_string true
/// ```
///
/// ## INPUT TYPES
///
/// - `any`
///
/// ## OUTPUT TYPES
///
/// - `str`
///
/// ## NOTES
///
/// - Integers are formatted without decimal points.
/// - Floats use standard decimal notation.
/// - `none` values are converted to an empty string.
#[xcl_fn(name = "io.to_string", ret = "str")]
pub fn io_to_string(args: &[Value]) -> Value {
    let v = args.first().copied().unwrap_or(Value::unit());
    if v.is_int() {
        Value::from_str_boxed(itoa::Buffer::new().format(v.as_i48()).to_string())
    } else if v.is_float() {
        Value::from_str_boxed(ryu::Buffer::new().format(v.as_f64()).to_string())
    } else {
        Value::from_str_boxed(v.as_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xcl_vm::{VARIANT_OK, VARIANT_ERR};
    use std::io::Write;
    use std::env::temp_dir;

    static TEST_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    fn create_env_file(content: &str) -> std::path::PathBuf {
        let dir = temp_dir();
        let counter = TEST_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = dir.join(format!("xcl_test_env_{}_{}.txt", std::process::id(), counter));
        let mut file = std::fs::File::create(&path).unwrap();
        file.write_all(content.as_bytes()).unwrap();
        path
    }

    fn get_result_value(result: &Value) -> Value {
        let map = result.as_map().unwrap();
        map.get("value").copied().unwrap_or(Value::nil())
    }

    #[test]
    fn read_env_basic_key_value() {
        let path = create_env_file("DATABASE_URL=postgres://localhost/mydb");
        let result = io_read_env(path.to_str().unwrap());
        std::fs::remove_file(&path).ok();

        assert_eq!(result.map_variant(), Some(VARIANT_OK));
        let vars = get_result_value(&result).as_map().unwrap();
        assert_eq!(vars.get("DATABASE_URL").unwrap().as_str(), Some("postgres://localhost/mydb"));
    }

    #[test]
    fn read_env_multiple_keys() {
        let path = create_env_file("KEY1=value1\nKEY2=value2\nKEY3=value3");
        let result = io_read_env(path.to_str().unwrap());
        std::fs::remove_file(&path).ok();

        assert_eq!(result.map_variant(), Some(VARIANT_OK));
        let vars = get_result_value(&result).as_map().unwrap();
        assert_eq!(vars.get("KEY1").unwrap().as_str(), Some("value1"));
        assert_eq!(vars.get("KEY2").unwrap().as_str(), Some("value2"));
        assert_eq!(vars.get("KEY3").unwrap().as_str(), Some("value3"));
    }

    #[test]
    fn read_env_ignores_comments() {
        let path = create_env_file("# This is a comment\nKEY=value\n# Another comment");
        let result = io_read_env(path.to_str().unwrap());
        std::fs::remove_file(&path).ok();

        assert_eq!(result.map_variant(), Some(VARIANT_OK));
        let vars = get_result_value(&result).as_map().unwrap();
        assert_eq!(vars.len(), 1);
        assert_eq!(vars.get("KEY").unwrap().as_str(), Some("value"));
    }

    #[test]
    fn read_env_ignores_empty_lines() {
        let path = create_env_file("KEY1=value1\n\n\nKEY2=value2\n");
        let result = io_read_env(path.to_str().unwrap());
        std::fs::remove_file(&path).ok();

        assert_eq!(result.map_variant(), Some(VARIANT_OK));
        let vars = get_result_value(&result).as_map().unwrap();
        assert_eq!(vars.len(), 2);
    }

    #[test]
    fn read_env_strips_export_prefix() {
        let path = create_env_file("export API_KEY=secret123");
        let result = io_read_env(path.to_str().unwrap());
        std::fs::remove_file(&path).ok();

        assert_eq!(result.map_variant(), Some(VARIANT_OK));
        let vars = get_result_value(&result).as_map().unwrap();
        assert_eq!(vars.get("API_KEY").unwrap().as_str(), Some("secret123"));
    }

    #[test]
    fn read_env_invalid_key_skipped() {
        let path = create_env_file("VALID_KEY=good\ninvalid-key=bad\nALSO_VALID=also_good");
        let result = io_read_env(path.to_str().unwrap());
        std::fs::remove_file(&path).ok();

        assert_eq!(result.map_variant(), Some(VARIANT_OK));
        let vars = get_result_value(&result).as_map().unwrap();
        assert_eq!(vars.len(), 2);
        assert_eq!(vars.get("VALID_KEY").unwrap().as_str(), Some("good"));
        assert_eq!(vars.get("ALSO_VALID").unwrap().as_str(), Some("also_good"));
    }

    #[test]
    fn read_env_value_with_equals() {
        let path = create_env_file("BASE64=data==value==");
        let result = io_read_env(path.to_str().unwrap());
        std::fs::remove_file(&path).ok();

        assert_eq!(result.map_variant(), Some(VARIANT_OK));
        let vars = get_result_value(&result).as_map().unwrap();
        assert_eq!(vars.get("BASE64").unwrap().as_str(), Some("data==value=="));
    }

    #[test]
    fn read_env_file_not_found() {
        let result = io_read_env("/nonexistent/path/.env");
        assert_eq!(result.map_variant(), Some(VARIANT_ERR));
    }

    #[test]
    fn read_env_empty_file() {
        let path = create_env_file("");
        let result = io_read_env(path.to_str().unwrap());
        std::fs::remove_file(&path).ok();

        assert_eq!(result.map_variant(), Some(VARIANT_OK));
        let vars = get_result_value(&result).as_map().unwrap();
        assert_eq!(vars.len(), 0);
    }
}

