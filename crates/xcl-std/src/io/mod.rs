use std::collections::HashMap;
use std::io::{BufRead, Read, Write, Seek, SeekFrom};
use xcl_macros::{xcl_fn, xcl_type};
use xcl_vm::{Value, HandleKind, DomainCtx};

#[xcl_type(name = "File", domain = "io", kind = "Opaque")]
pub struct FileType;

#[xcl_type(name = "Stream", domain = "io", kind = "Opaque")]
pub struct StreamType;

#[inline]
fn make_ok(value: Value) -> Value {
    let mut m = HashMap::with_capacity(2);
    m.insert("tag".into(), Value::from_str_boxed("Ok".into()));
    m.insert("value".into(), value);
    Value::from_map_boxed(m)
}

#[inline]
fn make_err(message: &str, code: i64) -> Value {
    let mut m = HashMap::with_capacity(3);
    m.insert("tag".into(), Value::from_str_boxed("Err".into()));
    m.insert("message".into(), Value::from_str_boxed(message.into()));
    m.insert("code".into(), Value::from_i48(code));
    Value::from_map_boxed(m)
}

#[inline]
fn make_err_io(path: &str, e: std::io::Error) -> Value {
    let code = e.raw_os_error().map(|c| c as i64).unwrap_or(0);
    make_err(&format!("{path}: {e}"), code)
}

#[xcl_fn(name = "open", domain = "io", args = "path: str; mode: str", ret = "Result")]
pub fn io_open(path: &str, mode: &str) -> Value {
    let file = open_with_mode(path, mode);
    match file {
        Ok(f) => make_ok(Value::from_handle(HandleKind::File { file: f, path: path.to_string() })),
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

#[xcl_fn(name = "read", domain = "io", args = "path: str", ret = "Result")]
pub fn io_read(path: &str) -> Value {
    match std::fs::read_to_string(path) {
        Ok(s) => make_ok(Value::from_str_boxed(s)),
        Err(e) => make_err_io(path, e),
    }
}

#[xcl_fn(name = "read_bytes", domain = "io", args = "path: str", ret = "Result")]
pub fn io_read_bytes(path: &str) -> Value {
    match std::fs::read(path) {
        Ok(bytes) => {
            let items: Vec<Value> = bytes.iter().map(|b| Value::from_i48(*b as i64)).collect();
            make_ok(Value::from_array_boxed(items))
        }
        Err(e) => make_err_io(path, e),
    }
}

#[xcl_fn(name = "write", domain = "io", args = "path: str; data: str", ret = "Result")]
pub fn io_write(path: &str, data: &str) -> Value {
    match std::fs::write(path, data.as_bytes()) {
        Ok(_) => make_ok(Value::unit()),
        Err(e) => make_err_io(path, e),
    }
}

#[xcl_fn(name = "write_bytes", domain = "io")]
pub fn io_write_bytes(ctx: &mut DomainCtx) -> Value {
    let handle = ctx.arg(0);
    let bytes = ctx.arg(1);

    let Some(arr) = bytes.as_array() else {
        return make_err("io.write_bytes: expected array", -1);
    };
    let data: Vec<u8> = arr.iter().map(|v| v.as_i48() as u8).collect();

    let Some(hkind) = handle.as_handle_mut() else {
        return make_err("io.write_bytes: not a handle", -1);
    };

    match hkind {
        HandleKind::File { file, .. } => {
            match file.write_all(&data) {
                Ok(_) => make_ok(Value::unit()),
                Err(e) => make_err(&format!("io.write_bytes: {e}"), e.raw_os_error().map(|c| c as i64).unwrap_or(0)),
            }
        }
        HandleKind::Stdout => {
            match std::io::stdout().write_all(&data) {
                Ok(_) => { let _ = std::io::stdout().flush(); make_ok(Value::unit()) }
                Err(e) => make_err(&format!("io.write_bytes: {e}"), e.raw_os_error().map(|c| c as i64).unwrap_or(0)),
            }
        }
        _ => make_err("io.write_bytes: handle not writable", -1),
    }
}

#[xcl_fn(name = "append", domain = "io", args = "path: str; data: str", ret = "Result")]
pub fn io_append(path: &str, data: &str) -> Value {
    match std::fs::OpenOptions::new().append(true).create(true).open(path) {
        Ok(mut file) => {
            match file.write_all(data.as_bytes()) {
                Ok(_) => make_ok(Value::unit()),
                Err(e) => make_err_io(path, e),
            }
        }
        Err(e) => make_err_io(path, e),
    }
}

#[xcl_fn(name = "lines", domain = "io", args = "path: str", ret = "Result")]
pub fn io_lines(path: &str) -> Value {
    match std::fs::read_to_string(path) {
        Ok(content) => {
            let lines: Vec<Value> = content.lines().map(|l| Value::from_str_boxed(l.to_string())).collect();
            make_ok(Value::from_array_boxed(lines))
        }
        Err(e) => make_err_io(path, e),
    }
}

#[xcl_fn(name = "copy", domain = "io", args = "src: str; dst: str", ret = "Result")]
pub fn io_copy(src: &str, dst: &str) -> Value {
    match std::fs::copy(src, dst) {
        Ok(bytes) => make_ok(Value::from_i48(bytes as i64)),
        Err(e) => make_err(&format!("io.copy: {src}: {e}"), e.raw_os_error().map(|c| c as i64).unwrap_or(0)),
    }
}

#[xcl_fn(name = "close", domain = "io")]
pub fn io_close(ctx: &mut DomainCtx) -> Value {
    let handle = ctx.arg(0);
    let _ = handle.take_handle();
    ctx.unit()
}

#[xcl_fn(name = "flush", domain = "io")]
pub fn io_flush(_ctx: &mut DomainCtx) -> Value {
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    Value::unit()
}

#[xcl_fn(name = "eof", domain = "io")]
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

#[xcl_fn(name = "read_from", domain = "io")]
pub fn io_read_from(ctx: &mut DomainCtx) -> Value {
    let handle = ctx.arg(0);
    let Some(hkind) = handle.as_handle() else {
        return make_err("io.read_from: not a handle", -1);
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
                    make_ok(Value::from_array_boxed(items))
                }
                Err(e) => make_err(&format!("io.read_from: {e}"), e.raw_os_error().map(|c| c as i64).unwrap_or(0)),
            }
        }
        _ => make_err("io.read_from: handle not readable", -1),
    }
}

#[xcl_fn(name = "read_line_from", domain = "io")]
pub fn io_read_line_from(ctx: &mut DomainCtx) -> Value {
    let handle = ctx.arg(0);
    let Some(hkind) = handle.as_handle() else {
        return make_err("io.read_line_from: not a handle", -1);
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
                Ok(n) if n > 0 => make_ok(Value::from_str_boxed(line.trim_end().to_string())),
                Ok(_) => make_ok(Value::unit()),
                Err(e) => make_err(&format!("io.read_line_from: {e}"), e.raw_os_error().map(|c| c as i64).unwrap_or(0)),
            }
        }
        _ => make_err("io.read_line_from: handle not readable", -1),
    }
}

#[xcl_fn(name = "read_all_from", domain = "io")]
pub fn io_read_all_from(ctx: &mut DomainCtx) -> Value {
    let handle = ctx.arg(0);
    let Some(hkind) = handle.as_handle() else {
        return make_err("io.read_all_from: not a handle", -1);
    };

    match hkind {
        HandleKind::File { path, .. } => {
            match std::fs::read_to_string(path) {
                Ok(s) => make_ok(Value::from_str_boxed(s)),
                Err(e) => make_err_io(path, e),
            }
        }
        _ => make_err("io.read_all_from: handle not readable", -1),
    }
}

#[xcl_fn(name = "write_to", domain = "io")]
pub fn io_write_to(ctx: &mut DomainCtx) -> Value {
    let handle = ctx.arg(0);
    let data = ctx.arg(1);

    let Some(arr) = data.as_array() else {
        return make_err("io.write_to: expected array", -1);
    };
    let data_bytes: Vec<u8> = arr.iter().map(|v| v.as_i48() as u8).collect();

    let Some(hkind) = handle.as_handle_mut() else {
        return make_err("io.write_to: not a handle", -1);
    };

    match hkind {
        HandleKind::File { file, .. } => {
            match file.write_all(&data_bytes) {
                Ok(_) => make_ok(Value::from_i48(data_bytes.len() as i64)),
                Err(e) => make_err(&format!("io.write_to: {e}"), e.raw_os_error().map(|c| c as i64).unwrap_or(0)),
            }
        }
        _ => make_err("io.write_to: handle not writable", -1),
    }
}

#[xcl_fn(name = "write_str_to", domain = "io")]
pub fn io_write_str_to(ctx: &mut DomainCtx) -> Value {
    let handle = ctx.arg(0);
    let data = ctx.arg(1).as_str().unwrap_or("");

    let Some(hkind) = handle.as_handle_mut() else {
        return make_err("io.write_str_to: not a handle", -1);
    };

    match hkind {
        HandleKind::File { file, .. } => {
            match file.write_all(data.as_bytes()) {
                Ok(_) => make_ok(Value::from_i48(data.len() as i64)),
                Err(e) => make_err(&format!("io.write_str_to: {e}"), e.raw_os_error().map(|c| c as i64).unwrap_or(0)),
            }
        }
        _ => make_err("io.write_str_to: handle not writable", -1),
    }
}

#[xcl_fn(name = "write_line_to", domain = "io")]
pub fn io_write_line_to(ctx: &mut DomainCtx) -> Value {
    let handle = ctx.arg(0);
    let data = ctx.arg(1).as_str().unwrap_or("");

    let mut line = data.to_string();
    if !line.ends_with('\n') {
        line.push('\n');
    }

    let Some(hkind) = handle.as_handle_mut() else {
        return make_err("io.write_line_to: not a handle", -1);
    };

    match hkind {
        HandleKind::File { file, .. } => {
            match file.write_all(line.as_bytes()) {
                Ok(_) => make_ok(Value::from_i48(line.len() as i64)),
                Err(e) => make_err(&format!("io.write_line_to: {e}"), e.raw_os_error().map(|c| c as i64).unwrap_or(0)),
            }
        }
        _ => make_err("io.write_line_to: handle not writable", -1),
    }
}

#[xcl_fn(name = "seek", domain = "io")]
pub fn io_seek(ctx: &mut DomainCtx) -> Value {
    let handle = ctx.arg(0);
    let offset = ctx.arg(1).as_i48();

    let Some(hkind) = handle.as_handle_mut() else {
        return make_err("io.seek: not a handle", -1);
    };

    match hkind {
        HandleKind::File { file, .. } => {
            match file.seek(SeekFrom::Start(offset as u64)) {
                Ok(pos) => make_ok(Value::from_i48(pos as i64)),
                Err(e) => make_err(&format!("io.seek: {e}"), e.raw_os_error().map(|c| c as i64).unwrap_or(0)),
            }
        }
        _ => make_err("io.seek: handle not seekable", -1),
    }
}

#[xcl_fn(name = "tell", domain = "io")]
pub fn io_tell(ctx: &mut DomainCtx) -> Value {
    let handle = ctx.arg(0);
    let Some(hkind) = handle.as_handle() else {
        return make_err("io.tell: not a handle", -1);
    };

    match hkind {
        HandleKind::File { path, .. } => {
            let mut f = match std::fs::File::open(path) {
                Ok(f) => f,
                Err(e) => return make_err_io(path, e),
            };
            match f.stream_position() {
                Ok(pos) => make_ok(Value::from_i48(pos as i64)),
                Err(e) => make_err(&format!("io.tell: {e}"), e.raw_os_error().map(|c| c as i64).unwrap_or(0)),
            }
        }
        _ => make_err("io.tell: handle not seekable", -1),
    }
}

#[xcl_fn(name = "stdin", domain = "io")]
pub fn io_stdin(_ctx: &mut DomainCtx) -> Value {
    Value::from_handle(HandleKind::Stdin)
}

#[xcl_fn(name = "stdout", domain = "io")]
pub fn io_stdout(_ctx: &mut DomainCtx) -> Value {
    Value::from_handle(HandleKind::Stdout)
}

#[xcl_fn(name = "stderr", domain = "io")]
pub fn io_stderr(_ctx: &mut DomainCtx) -> Value {
    Value::from_handle(HandleKind::Stderr)
}

#[xcl_fn(name = "is_tty", domain = "io")]
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
    }
}

#[xcl_fn(name = "read_line", domain = "io")]
pub fn io_read_line(_ctx: &mut DomainCtx) -> Value {
    let mut s = String::new();
    match std::io::stdin().read_line(&mut s) {
        Ok(n) if n > 0 => make_ok(Value::from_str_boxed(s.trim_end().to_string())),
        Ok(_) => make_ok(Value::unit()),
        Err(e) => make_err("io.read_line: stdin error", e.raw_os_error().map(|c| c as i64).unwrap_or(0)),
    }
}

#[xcl_fn(name = "read_all", domain = "io", args = "path: str", ret = "str")]
pub fn io_read_all(path: &str) -> Value {
    match std::fs::read_to_string(path) {
        Ok(s) => Value::from_str_boxed(s),
        Err(e) => Value::from_str_boxed(e.to_string()),
    }
}

#[xcl_fn(name = "temp_dir", domain = "io")]
pub fn io_temp_dir(_ctx: &mut DomainCtx) -> Value {
    let dir = std::env::temp_dir();
    Value::from_str_boxed(dir.to_string_lossy().to_string())
}

#[xcl_fn(name = "temp_file", domain = "io", args = "suffix: str", ret = "str")]
pub fn io_temp_file(suffix: &str) -> Value {
    let dir = std::env::temp_dir();
    let path = dir.join(format!("xcl_{}_{}", std::process::id(), suffix));
    let _ = std::fs::File::create(&path);
    Value::from_str_boxed(path.to_string_lossy().to_string())
}

#[xcl_fn(name = "to_string", domain = "io")]
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

