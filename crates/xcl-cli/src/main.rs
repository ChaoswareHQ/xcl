use mimalloc::MiMalloc;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

use std::env;
use std::fs;
use std::io::{self, Read};
use std::process;

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Default, Debug)]
struct CliOpts {
    help: bool,
    version: bool,
    eval: Option<String>,
    quiet: bool,
    decompile: bool,
    files: Vec<String>,
    script_args: Vec<String>,
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

        if arg.starts_with("--") {
            match arg.as_str() {
                "--help" => opts.help = true,
                "--version" => opts.version = true,
                "--quiet" => opts.quiet = true,
                "--decompile" => opts.decompile = true,
                "--eval" => {
                    if let Some(val) = args.next() {
                        opts.eval = Some(val);
                    } else {
                        eprintln!("xcl: error: --eval requires an argument");
                        process::exit(1);
                    }
                }
                unknown => {
                    eprintln!("xcl: error: unknown option '{}'", unknown);
                    process::exit(1);
                }
            }
            continue;
        }

        if arg.starts_with("-") && arg.len() > 1 {
            let chars: Vec<char> = arg[1..].chars().collect();
            let mut i = 0;
            while i < chars.len() {
                let c = chars[i];
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
                        i += 1; // skip the value chars
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
  -h, --help          Show this help message
  -v, --version       Show version information
  -e, --eval <EXPR>   Evaluate the given expression
  -q, --quiet         Suppress result output
  -d, --decompile     Decompile source to bytecode and print (do not run)
  --                  All following arguments are script arguments ($1, $2, ...)

When no FILE or -e is given, reads from stdin.

Examples:
  xcl script.xcl -- a b \"c d\"
  xcl -e 'print $1' -- hello
  echo 'print 42' | xcl
  xcl -q script.xcl
"
    );
}

fn print_version() {
    println!("xcl {}", VERSION);
}

/// Disassembles `source` when the `debug` feature is enabled.
///
/// Without the feature the flag stays accepted, so scripts and docs that pass
/// `-d` fail with an actionable message instead of "unknown option".
#[cfg(feature = "debug")]
fn decompile(source: &str) -> String {
    xcl_debug::decompile::decompile_source(source)
}

#[cfg(not(feature = "debug"))]
fn decompile(_source: &str) -> String {
    eprintln!("xcl: error: this build has no disassembler; rebuild with `--features debug`");
    process::exit(1);
}

fn run_source(
    source: &str,
    quiet: bool,
    script_name: &str,
    script_args: &[String],
) -> xcl_vm::Value {
    let mut vm = xcl_plugins::new_vm();
    let args: Vec<xcl_vm::Value> = script_args
        .iter()
        .map(|s| xcl_vm::Value::from_str_boxed(s.clone()))
        .collect();
    vm.set_script_args(script_name, args);

    let result = xcl_plugins::evaluate(source, &mut vm);
    if !quiet && !result.is_nil() {
        print_value(result);
    }
    result
}

fn print_value(v: xcl_vm::Value) {
    if let Some(s) = v.as_str() {
        println!("{}", s);
    } else if v.is_int() {
        println!("{}", v.as_i48());
    } else if v.is_float() {
        println!("{}", v.as_f64());
    } else if v.is_bool() {
        println!("{}", v.as_bool());
    } else if let Some(map) = v.as_map() {
        println!("{{");
        for (k, val) in map {
            print!("  {}: ", k);
            print_value(*val);
        }
        println!("}}");
    } else if let Some(arr) = v.as_array() {
        print!("[");
        for (i, item) in arr.iter().enumerate() {
            if i > 0 {
                print!(", ");
            }
            print_value(*item);
        }
        println!("]");
    } else if v.is_nil() {
        println!("none");
    } else {
        println!("<value>");
    }
}

fn main() {
    let opts = parse_args(env::args());

    match (opts.help, opts.version) {
        (true, _) => {
            print_help();
            process::exit(0);
        }
        (_, true) => {
            print_version();
            process::exit(0);
        }
        _ => {}
    }

    // Mode 1: -e expression
    if let Some(expr) = &opts.eval {
        if opts.decompile {
            // Decompile mode: show bytecode instead of running
            print!("{}", decompile(expr));
        } else {
            let result = run_source(expr, opts.quiet, "<eval>", &opts.script_args);
            check_result_exit(result);
        }
        return;
    }

    // Mode 1b: Decompile file(s)
    if opts.decompile && !opts.files.is_empty() {
        for file in &opts.files {
            match fs::read_to_string(file) {
                Ok(source) => print!("{}", decompile(&source)),
                Err(e) => {
                    eprintln!("xcl: error: cannot read '{}': {}", file, e);
                    process::exit(1);
                }
            }
        }
        return;
    }

    // Mode 2: Run file(s)
    if !opts.files.is_empty() {
        for file in &opts.files {
            match fs::read_to_string(file) {
                Ok(source) => {
                    let result = run_source(&source, opts.quiet, file, &opts.script_args);
                    check_result_exit(result);
                }
                Err(e) => {
                    eprintln!("xcl: error: cannot read '{}': {}", file, e);
                    process::exit(1);
                }
            }
        }
        return;
    }

    // Mode 3: Read from stdin (REPL-like — just evaluate)
    let mut input = String::new();
    io::stdin().read_to_string(&mut input).unwrap_or_else(|e| {
        eprintln!("xcl: error: cannot read stdin: {}", e);
        process::exit(1);
    });

    if input.is_empty() {
        return;
    }

    let result = run_source(&input, opts.quiet, "<stdin>", &opts.script_args);
    check_result_exit(result);
}

/// If the last result is an Err Result, exit with code 1.
fn check_result_exit(result: xcl_vm::Value) {
    if let Some(map) = result.as_map()
        && let Some(tag) = map.get("tag")
        && tag.as_str() == Some("Err")
    {
        let msg = map
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown error");
        eprintln!("xcl: runtime error: {}", msg);
        process::exit(1);
    }
}
