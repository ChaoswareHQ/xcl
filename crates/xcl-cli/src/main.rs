use mimalloc::MiMalloc;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

use std::env;
use std::fs;
use std::io::{self, Read};
use std::process;

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
    /// `-A/-W/-D/-F` in the order given: (canonical target, level name).
    levels: Vec<(String, &'static str)>,
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

/// Turns a lint target into the spelling the configuration matches.
///
/// Lint level flags are validated here so a typo fails immediately instead of
/// silently matching no rule.
#[cfg(feature = "lint")]
fn canonical_target(target: &str) -> String {
    match xcl_analysis::normalize_target(target) {
        Some(canonical) => canonical,
        None => {
            eprintln!("xcl: error: unknown lint: `{}`", target);
            eprintln!("xcl: note: run `xcl --list-lints` to see rule codes, names, and groups");
            process::exit(1);
        }
    }
}

/// Without the `lint` feature there is nothing to validate against; the flag is
/// accepted and inert, the same way `-d` is.
#[cfg(not(feature = "lint"))]
fn canonical_target(target: &str) -> String {
    target.to_string()
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
                "deny-warnings" => {
                    reject_value(name, &inline);
                    opts.levels.push(("warnings".to_string(), "deny"));
                }
                "eval" => opts.eval = Some(take_value(name, inline, &mut args)),
                "explain" => opts.explain = Some(take_value(name, inline, &mut args)),
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
                    opts.levels.push((canonical_target(&value), level));
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
                    opts.levels.push((canonical_target(&value), level));
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

When no FILE or -e is given, reads from stdin.

Examples:
  xcl script.xcl -- a b \"c d\"
  xcl -e 'print $1' -- hello
  echo 'print 42' | xcl
  xcl -q script.xcl
  xcl --lint script.xcl
  xcl --lint --error-format=json script.xcl
  xcl -D warnings --lint script.xcl
  xcl --explain W0001
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

/// Builds the lint configuration from the `-A/-W/-D/-F` flags.
#[cfg(feature = "lint")]
fn lint_config(opts: &CliOpts) -> xcl_analysis::LintConfig {
    let mut config = xcl_analysis::LintConfig::new();
    for (target, level) in &opts.levels {
        let level =
            xcl_analysis::LintLevel::from_name(level).unwrap_or(xcl_analysis::LintLevel::Warn);
        config = config.set(target.clone(), level);
    }
    config
}

/// Analyzes one source and writes its diagnostics.
///
/// Returns `true` when the report contains errors, which is what makes the
/// process exit non-zero.
#[cfg(feature = "lint")]
fn lint_source(source: &str, file: &str, opts: &CliOpts) -> bool {
    let config = lint_config(opts);
    let report = xcl_analysis::check_source(file, source, &config);
    let map = xcl_analysis::SourceMap::new(file, source);

    match opts.format {
        OutputFormat::Human => eprint!("{}", report.render_human(&map)),
        OutputFormat::Json => {
            print!("{}", report.render_json(&map));
            let errors = report.count(xcl_analysis::Level::Error);
            if errors > 0 {
                eprintln!(
                    "xcl: aborting due to {} previous error{}",
                    errors,
                    if errors == 1 { "" } else { "s" }
                );
            }
        }
    }

    report.has_errors()
}

#[cfg(not(feature = "lint"))]
fn lint_source(_source: &str, _file: &str, _opts: &CliOpts) -> bool {
    eprintln!("xcl: error: this build has no linter; rebuild with `--features lint`");
    process::exit(1);
}

/// Prints one's rule documentation, following a rename or a removal.
#[cfg(feature = "lint")]
fn explain_code(code: &str) {
    let code = code.trim();
    if let Some(text) = xcl_analysis::explain(&code.to_ascii_uppercase()) {
        print!("{text}");
        return;
    }

    match xcl_analysis::resolve_name(code) {
        Ok(rule) => print!("{}", rule.render()),
        Err(xcl_analysis::Resolution::Renamed { to }) => {
            eprintln!("xcl: note: `{}` was renamed to `{}`", code, to);
            match xcl_analysis::rule_by_name(to) {
                Some(rule) => print!("{}", rule.render()),
                None => process::exit(1),
            }
        }
        Err(xcl_analysis::Resolution::Removed { replaced_by }) => {
            eprintln!("xcl: error: `{}` has been removed", code);
            if let Some(replacement) = replaced_by {
                eprintln!("xcl: note: use `{}` instead", replacement);
            }
            process::exit(1);
        }
        Err(xcl_analysis::Resolution::Unknown) => {
            eprintln!("xcl: error: unknown lint: `{}`", code);
            eprintln!("xcl: note: run `xcl --list-lints` to see rule codes, names, and groups");
            process::exit(1);
        }
    }
}

#[cfg(not(feature = "lint"))]
fn explain_code(_code: &str) {
    eprintln!("xcl: error: this build has no rule catalogue; rebuild with `--features lint`");
    process::exit(1);
}

/// Prints every rule so a reader or an agent can discover the catalogue.
#[cfg(feature = "lint")]
fn list_lints() {
    println!(
        "{:<7} {:<24} {:<12} {:<8} SUMMARY",
        "CODE", "NAME", "GROUP", "DEFAULT"
    );
    for rule in xcl_analysis::RULES {
        println!(
            "{:<7} {:<24} {:<12} {:<8} {}",
            rule.code,
            rule.name,
            rule.group.name(),
            rule.default_level.name(),
            rule.summary
        );
    }
    println!();
    println!("Syntax errors (not lints; reported when the parser fails):");
    for (code, summary) in xcl_vm::PARSE_ERROR_CODES {
        println!("  {:<7} {}", code, summary);
    }
    println!();
    println!("Groups:  correctness, suspicious, style, complexity, perf, restriction");
    println!("Levels:  allow, warn, deny, forbid");
    println!("Targets: a code (W0001), a name (unused_binding), a group, or `warnings`");
    println!("Explain: xcl --explain W0001");
}

#[cfg(not(feature = "lint"))]
fn list_lints() {
    eprintln!("xcl: error: this build has no linter; rebuild with `--features lint`");
    process::exit(1);
}

fn run_source(
    source: &str,
    quiet: bool,
    script_name: &str,
    script_args: &[String],
    format: OutputFormat,
) -> xcl_vm::Value {
    let mut vm = xcl_plugins::new_vm();
    let args: Vec<xcl_vm::Value> = script_args
        .iter()
        .map(|s| xcl_vm::Value::from_str_boxed(s.clone()))
        .collect();
    vm.set_script_args(script_name, args);

    // Report every syntax error, not just the first one.
    let result = match xcl_plugins::evaluate_recover(script_name, source, &mut vm) {
        Ok(result) => result,
        Err(errors) => {
            report_parse_errors(source, script_name, &errors, format);
            process::exit(1);
        }
    };

    if !quiet && !result.is_nil() {
        print_value(result);
    }
    result
}

/// Writes the syntax errors found while running a program.
///
/// The JSON form is the same contract `--lint` emits, so a tool can parse a
/// failed run without knowing which mode produced it.
fn report_parse_errors(
    source: &str,
    file: &str,
    errors: &[xcl_vm::ParseError],
    format: OutputFormat,
) {
    match format {
        OutputFormat::Human => {
            for error in errors {
                eprintln!("{}", error.render());
            }
        }
        OutputFormat::Json => report_parse_errors_json(source, file, errors),
    }
    eprintln!(
        "xcl: aborting due to {} previous error{}",
        errors.len(),
        if errors.len() == 1 { "" } else { "s" }
    );
}

#[cfg(feature = "lint")]
fn report_parse_errors_json(source: &str, file: &str, errors: &[xcl_vm::ParseError]) {
    let report = xcl_analysis::parse_error_report(errors);
    let map = xcl_analysis::SourceMap::new(file, source);
    print!("{}", report.render_json(&map));
}

#[cfg(not(feature = "lint"))]
fn report_parse_errors_json(source: &str, file: &str, errors: &[xcl_vm::ParseError]) {
    // No analysis crate: fall back to the human form rather than losing errors.
    let _ = (source, file);
    for error in errors {
        eprintln!("{}", error.render());
    }
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

/// Reads all of stdin, or exits with an error.
fn read_stdin() -> String {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input).unwrap_or_else(|e| {
        eprintln!("xcl: error: cannot read stdin: {}", e);
        process::exit(1);
    });
    input
}

/// Reads a source file, or exits with an error.
fn read_source(file: &str) -> String {
    fs::read_to_string(file).unwrap_or_else(|e| {
        eprintln!("xcl: error: cannot read '{}': {}", file, e);
        process::exit(1);
    })
}

/// Runs every requested lint and exits: 1 when a diagnostic is an error.
fn run_lint_mode(opts: &CliOpts) -> ! {
    if let Some(expr) = &opts.eval {
        let failed = lint_source(expr, "<eval>", opts);
        process::exit(if failed { 1 } else { 0 });
    }

    if opts.files.is_empty() {
        let input = read_stdin();
        let failed = lint_source(&input, "<stdin>", opts);
        process::exit(if failed { 1 } else { 0 });
    }

    let mut failed = false;
    for file in &opts.files {
        let source = read_source(file);
        failed |= lint_source(&source, file, opts);
    }
    process::exit(if failed { 1 } else { 0 });
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
    if let Some(code) = &opts.explain {
        explain_code(code);
        return;
    }
    if opts.list_lints {
        list_lints();
        return;
    }
    if opts.lint {
        run_lint_mode(&opts);
    }

    // Mode 1: -e expression
    if let Some(expr) = &opts.eval {
        if opts.decompile {
            // Decompile mode: show bytecode instead of running
            print!("{}", decompile(expr));
        } else {
            let result = run_source(expr, opts.quiet, "<eval>", &opts.script_args, opts.format);
            check_result_exit(result);
        }
        return;
    }

    // Mode 1b: Decompile file(s)
    if opts.decompile && !opts.files.is_empty() {
        for file in &opts.files {
            print!("{}", decompile(&read_source(file)));
        }
        return;
    }

    // Mode 2: Run file(s)
    if !opts.files.is_empty() {
        for file in &opts.files {
            let source = read_source(file);
            let result = run_source(&source, opts.quiet, file, &opts.script_args, opts.format);
            check_result_exit(result);
        }
        return;
    }

    // Mode 3: Read from stdin (REPL-like — just evaluate)
    let input = read_stdin();
    if input.is_empty() {
        return;
    }

    let result = run_source(
        &input,
        opts.quiet,
        "<stdin>",
        &opts.script_args,
        opts.format,
    );
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
