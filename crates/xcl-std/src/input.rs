use std::cell::RefCell;
use std::io::{self, BufRead, BufReader, StdinLock, Read};

use xcl_macros::xcl_fn;
use xcl_vm::Value;

const IN_CAPACITY: usize = 64 * 1024;

pub struct FastIn {
    buf: BufReader<StdinLock<'static>>,
    scratch: String,
}

impl FastIn {
    fn new() -> Self {
        Self {
            buf: BufReader::with_capacity(IN_CAPACITY, io::stdin().lock()),
            scratch: String::with_capacity(256),
        }
    }

    /// Read one line. Strips trailing `\n` and `\r`.
    pub fn read_line(&mut self) -> io::Result<Option<String>> {
        self.scratch.clear();
        let n = self.buf.read_line(&mut self.scratch)?;
        if n == 0 {
            return Ok(None);
        }
        while self.scratch.ends_with('\n') || self.scratch.ends_with('\r') {
            self.scratch.pop();
        }
        Ok(Some(std::mem::take(&mut self.scratch)))
    }

    /// Read one whitespace-delimited token.
    fn read_token(&mut self) -> io::Result<bool> {
        self.scratch.clear();

        // Skip leading whitespace
        loop {
            let consumed = {
                let buf = self.buf.fill_buf()?;
                if buf.is_empty() {
                    return Ok(false);
                }
                let mut i = 0;
                while i < buf.len() && buf[i].is_ascii_whitespace() {
                    i += 1;
                }
                if i < buf.len() {
                    break;
                }
                i
            };
            self.buf.consume(consumed);
        }

        // Read until whitespace
        loop {
            let (consumed, done) = {
                let buf = self.buf.fill_buf()?;
                if buf.is_empty() {
                    break;
                }
                let mut j = 0;
                while j < buf.len() && !buf[j].is_ascii_whitespace() {
                    self.scratch.push(buf[j] as char);
                    j += 1;
                }
                (j, j < buf.len())
            };
            self.buf.consume(consumed);
            if done {
                break;
            }
        }
        Ok(true)
    }

    pub fn read_i64(&mut self) -> io::Result<Option<i64>> {
        if !self.read_token()? {
            return Ok(None);
        }
        Ok(self.scratch.parse::<i64>().ok())
    }

    pub fn read_f64(&mut self) -> io::Result<Option<f64>> {
        if !self.read_token()? {
            return Ok(None);
        }
        Ok(self.scratch.parse::<f64>().ok())
    }

    pub fn read_bool(&mut self) -> io::Result<Option<bool>> {
        if !self.read_token()? {
            return Ok(None);
        }
        match self.scratch.as_str() {
            "true" | "yes" | "y" | "1" => Ok(Some(true)),
            "false" | "no" | "n" | "0" => Ok(Some(false)),
            _ => Ok(None),
        }
    }

    pub fn read_str(&mut self) -> io::Result<Option<String>> {
        if !self.read_token()? {
            return Ok(None);
        }
        Ok(Some(std::mem::take(&mut self.scratch)))
    }

    pub fn read_all(&mut self) -> io::Result<String> {
        let mut s = String::new();
        self.buf.read_to_string(&mut s)?;
        Ok(s)
    }
}

thread_local! {
    static IN: RefCell<FastIn> = RefCell::new(FastIn::new());
}

#[inline]
pub fn with_in<R>(f: impl FnOnce(&mut FastIn) -> R) -> R {
    IN.with(|i| f(&mut i.borrow_mut()))
}

fn emit_prompt(prompt: &str) {
    if prompt.is_empty() {
        return;
    }
    crate::output::with_out(|w| {
        let _ = w.write_str(prompt);
    });
    crate::output::flush_out();
}

/// Read input from standard input, with optional type conversion.
///
/// By default, reads a full line of text. Use named flags to specify the input type:
/// `--bool` for boolean, `--int` for integer, `--float` for floating-point,
/// `--word` for a single word, `--line` for a full line (default), or `--all`
/// for all remaining input until EOF.
///
/// ## SYNTAX
///
/// ```xcl
/// fn input($prompt: str; --bool: bool; --int: bool; --float: bool; --word: bool; --line: bool; --all: bool) -> any
/// ```
///
/// ## PARAMETERS
///
/// `$prompt`: An optional string displayed before reading input. If omitted or empty, no
/// prompt is shown.
///
/// `--bool`: When set to `true`, reads and parses a boolean value. Accepts `true`, `yes`,
/// `y`, `1` for true and `false`, `no`, `n`, `0` for false. Returns `none` for
/// unrecognized input.
///
/// `--int`: When set to `true`, reads and parses a signed 64-bit integer. Returns `none`
/// if the input cannot be parsed.
///
/// `--float`: When set to `true`, reads and parses a 64-bit floating-point number.
/// Returns `none` if the input cannot be parsed.
///
/// `--word`: When set to `true`, reads a single whitespace-delimited word instead of a
/// full line.
///
/// `--line`: When set to `true`, reads a full line of text (default behavior).
///
/// `--all`: When set to `true`, reads all remaining input until EOF. Ignores the prompt
/// parameter.
///
/// ## EXAMPLES
///
/// ```xcl
/// # Read a line of text (default)
/// $name = input "Enter your name: "
/// print "Hello, " $name "!"
/// ```
///
/// ```xcl
/// # Read an integer
/// $age = input --int=true "Enter your age: "
/// if $age is i64 {
///     print "You are " $age " years old."
/// }
/// ```
///
/// ```xcl
/// # Read a boolean
/// $confirmed = input --bool=true "Continue? (yes/no): "
/// if $confirmed == true {
///     print "Proceeding..."
/// }
/// ```
///
/// ```xcl
/// # Read a single word
/// $word = input --word=true "Enter a word: "
/// ```
///
/// ```xcl
/// # Read all input until EOF
/// $content = input --all=true
/// print "Read " $content " characters"
/// ```
///
/// ## OUTPUT TYPES
///
/// - `str?` — Default (line mode) or `--word=true`.
/// - `bool?` — When `--bool=true`.
/// - `i64?` — When `--int=true`.
/// - `f64?` — When `--float=true`.
/// - `str` — When `--all=true`.
///
/// ## NOTES
///
/// - Only one type flag should be set at a time; if multiple are set, the first matching
/// flag takes precedence in the order: `--all`, `--bool`, `--int`, `--float`, `--word`,
/// `--line`.
/// - The returned string in line mode has trailing `\n` and `\r` characters removed.
/// - On EOF, returns `none` (except `--all`, which returns an empty string).
/// - The prompt is written to stdout and flushed before reading begins.
#[xcl_fn(
    name = "input",
    ret = "any",
    args = "prompt: str",
    named(
        { name = "bool", ty = "bool", default = "false" },
        { name = "int", ty = "bool", default = "false" },
        { name = "float", ty = "bool", default = "false" },
        { name = "word", ty = "bool", default = "false" },
        { name = "line", ty = "bool", default = "false" },
        { name = "all", ty = "bool", default = "false" },
    ),
)]
pub fn io_input_fn(args: &[Value]) -> Value {
    // Parse positional prompt arg
    let prompt = args.first().and_then(|v| v.as_str()).unwrap_or("");

    // Determine input mode from named params
    let mut as_bool = false;
    let mut as_int = false;
    let mut as_float = false;
    let mut as_word = false;
    let mut as_all = false;

    // Named params come as a map value in the args array
    for arg in args {
        if let Some(map) = arg.as_map() {
            for (key, val) in map {
                let is_true = val.is_bool() && val.as_bool();
                match key.as_str() {
                    "bool" => as_bool = is_true,
                    "int" => as_int = is_true,
                    "float" => as_float = is_true,
                    "word" => as_word = is_true,
                    "all" => as_all = is_true,
                    _ => {}
                }
            }
        }
    }

    emit_prompt(prompt);

    if as_all {
        with_in(|r| match r.read_all() {
            Ok(s) => Value::from_str_boxed(s),
            Err(_) => Value::unit(),
        })
    } else if as_bool {
        with_in(|r| match r.read_bool() {
            Ok(Some(b)) => Value::bool_(b),
            _ => Value::unit(),
        })
    } else if as_int {
        with_in(|r| match r.read_i64() {
            Ok(Some(n)) => Value::from_i48(n),
            _ => Value::unit(),
        })
    } else if as_float {
        with_in(|r| match r.read_f64() {
            Ok(Some(f)) => Value::from_f64(f),
            _ => Value::unit(),
        })
    } else if as_word {
        with_in(|r| match r.read_str() {
            Ok(Some(s)) => Value::from_str_boxed(s),
            _ => Value::unit(),
        })
    } else {
        // Default: read line
        with_in(|r| match r.read_line() {
            Ok(Some(s)) => Value::from_str_boxed(s),
            _ => Value::unit(),
        })
    }
}
