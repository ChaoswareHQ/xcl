//! Thin launcher for the XCL toolchain and engine.
//!
//! The command-line surface (options, help, exit codes) lives here. Source
//! files, linting, and plugin configuration go through `xcl_toolchain.dll`
//! (lexing, parsing, compiling, diagnostics). Precompiled `.xclb` files can
//! instead go straight to `xcl_rt.dll`, which only executes bytecode. Both are
//! loaded at run time, so this binary has no link-time dependency on either.
//!
//! This mirrors how an embedded Luau host loads the Luau library and forwards
//! work to it.

use std::env;
use std::ffi::{CStr, CString, c_char, c_void};
use std::io::{self, Read};
use std::process;

mod loader;

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// How diagnostics are written.
///
/// `Human` is the compiler-style view for reading; `Json` is one object per
/// line on stdout, which is what a tool or an agent consumes.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
enum OutputFormat {
    #[default]
    Human,
    Json,
}

impl OutputFormat {
    fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "human" => OutputFormat::Human,
            "json" => OutputFormat::Json,
            _ => return None,
        })
    }

    fn as_i32(self) -> i32 {
        i32::from(self == OutputFormat::Json)
    }
}

#[derive(Default, Debug)]
struct CliOpts {
    help: bool,
    version: bool,
    eval: Option<String>,
    quiet: bool,
    decompile: bool,
    files: Vec<String>,
    script_args: Vec<String>,
    /// Analyse instead of running.
    lint: bool,
    list_lints: bool,
    explain: Option<String>,
    format: OutputFormat,
    /// `-A/-W/-D/-F` in the order given: (target, level name).
    levels: Vec<(String, &'static str)>,
    /// `--features` names, in the order given.
    features: Vec<String>,
    /// `--no-default-features`.
    no_default_features: bool,
    /// `--all-features`.
    all_features: bool,
    /// `--list-features`.
    list_features: bool,
    /// `--profile <name>`.
    profile: Option<String>,
    /// `--compile[=OUT]` — compile to .xclb, do not run.
    compile: Option<Option<String>>,
    /// `--rt` — run .xclb with bare engine (xcl_rt.dll).
    rt: bool,
    /// `abi --rt` — print engine ABI table.
    abi_rt: bool,
}

/// Reads the value of an option, either from `--opt=value` or the next argument.
fn take_value(
    name: &str,
    inline: Option<String>,
    args: &mut impl Iterator<Item = String>,
) -> String {
    if let Some(value) = inline {
        return value;
    }
    match args.next() {
        Some(value) => value,
        None => {
            eprintln!("xcl: error: --{} requires an argument", name);
            process::exit(1);
        }
    }
}

/// Rejects `--flag=value` for flags that take no value.
fn reject_value(name: &str, inline: &Option<String>) {
    if inline.is_some() {
        eprintln!("xcl: error: --{} does not take a value", name);
        process::exit(1);
    }
}

fn parse_args<I>(iter: I) -> CliOpts
where
    I: IntoIterator<Item = String>,
{
    let mut opts = CliOpts::default();
    let mut args = iter.into_iter();

    args.next();

    while let Some(arg) = args.next() {
        if opts.help {
            continue;
        }

        if arg == "--" {
            opts.script_args.extend(args.by_ref());
            break;
        }

        if let Some(rest) = arg.strip_prefix("--") {
            let (name, inline) = match rest.split_once('=') {
                Some((name, value)) => (name, Some(value.to_string())),
                None => (rest, None),
            };

            match name {
                "help" => {
                    reject_value(name, &inline);
                    opts.help = true
                }
                "version" => {
                    reject_value(name, &inline);
                    opts.version = true
                }
                "quiet" => {
                    reject_value(name, &inline);
                    opts.quiet = true
                }
                "decompile" => {
                    reject_value(name, &inline);
                    opts.decompile = true
                }
                "lint" => {
                    reject_value(name, &inline);
                    opts.lint = true
                }
                "list-lints" => {
                    reject_value(name, &inline);
                    opts.list_lints = true
                }
                "list-features" => {
                    reject_value(name, &inline);
                    opts.list_features = true
                }
                "no-default-features" => {
                    reject_value(name, &inline);
                    opts.no_default_features = true
                }
                "all-features" => {
                    reject_value(name, &inline);
                    opts.all_features = true
                }
                "features" => {
                    let value = take_value(name, inline, &mut args);
                    opts.features.extend(
                        value
                            .split([',', ' ', '\t', '\n', '\r'])
                            .map(str::trim)
                            .filter(|feature| !feature.is_empty())
                            .map(str::to_string),
                    );
                }
                "deny-warnings" => {
                    opts.levels.push(("warnings".to_string(), "deny"));
                }
                "eval" => opts.eval = Some(take_value(name, inline, &mut args)),
                "explain" => opts.explain = Some(take_value(name, inline, &mut args)),
                "profile" => opts.profile = Some(take_value(name, inline, &mut args)),
                "compile" => {
                    opts.compile = Some(inline);
                    // If no inline value, check if next arg looks like =value
                    // (already handled by split_once above)
                }
                "rt" => {
                    reject_value(name, &inline);
                    // If the first file is "abi", this is `abi --rt`
                    if opts.files.first().map(|f| f == "abi") == Some(true) {
                        opts.abi_rt = true;
                    } else {
                        opts.rt = true;
                    }
                }
                "error-format" => {
                    let value = take_value(name, inline, &mut args);
                    match OutputFormat::from_name(&value) {
                        Some(format) => opts.format = format,
                        None => {
                            eprintln!("xcl: error: invalid value '{}' for --error-format", value);
                            eprintln!("xcl: note: accepted values are `human` and `json`");
                            process::exit(1);
                        }
                    }
                }
                "allow" | "warn" | "deny" | "forbid" => {
                    let value = take_value(name, inline, &mut args);
                    let level = match name {
                        "allow" => "allow",
                        "warn" => "warn",
                        "deny" => "deny",
                        _ => "forbid",
                    };
                    opts.levels.push((value, level));
                }
                unknown => {
                    eprintln!("xcl: error: unknown option '--{}'", unknown);
                    process::exit(1);
                }
            }
            continue;
        }

        if arg.starts_with('-') && arg.len() > 1 {
            let chars: Vec<char> = arg[1..].chars().collect();
            let mut i = 0;
            while i < chars.len() {
                let c = chars[i];
                let level = match c {
                    'A' => Some("allow"),
                    'W' => Some("warn"),
                    'D' => Some("deny"),
                    'F' => Some("forbid"),
                    _ => None,
                };
                if let Some(level) = level {
                    // `-Astyle` and `-A style` both work.
                    let value: String = if i + 1 < chars.len() {
                        chars[i + 1..].iter().collect()
                    } else {
                        match args.next() {
                            Some(value) => value,
                            None => {
                                eprintln!("xcl: error: -{} requires an argument", c);
                                process::exit(1);
                            }
                        }
                    };
                    opts.levels.push((value, level));
                    break;
                }

                match c {
                    'h' => opts.help = true,
                    'v' => opts.version = true,
                    'q' => opts.quiet = true,
                    'd' => opts.decompile = true,
                    'e' => {
                        let val = if i + 1 < chars.len() {
                            chars[i + 1..].iter().collect::<String>()
                        } else if let Some(next) = args.next() {
                            next
                        } else {
                            eprintln!("xcl: error: -e requires an argument");
                            process::exit(1);
                        };
                        opts.eval = Some(val);
                        break;
                    }
                    unknown => {
                        eprintln!("xcl: error: unknown option '-{}'", unknown);
                        process::exit(1);
                    }
                }
                i += 1;
            }
            continue;
        }

        opts.files.push(arg);
    }

    opts
}

fn print_help() {
    println!(
        "Usage: xcl [OPTIONS] [FILE]...

Run XCL source files or evaluate expressions.

Options:
  -h, --help              Show this help message
  -v, --version           Show version information
  -e, --eval <EXPR>       Evaluate the given expression
  -q, --quiet             Suppress result output
  -d, --decompile         Decompile source to bytecode and print (do not run)
  --compile[=OUT]         Compile FILE to .xclb bytecode (default <stem>.xclb)
  --rt                    Run .xclb with the bare engine (xcl_rt.dll, no toolchain)
  --                      All following arguments are script arguments ($1, $2, ...)

Linting:
  --lint                  Analyse and report diagnostics instead of running
  --error-format <FMT>    `human` (default) or `json` (one object per line)
  --explain <CODE>        Explain a rule by code (W0001) or name (unused_binding)
  --list-lints            List every rule, its group, and its default level
  -A, --allow <TARGET>    Silence a rule, rule group, or `warnings`
  -W, --warn <TARGET>     Report a rule as a warning
  -D, --deny <TARGET>     Report a rule as an error
  -F, --forbid <TARGET>   Report a rule as an error; cannot be overridden later
      --deny-warnings     Shorthand for `-D warnings`

Runtime:
  The toolchain is loaded from `xcl_toolchain.dll` (or the platform
  equivalent) next to this binary. Set XCL_TOOLCHAIN to load another build.
  The engine is loaded from `xcl_rt.dll`. Set XCL_RT to override.

Commands:
  list                    List compiled plugins and the domains they install
  manifest                Print the generated plugin manifest (TOML)
  abi                     List the C ABI exported by the toolchain
  abi --rt                List the C ABI exported by the engine

Features:
  --features <LIST>       Enable compiled features in addition to defaults
  --no-default-features   Install only the features named by --features
  --all-features          Install every compiled feature

Security:
  --profile <NAME>        Capabilities granted to plugins: `sandbox`,
                          `automation`, `game-mod`, `trusted` (default), or
                          `deny-all`. An external plugin directory's
                          `plugin.toml` profile is used when this is absent.

When no FILE or -e is given, reads from stdin.

Examples:
  xcl script.xcl -- a b \"c d\"
  xcl -e 'print $1' -- hello
  echo 'print 42' | xcl
  xcl --compile script.xcl          # produces script.xclb
  xcl script.xclb                   # run via the toolchain (std natives available)
  xcl --rt script.xclb              # run via bare engine (core only)
  xcl --profile sandbox script.xcl
  xcl --lint script.xcl
  xcl abi --rt
"
    );
}

fn print_version() {
    println!("xcl {}", VERSION);
}

/// A loaded toolchain runtime plus its per-process context.
struct SdkHost {
    api: loader::RuntimeApi,
    ctx: *mut c_void,
}

impl SdkHost {
    /// Loads the toolchain shared library and applies the command-line configuration.
    fn load(opts: &CliOpts) -> Self {
        let api = loader::RuntimeApi::load().unwrap_or_else(|error| {
            eprintln!("xcl: error: {}", error);
            process::exit(1);
        });

        let ctx = unsafe { (api.new)() };
        if ctx.is_null() {
            eprintln!("xcl: error: the runtime could not create a context");
            process::exit(1);
        }

        let host = Self { api, ctx };
        unsafe {
            (host.api.set_quiet)(host.ctx, i32::from(opts.quiet));
            (host.api.set_format)(host.ctx, opts.format.as_i32());
        }

        let script_args: Vec<CString> = opts
            .script_args
            .iter()
            .map(|arg| CString::new(arg.as_str()).unwrap_or_default())
            .collect();
        let script_arg_ptrs: Vec<*const c_char> =
            script_args.iter().map(|arg| arg.as_ptr()).collect();
        unsafe {
            (host.api.set_script_args)(host.ctx, script_arg_ptrs.len(), script_arg_ptrs.as_ptr());
        }

        for (target, level) in &opts.levels {
            let target = CString::new(target.as_str()).unwrap_or_default();
            let level = CString::new(*level).unwrap_or_default();
            let status = unsafe { (host.api.add_level)(host.ctx, target.as_ptr(), level.as_ptr()) };
            if status != 0 {
                process::exit(status);
            }
        }

        let mode = if opts.all_features {
            3
        } else if opts.no_default_features {
            2
        } else if opts.features.is_empty() {
            0
        } else {
            1
        };
        let features = CString::new(opts.features.join(",")).unwrap_or_default();
        let status = unsafe { (host.api.set_features)(host.ctx, features.as_ptr(), mode) };
        if status != 0 {
            process::exit(status);
        }

        if let Some(name) = &opts.profile {
            let name = CString::new(name.as_str()).unwrap_or_default();
            let status = unsafe { (host.api.set_profile)(host.ctx, name.as_ptr()) };
            if status != 0 {
                process::exit(status);
            }
        }

        host
    }

    fn run_file(&self, path: &str) -> i32 {
        let path = CString::new(path).unwrap_or_default();
        unsafe { (self.api.run_file)(self.ctx, path.as_ptr()) }
    }

    fn run_source(&self, name: &str, source: &str) -> i32 {
        let name = CString::new(name).unwrap_or_default();
        unsafe { (self.api.run_source)(self.ctx, name.as_ptr(), source.as_ptr(), source.len()) }
    }

    fn run_bytecode(&self, path: &str) -> i32 {
        let path = CString::new(path).unwrap_or_default();
        unsafe { (self.api.run_bytecode)(self.ctx, path.as_ptr()) }
    }

    fn compile_file(&self, src: &str, out: &str) -> i32 {
        let src = CString::new(src).unwrap_or_default();
        let out = CString::new(out).unwrap_or_default();
        unsafe { (self.api.compile_file)(self.ctx, src.as_ptr(), out.as_ptr()) }
    }

    fn decompile_file(&self, path: &str) -> i32 {
        let path = CString::new(path).unwrap_or_default();
        unsafe { (self.api.decompile_file)(self.ctx, path.as_ptr()) }
    }

    fn decompile_source(&self, name: &str, source: &str) -> i32 {
        let name = CString::new(name).unwrap_or_default();
        unsafe {
            (self.api.decompile_source)(self.ctx, name.as_ptr(), source.as_ptr(), source.len())
        }
    }

    fn lint_file(&self, path: &str) -> i32 {
        let path = CString::new(path).unwrap_or_default();
        unsafe { (self.api.lint_file)(self.ctx, path.as_ptr()) }
    }

    fn lint_source(&self, name: &str, source: &str) -> i32 {
        let name = CString::new(name).unwrap_or_default();
        unsafe { (self.api.lint_source)(self.ctx, name.as_ptr(), source.as_ptr(), source.len()) }
    }

    fn list_lints(&self) -> i32 {
        unsafe { (self.api.list_lints)(self.ctx) }
    }

    fn list_plugins(&self) -> i32 {
        unsafe { (self.api.list_plugins)(self.ctx) }
    }

    fn manifest(&self) -> i32 {
        unsafe { (self.api.manifest)(self.ctx) }
    }

    fn abi_list(&self) -> i32 {
        unsafe { (self.api.abi_list)(self.ctx) }
    }

    fn explain(&self, code: &str) -> i32 {
        let code = CString::new(code).unwrap_or_default();
        unsafe { (self.api.explain)(self.ctx, code.as_ptr()) }
    }
}

impl Drop for SdkHost {
    fn drop(&mut self) {
        unsafe { (self.api.free)(self.ctx) };
    }
}

/// A loaded engine instance for `--rt` mode.
struct EngineHost {
    api: loader::EngineApi,
    ctx: *mut c_void,
}

impl EngineHost {
    fn load(opts: &CliOpts) -> Self {
        let api = loader::EngineApi::load().unwrap_or_else(|error| {
            eprintln!("xcl: error: {}", error);
            process::exit(1);
        });

        let ctx = unsafe { (api.new)() };
        if ctx.is_null() {
            eprintln!("xcl: error: the engine could not create a context");
            process::exit(1);
        }

        let host = Self { api, ctx };
        unsafe {
            (host.api.set_quiet)(host.ctx, i32::from(opts.quiet));
        }

        let script_args: Vec<CString> = opts
            .script_args
            .iter()
            .map(|arg| CString::new(arg.as_str()).unwrap_or_default())
            .collect();
        let script_arg_ptrs: Vec<*const c_char> =
            script_args.iter().map(|arg| arg.as_ptr()).collect();
        unsafe {
            (host.api.set_script_args)(host.ctx, script_arg_ptrs.len(), script_arg_ptrs.as_ptr());
        }

        host
    }

    fn run_bytecode(&self, path: &str) -> i32 {
        let path = CString::new(path).unwrap_or_default();
        let status = unsafe { (self.api.load)(self.ctx, path.as_ptr()) };
        if status != 0 {
            self.report_error();
            return 1;
        }

        let status = unsafe { (self.api.run)(self.ctx) };
        if status != 0 {
            self.report_error();
        }
        status
    }

    /// Prints the engine's last error, if it set one.
    fn report_error(&self) {
        let err = unsafe { (self.api.error)(self.ctx) };
        if err.is_null() {
            return;
        }
        let message = unsafe { CStr::from_ptr(err) }.to_string_lossy();
        if !message.is_empty() {
            eprintln!("xcl: error: {message}");
        }
    }
}

impl Drop for EngineHost {
    fn drop(&mut self) {
        unsafe { (self.api.free)(self.ctx) };
    }
}

/// Reads all of stdin, or exits with an error.
fn read_stdin() -> String {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input).unwrap_or_else(|e| {
        eprintln!("xcl: error: cannot read stdin: {}", e);
        process::exit(1);
    });
    input
}

/// Lints the selected inputs and returns the process exit code.
fn run_lint_mode(opts: &CliOpts, host: &SdkHost) -> i32 {
    if let Some(expr) = &opts.eval {
        return host.lint_source("<eval>", expr);
    }

    if opts.files.is_empty() {
        let input = read_stdin();
        return host.lint_source("<stdin>", &input);
    }

    let mut failed = 0;
    for file in &opts.files {
        failed |= host.lint_file(file);
    }
    failed
}

fn main() {
    let opts = parse_args(env::args());

    if opts.help {
        print_help();
        return;
    }
    if opts.version {
        print_version();
        return;
    }

    // --compile combined with --rt is an error
    if opts.compile.is_some() && opts.rt {
        eprintln!("xcl: error: --compile cannot be used with --rt");
        eprintln!("xcl: note: --compile uses the toolchain; --rt uses the bare engine");
        process::exit(1);
    }

    // abi --rt → load engine and print exports
    if opts.abi_rt {
        match loader::EngineApi::load() {
            Ok(_api) => {
                // Print the engine ABI table directly
                println!("XCL Engine ABI v{}", loader::ENGINE_ABI_VERSION);
                println!("  uint32_t xcl_rt_abi_version(void)");
                println!("  XclEngine* xcl_rt_new(void)");
                println!("  void xcl_rt_free(XclEngine*)");
                println!("  int xcl_rt_load(XclEngine*, const char* path)");
                println!("  int xcl_rt_run(XclEngine*)");
                println!("  const char* xcl_rt_error(XclEngine*)");
                println!("  int xcl_rt_set_quiet(XclEngine*, int quiet)");
                println!(
                    "  void xcl_rt_set_script_args(XclEngine*, size_t argc, const char* const* argv)"
                );
                process::exit(0);
            }
            Err(error) => {
                eprintln!("xcl: error: {}", error);
                process::exit(1);
            }
        }
    }

    // Load the toolchain for all non-engine operations
    let host = SdkHost::load(&opts);

    // Simple subcommands take the first positional word. A file literally named
    // `list` or `abi` is shadowed; commands win.
    match opts.files.first().map(String::as_str) {
        Some("list") => process::exit(host.list_plugins()),
        Some("manifest") => process::exit(host.manifest()),
        Some("abi") => process::exit(host.abi_list()),
        _ => {}
    }

    if let Some(code) = &opts.explain {
        process::exit(host.explain(code));
    }
    if opts.list_lints {
        process::exit(host.list_lints());
    }
    if opts.list_features {
        process::exit(host.list_plugins());
    }
    if opts.lint {
        process::exit(run_lint_mode(&opts, &host));
    }

    // --compile: compile to .xclb, do not run
    if let Some(out_opt) = &opts.compile {
        // Need exactly one input file
        if opts.files.len() != 1 {
            eprintln!("xcl: error: --compile requires exactly one input file");
            process::exit(1);
        }
        let input = &opts.files[0];
        let output = match out_opt {
            Some(path) => path.clone(),
            // Default: `<stem>.xclb` next to the source. `with_extension` keeps the
            // directory; `parent()` on a bare file name is `""`, which would turn
            // `core.xcl` into `/core.xclb`.
            None => std::path::Path::new(input)
                .with_extension("xclb")
                .to_string_lossy()
                .into_owned(),
        };
        process::exit(host.compile_file(input, &output));
    }

    // Mode 1: -e expression
    if let Some(expr) = &opts.eval {
        let status = if opts.decompile {
            host.decompile_source("<eval>", expr)
        } else {
            host.run_source("<eval>", expr)
        };
        process::exit(status);
    }

    // Mode 1b: Decompile file(s)
    if opts.decompile && !opts.files.is_empty() {
        let mut status = 0;
        for file in &opts.files {
            status = host.decompile_file(file);
            if status != 0 {
                break;
            }
        }
        process::exit(status);
    }

    // --rt mode: run .xclb with bare engine
    if opts.rt {
        if opts.files.is_empty() {
            eprintln!("xcl: error: --rt requires a .xclb file");
            process::exit(1);
        }
        // Drop the toolchain host, load engine
        drop(host);
        let engine = EngineHost::load(&opts);
        for file in &opts.files {
            let status = engine.run_bytecode(file);
            if status != 0 {
                process::exit(status);
            }
        }
        return;
    }

    // Mode 2: Run file(s) — route .xclb through the toolchain
    if !opts.files.is_empty() {
        for file in &opts.files {
            let status = if file.ends_with(".xclb") {
                host.run_bytecode(file)
            } else {
                host.run_file(file)
            };
            if status != 0 {
                process::exit(status);
            }
        }
        return;
    }

    // Mode 3: Read from stdin (REPL-like — just evaluate)
    let input = read_stdin();
    if input.is_empty() {
        return;
    }

    let status = host.run_source("<stdin>", &input);
    process::exit(status);
}
