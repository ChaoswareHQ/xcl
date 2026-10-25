//! Loadable XCL SDK.
//!
//! The `xcl` launcher is deliberately thin: it parses command-line options,
//! loads this shared library (`xcl_toolchain.dll` / `libxcl_toolchain.so` / `libxcl_toolchain.dylib`),
//! and forwards commands through the C ABI below. Everything that needs the
//! interpreter lives here — the VM, plugins, the allocator, file and import
//! loading, diagnostics, and printing.
//!
//! Optional standard-library features are compiled in and selected by the host
//! before a run (`xcl_toolchain_set_features`). The catalogue lives in
//! `xcl_rt::FEATURE_REGISTRY`.
//!
//! The ABI is intentionally narrow: the host passes UTF-8 paths/source and a
//! few configuration flags, and this library writes program output and
//! diagnostics to the standard streams. Return values are process exit codes
//! (`0` success, non-zero failure), so no allocated value crosses the boundary.

use std::collections::HashSet;
use std::ffi::{CStr, c_char};
use std::fs;
use std::path::Path;




use crate::plugin_config::{Grants, Profile};
use crate::{Bytecode, FeatureDescriptor, Value, bytecode::compile as compile_ast};

/// How diagnostics are written.
#[derive(Clone, Copy, Default)]
enum OutputFormat {
    /// Compiler-style, for humans.
    #[default]
    Human,
    /// One JSON object per line on stdout, for tools.
    Json,
}

impl OutputFormat {
    fn from_i32(value: i32) -> Self {
        if value == 1 { Self::Json } else { Self::Human }
    }
}

/// Per-launcher configuration. One instance is created by `xcl_toolchain_new`.
pub struct Runtime {
    quiet: bool,
    format: OutputFormat,
    /// Canonical lint target and level (`allow`, `warn`, `deny`, `forbid`).
    levels: Vec<(String, String)>,
    /// Positional script arguments (`$1`, `$2`, ...).
    script_args: Vec<String>,
    /// Features selected for this context, in dependency order.
    resolved_features: Vec<&'static FeatureDescriptor>,
    /// Capability profile for this run.
    profile: Profile,
    /// Capabilities the profile grants, after any `[permissions]` overrides.
    grants: Grants,
    /// True when the host chose the profile on the command line. An explicit
    /// profile wins over one named by a `plugin.toml`.
    profile_explicit: bool,
}

impl Default for Runtime {
    fn default() -> Self {
        Self {
            quiet: false,
            format: OutputFormat::Human,
            levels: Vec::new(),
            script_args: Vec::new(),
            resolved_features: resolve_features(FeatureMode::Defaults, &[]).unwrap_or_default(),
            profile: Profile::Trusted,
            grants: Profile::Trusted.grants(),
            profile_explicit: false,
        }
    }
}

/// How the host supplied its feature list.
#[derive(Clone, Copy)]
enum FeatureMode {
    /// Defaults only.
    Defaults,
    /// Defaults plus explicitly requested features.
    DefaultsPlus,
    /// Only the explicitly requested features (plus their dependencies).
    Explicit,
    /// Every compiled feature.
    All,
}

impl FeatureMode {
    fn from_i32(value: i32) -> Self {
        match value {
            1 => Self::DefaultsPlus,
            2 => Self::Explicit,
            3 => Self::All,
            _ => Self::Defaults,
        }
    }
}

fn find_feature(name: &str) -> Option<&'static FeatureDescriptor> {
    xcl_rt::FEATURE_REGISTRY
        .iter()
        .find(|feature| feature.name == name || feature.aliases.contains(&name))
}

/// The compiled plugin a feature installs.
///
/// A feature and its plugin normally share a name. When they do not, the
/// feature's domains decide: a feature installs the plugin that publishes
/// them.
fn plugin_for(feature: &FeatureDescriptor) -> Option<&'static crate::PluginDescriptor> {
    xcl_rt::PLUGIN_REGISTRY
        .iter()
        .find(|plugin| plugin.name == feature.name)
        .or_else(|| {
            xcl_rt::PLUGIN_REGISTRY.iter().find(|plugin| {
                plugin
                    .domains
                    .iter()
                    .any(|alias| feature.domains.contains(&alias.domain))
            })
        })
}

/// Explains why a profile refused the selected plugins.
fn report_denied(profile: Profile, denied: &[(&'static str, Vec<&'static str>)]) {
    for (plugin, missing) in denied {
        eprintln!(
            "xcl: error: the `{}` profile does not grant `{}` the permissions: {}",
            profile.name(),
            plugin,
            missing.join(", ")
        );
    }
    eprintln!(
        "xcl: note: use `--profile trusted`, add to `[permissions]` in plugin.toml, \
         or install a feature that fits the profile"
    );
}

fn available_features() -> String {
    let mut names: Vec<&'static str> = xcl_rt::FEATURE_REGISTRY
        .iter()
        .map(|feature| feature.name)
        .collect();
    names.sort_unstable();
    names.join(", ")
}

/// Resolves a feature selection to an install order, dependencies first.
fn resolve_features(
    mode: FeatureMode,
    requested: &[String],
) -> Result<Vec<&'static FeatureDescriptor>, String> {
    let all: Vec<&'static FeatureDescriptor> = xcl_rt::FEATURE_REGISTRY.iter().collect();
    let defaults = || all.iter().copied().filter(|feature| feature.default);

    let mut roots: Vec<&'static FeatureDescriptor> = Vec::new();
    match mode {
        FeatureMode::Defaults => roots.extend(defaults()),
        FeatureMode::All => roots.extend(all.iter().copied()),
        FeatureMode::DefaultsPlus | FeatureMode::Explicit => {
            if matches!(mode, FeatureMode::DefaultsPlus) {
                roots.extend(defaults());
            }
            for name in requested {
                let feature = find_feature(name).ok_or_else(|| {
                    format!(
                        "unknown feature `{}` (available: {})",
                        name,
                        available_features()
                    )
                })?;
                roots.push(feature);
            }
        }
    }

    let mut resolved = Vec::new();
    let mut done = HashSet::new();
    for feature in roots {
        visit_feature(feature, &mut resolved, &mut done, &mut HashSet::new())?;
    }
    Ok(resolved)
}

fn visit_feature(
    feature: &'static FeatureDescriptor,
    resolved: &mut Vec<&'static FeatureDescriptor>,
    done: &mut HashSet<&'static str>,
    visiting: &mut HashSet<&'static str>,
) -> Result<(), String> {
    if done.contains(feature.name) {
        return Ok(());
    }
    if !visiting.insert(feature.name) {
        return Err(format!("feature dependency cycle at `{}`", feature.name));
    }
    for required in feature.requires {
        let dependency = find_feature(required).ok_or_else(|| {
            format!(
                "feature `{}` requires unknown feature `{}`",
                feature.name, required
            )
        })?;
        visit_feature(dependency, resolved, done, visiting)?;
    }
    visiting.remove(feature.name);
    done.insert(feature.name);
    resolved.push(feature);
    Ok(())
}

impl Runtime {
    /// Creates a VM with exactly the selected features installed.
    ///
    /// A plugin whose declared permissions the active profile does not grant is
    /// refused here, before any of its natives are visible to the script. The
    /// error names every denied plugin and every missing capability, so the fix
    /// is a profile change or a `plugin.toml` grant, never a silent downgrade.
    fn new_vm(&self) -> Result<crate::VM, Vec<(&'static str, Vec<&'static str>)>> {
        let denied = self.denied_plugins();
        if !denied.is_empty() {
            return Err(denied);
        }

        let mut vm = crate::VM::new();
        for feature in &self.resolved_features {
            (feature.install)(&mut vm);
        }
        Ok(vm)
    }

    /// Plugins that want more than the active profile grants, and what they
    /// are missing.
    fn denied_plugins(&self) -> Vec<(&'static str, Vec<&'static str>)> {
        let mut denied = Vec::new();
        for feature in &self.resolved_features {
            let Some(plugin) = plugin_for(feature) else {
                continue;
            };
            let missing = self.grants.missing(plugin.permissions);
            if !missing.is_empty() {
                denied.push((plugin.name, missing));
            }
        }
        denied
    }

    /// Sets the capability profile. Returns non-zero for an unknown name.
    fn set_profile(&mut self, name: &str) -> i32 {
        match Profile::from_name(name) {
            Some(profile) => {
                self.profile = profile;
                self.grants = profile.grants();
                self.profile_explicit = true;
                0
            }
            None => {
                eprintln!("xcl: error: unknown profile `{}`", name);
                eprintln!(
                    "xcl: note: expected one of {}",
                    Profile::ALL
                        .iter()
                        .map(|profile| profile.name())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                1
            }
        }
    }

    /// Applies the profile and permission overrides from a `plugin.toml`.
    ///
    /// An explicit `--profile` wins: the host gets the last word on what an
    /// untrusted directory is allowed to reach.
    fn apply_plugin_config(&mut self, config: &crate::plugin_config::Config) {
        if !self.profile_explicit {
            self.profile = config.profile;
            self.grants = config.grants;
        }
    }

    fn set_features(&mut self, csv: Option<&str>, mode: i32) -> i32 {
        let mode = FeatureMode::from_i32(mode);
        let requested: Vec<String> = csv
            .unwrap_or("")
            .split([',', ' ', '\t', '\n', '\r'])
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_string)
            .collect();

        match resolve_features(mode, &requested) {
            Ok(features) => {
                self.resolved_features = features;
                0
            }
            Err(message) => {
                eprintln!("xcl: error: {}", message);
                1
            }
        }
    }

    fn set_args(&self, vm: &mut crate::VM, name: &str) {
        let args = self
            .script_args
            .iter()
            .map(|arg| Value::from_str_boxed(arg.clone()))
            .collect();
        vm.set_script_args(name, args);
    }

    fn run_file(&mut self, path: &str) -> i32 {
        let requested = Path::new(path);
        if requested.is_dir() {
            return self.run_plugin_dir(requested);
        }

        let source = match read_source(path) {
            Ok(source) => source,
            Err(code) => return code,
        };
        self.run_loaded(Some(requested), path, &source)
    }

    /// Runs a directory plugin: reads `plugin.toml`, applies its profile, and
    /// runs its entry script.
    fn run_plugin_dir(&mut self, dir: &Path) -> i32 {
        let config = match crate::plugin_config::Config::load(dir) {
            Ok(config) => config,
            Err(error) => {
                eprintln!("{}", error.render());
                return 1;
            }
        };
        self.apply_plugin_config(&config);

        let entry = config.entry_display();
        let source = match fs::read_to_string(&config.entry) {
            Ok(source) => source,
            Err(err) => {
                eprintln!(
                    "xcl: error: plugin `{}` cannot read its entry `{}`: {err}",
                    config.label(),
                    entry
                );
                return 1;
            }
        };
        self.run_loaded(Some(&config.entry), &entry, &source)
    }

    fn run_source(&self, name: &str, source: &str) -> i32 {
        self.run_loaded(None, name, source)
    }

    fn run_loaded(&self, entry: Option<&Path>, name: &str, source: &str) -> i32 {
        let ast = match crate::imports::load(entry, name, source) {
            Ok(ast) => ast,
            Err(failure) => {
                self.report_failure(failure);
                return 1;
            }
        };

        let mut vm = match self.new_vm() {
            Ok(vm) => vm,
            Err(denied) => {
                report_denied(self.profile, &denied);
                return 1;
            }
        };
        self.set_args(&mut vm, name);
        let result = crate::evaluate_ast(&ast, &mut vm);
        self.finish(result)
    }

    fn finish(&self, result: Value) -> i32 {
        if !self.quiet && !result.is_nil() {
            print_value(result);
        }
        if let Some(map) = result.as_map()
            && map.get("tag").and_then(|tag| tag.as_str()) == Some("Err")
        {
            let message = map
                .get("message")
                .and_then(|value| value.as_str())
                .unwrap_or("unknown error");
            eprintln!("xcl: runtime error: {}", message);
            return 1;
        }
        0
    }

    fn decompile_file(&self, path: &str) -> i32 {
        let source = match read_source(path) {
            Ok(source) => source,
            Err(code) => return code,
        };
        self.decompile_loaded(Some(Path::new(path)), path, &source)
    }

    fn decompile_source(&self, name: &str, source: &str) -> i32 {
        self.decompile_loaded(None, name, source)
    }

    fn decompile_loaded(&self, entry: Option<&Path>, name: &str, source: &str) -> i32 {
        let ast = match crate::imports::load(entry, name, source) {
            Ok(ast) => ast,
            Err(failure) => {
                self.report_failure(failure);
                return 1;
            }
        };
        print!(
            "{}",
            crate::decompile::decompile(&crate::bytecode::compile(&ast))
        );
        0
    }

    fn lint_file(&self, path: &str) -> i32 {
        let source = match read_source(path) {
            Ok(source) => source,
            Err(code) => return code,
        };
        let stripped = crate::imports::strip_imports(&source, path);
        self.lint_text(path, &stripped)
    }

    fn lint_source(&self, name: &str, source: &str) -> i32 {
        let stripped = crate::imports::strip_imports(source, name);
        self.lint_text(name, &stripped)
    }

    /// Compiles a source file to `.xclb` bytecode without running it.
    fn compile_file(&self, src: &str, out: &str) -> i32 {
        let source = match read_source(src) {
            Ok(source) => source,
            Err(code) => return code,
        };
        let ast = match crate::imports::load(Some(Path::new(src)), src, &source) {
            Ok(ast) => ast,
            Err(failure) => {
                self.report_failure(failure);
                return 1;
            }
        };
        let bytecode = compile_ast(&ast);
        match bytecode.to_file(Path::new(out)) {
            Ok(()) => {
                flush_streams();
                0
            }
            Err(err) => {
                eprintln!("xcl: error: cannot write '{}': {}", out, err);
                flush_streams();
                1
            }
        }
    }

    /// Decodes a `.xclb` bytecode file and runs it. Does not touch the
    /// lexer/parser/compiler — this is the whole point of the split.
    fn run_bytecode(&self, path: &str) -> i32 {
        let bytecode = match Bytecode::from_file(Path::new(path)) {
            Ok(bc) => bc,
            Err(err) => {
                eprintln!("xcl: error: cannot load '{}': {}", path, err);
                return flushed(1);
            }
        };

        let mut vm = match self.new_vm() {
            Ok(vm) => vm,
            Err(denied) => {
                report_denied(self.profile, &denied);
                return 1;
            }
        };
        self.set_args(&mut vm, path);
        let result = vm.run(&bytecode);
        self.finish(result)
    }

    fn lint_text(&self, file: &str, source: &str) -> i32 {
        let report = crate::lint::check_source(file, source, &self.lint_config());
        let map = crate::lint::SourceMap::new(file, source);

        match self.format {
            OutputFormat::Human => eprint!("{}", report.render_human(&map)),
            OutputFormat::Json => {
                print!("{}", report.render_json(&map));
                let errors = report.count(crate::lint::Level::Error);
                if errors > 0 {
                    eprintln!(
                        "xcl: aborting due to {} previous error{}",
                        errors,
                        if errors == 1 { "" } else { "s" }
                    );
                }
            }
        }
        i32::from(report.has_errors())
    }

    fn lint_config(&self) -> crate::lint::LintConfig {
        let mut config = crate::lint::LintConfig::new();
        for (target, level) in &self.levels {
            let level =
                crate::lint::LintLevel::from_name(level).unwrap_or(crate::lint::LintLevel::Warn);
            config = config.set(target.clone(), level);
        }
        config
    }

    fn add_level(&mut self, target: &str, level: &str) -> i32 {
        match crate::lint::normalize_target(target) {
            Some(canonical) => {
                self.levels.push((canonical, level.to_string()));
                0
            }
            None => {
                eprintln!("xcl: error: unknown lint: `{}`", target);
                eprintln!("xcl: note: run `xcl --list-lints` to see rule codes, names, and groups");
                1
            }
        }
    }

    fn report_failure(&self, failure: crate::imports::Failure) {
        match failure {
            crate::imports::Failure::Parse {
                file,
                source,
                errors,
            } => self.report_parse_errors(&source, &file, &errors),
            crate::imports::Failure::Resolve { source, error } => {
                let file = error.span.file.clone();
                self.report_parse_errors(&source, &file, std::slice::from_ref(&error));
            }
        }
    }

    fn report_parse_errors(&self, source: &str, file: &str, errors: &[crate::ParseError]) {
        match self.format {
            OutputFormat::Human => {
                for error in errors {
                    eprintln!("{}", error.render());
                }
            }
            OutputFormat::Json => {
                let report = crate::lint::parse_error_report(errors);
                let map = crate::lint::SourceMap::new(file, source);
                print!("{}", report.render_json(&map));
            }
        }
        eprintln!(
            "xcl: aborting due to {} previous error{}",
            errors.len(),
            if errors.len() == 1 { "" } else { "s" }
        );
    }
}

fn read_source(path: &str) -> Result<String, i32> {
    fs::read_to_string(path).map_err(|err| {
        eprintln!("xcl: error: cannot read '{}': {}", path, err);
        1
    })
}

/// Flushes both streams before crossing back into the launcher.
///
/// The runtime owns Rust's buffered writers, and the launcher may exit with
/// `process::exit`, which does not run destructors. Without this, output from
/// `print` (which does not append a newline) can be lost.
fn flush_streams() {
    use std::io::Write;
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
}

/// Runs a body and flushes its output before returning the exit code.
fn flushed(code: i32) -> i32 {
    flush_streams();
    code
}

fn print_value(value: Value) {
    if let Some(text) = value.as_str() {
        println!("{}", text);
    } else if value.is_int() {
        println!("{}", value.as_i48());
    } else if value.is_float() {
        println!("{}", value.as_f64());
    } else if value.is_bool() {
        println!("{}", value.as_bool());
    } else if let Some(map) = value.as_map() {
        println!("{{");
        for (key, item) in map {
            print!("  {}: ", key);
            print_value(*item);
        }
        println!("}}");
    } else if let Some(array) = value.as_array() {
        print!("[");
        for (index, item) in array.iter().enumerate() {
            if index > 0 {
                print!(", ");
            }
            print_value(*item);
        }
        println!("]");
    } else if value.is_nil() {
        println!("none");
    } else {
        println!("<value>");
    }
}

fn print_plugins(runtime: &Runtime) {
    println!(
        "{:<8} {:<12} {:<8} {:<34} {:<28} SUMMARY",
        "PLUGIN", "ALIASES", "DEFAULT", "DOMAINS", "PERMISSIONS"
    );
    let mut features: Vec<_> = xcl_rt::FEATURE_REGISTRY.iter().collect();
    features.sort_by_key(|feature| (!feature.default, feature.name));
    for feature in features {
        println!(
            "{:<8} {:<12} {:<8} {:<34} {:<28} {}",
            feature.name,
            feature.aliases.join(", "),
            if feature.default { "yes" } else { "no" },
            feature.domains.join(", "),
            permissions_of(feature.name),
            feature.summary
        );
    }
    println!();
    println!(
        "Install a plugin with --features <name>, or use --no-default-features to \
         start from an empty runtime."
    );
    println!(
        "Permissions are declared (Cargo.toml metadata for compiled plugins, \
         plugin.toml for external ones) and checked against the active profile."
    );
    println!(
        "Active profile: {} — {} [{}]",
        runtime.profile.name(),
        runtime.profile.summary(),
        runtime.grants.names().join(", ")
    );
    for profile in Profile::ALL {
        if profile == runtime.profile {
            continue;
        }
        println!("  {:<11} {}", profile.name(), profile.summary());
    }
}

/// Capabilities declared by the plugin that owns `feature`.
///
/// A feature and its plugin share a name; when they do not, the feature's
/// domains decide which plugin to ask.
fn permissions_of(feature: &str) -> String {
    xcl_rt::PLUGIN_REGISTRY
        .iter()
        .find(|plugin| plugin.name == feature)
        .map(|plugin| plugin.permissions.join(", "))
        .unwrap_or_default()
}

/// The C ABI this SDK exports.
const ABI_EXPORTS: &[(&str, &str)] = &[
    ("xcl_toolchain_abi_version", "uint32_t xcl_toolchain_abi_version(void)"),
    ("xcl_toolchain_abi_list", "int xcl_toolchain_abi_list(XclRuntime*)"),
    ("xcl_toolchain_new", "XclRuntime* xcl_toolchain_new(void)"),
    ("xcl_toolchain_free", "void xcl_toolchain_free(XclRuntime*)"),
    (
        "xcl_toolchain_set_quiet",
        "void xcl_toolchain_set_quiet(XclRuntime*, int quiet)",
    ),
    (
        "xcl_toolchain_set_format",
        "void xcl_toolchain_set_format(XclRuntime*, int format)",
    ),
    (
        "xcl_toolchain_add_level",
        "int xcl_toolchain_add_level(XclRuntime*, const char* target, const char* level)",
    ),
    (
        "xcl_toolchain_set_script_args",
        "void xcl_toolchain_set_script_args(XclRuntime*, size_t argc, const char* const* argv)",
    ),
    (
        "xcl_toolchain_set_features",
        "int xcl_toolchain_set_features(XclRuntime*, const char* csv, int mode)",
    ),
    (
        "xcl_toolchain_set_profile",
        "int xcl_toolchain_set_profile(XclRuntime*, const char* name)",
    ),
    (
        "xcl_toolchain_list_plugins",
        "int xcl_toolchain_list_plugins(XclRuntime*)",
    ),
    ("xcl_toolchain_manifest", "int xcl_toolchain_manifest(XclRuntime*)"),
    (
        "xcl_toolchain_run_file",
        "int xcl_toolchain_run_file(XclRuntime*, const char* path)",
    ),
    (
        "xcl_toolchain_run_source",
        "int xcl_toolchain_run_source(XclRuntime*, const char* name, const char* source, size_t len)",
    ),
    (
        "xcl_toolchain_decompile_file",
        "int xcl_toolchain_decompile_file(XclRuntime*, const char* path)",
    ),
    (
        "xcl_toolchain_decompile_source",
        "int xcl_toolchain_decompile_source(XclRuntime*, const char* name, const char* source, size_t len)",
    ),
    (
        "xcl_toolchain_lint_file",
        "int xcl_toolchain_lint_file(XclRuntime*, const char* path)",
    ),
    (
        "xcl_toolchain_lint_source",
        "int xcl_toolchain_lint_source(XclRuntime*, const char* name, const char* source, size_t len)",
    ),
    ("xcl_toolchain_list_lints", "int xcl_toolchain_list_lints(XclRuntime*)"),
    (
        "xcl_toolchain_explain",
        "int xcl_toolchain_explain(XclRuntime*, const char* code)",
    ),
    (
        "xcl_toolchain_compile_file",
        "int xcl_toolchain_compile_file(XclRuntime*, const char* src, const char* out)",
    ),
    (
        "xcl_toolchain_run_bytecode",
        "int xcl_toolchain_run_bytecode(XclRuntime*, const char* path)",
    ),
];

fn print_abi() {
    println!("XCL toolchain ABI v{}", xcl_toolchain_abi_version());
    for (_, signature) in ABI_EXPORTS {
        println!("  {}", signature);
    }
}

fn print_lints() {
    println!(
        "{:<7} {:<24} {:<12} {:<8} SUMMARY",
        "CODE", "NAME", "GROUP", "DEFAULT"
    );
    for rule in crate::lint::RULES {
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
    for (code, summary) in crate::PARSE_ERROR_CODES {
        println!("  {:<7} {}", code, summary);
    }
    println!();
    println!("Groups:  correctness, suspicious, style, complexity, perf, restriction");
    println!("Levels:  allow, warn, deny, forbid");
    println!("Targets: a code (W0001), a name (unused_binding), a group, or `warnings`");
    println!("Explain: xcl --explain W0001");
}

/// Prints rule documentation, following a rename or a removal.
fn explain_code(code: &str) -> i32 {
    let code = code.trim();
    if let Some(text) = crate::lint::explain(&code.to_ascii_uppercase()) {
        print!("{text}");
        return 0;
    }

    match crate::lint::resolve_name(code) {
        Ok(rule) => {
            print!("{}", rule.render());
            0
        }
        Err(crate::lint::Resolution::Renamed { to }) => {
            eprintln!("xcl: note: `{}` was renamed to `{}`", code, to);
            match crate::lint::rule_by_name(to) {
                Some(rule) => {
                    print!("{}", rule.render());
                    0
                }
                None => 1,
            }
        }
        Err(crate::lint::Resolution::Removed { replaced_by }) => {
            eprintln!("xcl: error: `{}` has been removed", code);
            if let Some(replacement) = replaced_by {
                eprintln!("xcl: note: use `{}` instead", replacement);
            }
            1
        }
        Err(crate::lint::Resolution::Unknown) => {
            eprintln!("xcl: error: unknown lint: `{}`", code);
            eprintln!("xcl: note: run `xcl --list-lints` to see rule codes, names, and groups");
            1
        }
    }
}

// ── C ABI ───────────────────────────────────────────────────────────────────

/// ABI version understood by this SDK. The launcher refuses a mismatch.
pub const ABI_VERSION: u32 = 7;

#[unsafe(no_mangle)]
pub extern "C" fn xcl_toolchain_abi_version() -> u32 {
    ABI_VERSION
}

/// Prints the C ABI this SDK exports.
///
/// # Safety
/// `ctx` may be null; the function does not touch the runtime state.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xcl_toolchain_abi_list(_ctx: *mut Runtime) -> i32 {
    print_abi();
    flushed(0)
}

/// Borrows the runtime behind an opaque pointer.
///
/// # Safety
/// `ctx` must be a pointer returned by [`xcl_toolchain_new`] that has not been
/// freed yet.
unsafe fn runtime<'a>(ctx: *mut Runtime) -> Option<&'a mut Runtime> {
    unsafe { ctx.as_mut() }
}

/// Borrows a NUL-terminated UTF-8 argument.
///
/// # Safety
/// `ptr` must be null or point to a valid NUL-terminated string.
unsafe fn text<'a>(ptr: *const c_char) -> Option<&'a str> {
    if ptr.is_null() {
        return None;
    }
    unsafe { CStr::from_ptr(ptr) }.to_str().ok()
}

/// Borrows a UTF-8 source buffer of `len` bytes.
///
/// # Safety
/// `ptr` must be valid for `len` bytes (or null when `len` is zero).
unsafe fn bytes<'a>(ptr: *const u8, len: usize) -> Option<&'a str> {
    if len == 0 {
        return Some("");
    }
    if ptr.is_null() {
        return None;
    }
    core::str::from_utf8(unsafe { core::slice::from_raw_parts(ptr, len) }).ok()
}

/// Creates a runtime instance.
///
/// # Safety
/// The returned pointer must be passed to [`xcl_toolchain_free`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xcl_toolchain_new() -> *mut Runtime {
    Box::into_raw(Box::new(Runtime::default()))
}

/// Frees a runtime instance. A null pointer is ignored.
///
/// # Safety
/// `ctx` must come from [`xcl_toolchain_new`] and must not be used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xcl_toolchain_free(ctx: *mut Runtime) {
    if !ctx.is_null() {
        unsafe { drop(Box::from_raw(ctx)) };
    }
}

/// Sets whether the result value is printed after a successful run.
///
/// # Safety
/// `ctx` must be a live runtime pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xcl_toolchain_set_quiet(ctx: *mut Runtime, quiet: i32) {
    if let Some(runtime) = unsafe { runtime(ctx) } {
        runtime.quiet = quiet != 0;
    }
}

/// Sets the diagnostic format: `0` human, `1` JSON.
///
/// # Safety
/// `ctx` must be a live runtime pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xcl_toolchain_set_format(ctx: *mut Runtime, format: i32) {
    if let Some(runtime) = unsafe { runtime(ctx) } {
        runtime.format = OutputFormat::from_i32(format);
    }
}

/// Adds a lint level override (`-A/-W/-D/-F`). Returns non-zero when the
/// target is unknown.
///
/// # Safety
/// `ctx` must be a live runtime pointer; the strings must be valid UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xcl_toolchain_add_level(
    ctx: *mut Runtime,
    target: *const c_char,
    level: *const c_char,
) -> i32 {
    let Some(runtime) = (unsafe { runtime(ctx) }) else {
        return 1;
    };
    let (Some(target), Some(level)) = (unsafe { text(target) }, unsafe { text(level) }) else {
        return flushed(1);
    };
    flushed(runtime.add_level(target, level))
}

/// Copies the positional script arguments (`$1`, `$2`, ...).
///
/// # Safety
/// `argv` must point to `argc` valid NUL-terminated strings.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xcl_toolchain_set_script_args(
    ctx: *mut Runtime,
    argc: usize,
    argv: *const *const c_char,
) {
    let Some(runtime) = (unsafe { runtime(ctx) }) else {
        return;
    };
    if argc > 0 && argv.is_null() {
        return;
    }
    let mut args = Vec::with_capacity(argc);
    for index in 0..argc {
        let pointer = unsafe { *argv.add(index) };
        if let Some(arg) = unsafe { text(pointer) } {
            args.push(arg.to_string());
        }
    }
    runtime.script_args = args;
}

/// Selects which compiled features to install.
///
/// `mode` is `0` defaults, `1` defaults plus `csv`, `2` only `csv` (plus
/// dependencies), and `3` every compiled feature. A null `csv` behaves like an
/// empty list. Returns non-zero and prints a diagnostic for an unknown feature.
///
/// # Safety
/// `ctx` must be a live runtime pointer; `csv` must be null or valid UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xcl_toolchain_set_features(
    ctx: *mut Runtime,
    csv: *const c_char,
    mode: i32,
) -> i32 {
    let Some(runtime) = (unsafe { runtime(ctx) }) else {
        return flushed(1);
    };
    let csv = if csv.is_null() {
        None
    } else {
        match unsafe { text(csv) } {
            Some(csv) => Some(csv),
            None => {
                eprintln!("xcl: error: invalid UTF-8 feature list");
                return flushed(1);
            }
        }
    };
    flushed(runtime.set_features(csv, mode))
}

/// Selects the capability profile.
///
/// `sandbox`, `automation`, `game-mod`, `trusted`, or `deny-all`. An explicit
/// profile overrides one named by an external plugin's `plugin.toml`. Returns
/// non-zero and prints a diagnostic for an unknown name.
///
/// # Safety
/// `ctx` must be a live runtime pointer; `name` must be null or valid UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xcl_toolchain_set_profile(ctx: *mut Runtime, name: *const c_char) -> i32 {
    let Some(runtime) = (unsafe { runtime(ctx) }) else {
        return flushed(1);
    };
    let Some(name) = (unsafe { text(name) }) else {
        eprintln!("xcl: error: invalid UTF-8 profile name");
        return flushed(1);
    };
    flushed(runtime.set_profile(name))
}

/// Prints the compiled plugin catalogue.
///
/// # Safety
/// `ctx` must be a live runtime pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xcl_toolchain_list_plugins(ctx: *mut Runtime) -> i32 {
    let Some(runtime) = (unsafe { runtime(ctx) }) else {
        return flushed(1);
    };
    print_plugins(runtime);
    flushed(0)
}

/// Prints the generated plugin manifest (TOML) for the compiled registry.
///
/// The manifest is a projection of the registry populated by the registration
/// macros, so it always matches the code; it is not a hand-maintained file.
///
/// # Safety
/// `ctx` must be a live runtime pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xcl_toolchain_manifest(_ctx: *mut Runtime) -> i32 {
    print!("{}", crate::manifest::manifest_toml());
    flushed(0)
}

/// Reads, resolves imports for, compiles, and runs a file.
///
/// # Safety
/// `ctx` must be a live runtime pointer and `path` a valid UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xcl_toolchain_run_file(ctx: *mut Runtime, path: *const c_char) -> i32 {
    let Some(runtime) = (unsafe { runtime(ctx) }) else {
        return 1;
    };
    let Some(path) = (unsafe { text(path) }) else {
        eprintln!("xcl: error: invalid UTF-8 path");
        return flushed(1);
    };
    flushed(runtime.run_file(path))
}

/// Runs source text. `name` is used in diagnostics.
///
/// # Safety
/// `ctx` must be a live runtime pointer; the strings must be valid UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xcl_toolchain_run_source(
    ctx: *mut Runtime,
    name: *const c_char,
    source: *const u8,
    source_len: usize,
) -> i32 {
    let Some(runtime) = (unsafe { runtime(ctx) }) else {
        return 1;
    };
    let (Some(name), Some(source)) = (unsafe { text(name) }, unsafe { bytes(source, source_len) })
    else {
        eprintln!("xcl: error: invalid UTF-8 source");
        return flushed(1);
    };
    flushed(runtime.run_source(name, source))
}

/// Disassembles a source file after resolving its imports.
///
/// # Safety
/// `ctx` must be a live runtime pointer and `path` a valid UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xcl_toolchain_decompile_file(ctx: *mut Runtime, path: *const c_char) -> i32 {
    let Some(runtime) = (unsafe { runtime(ctx) }) else {
        return 1;
    };
    let Some(path) = (unsafe { text(path) }) else {
        eprintln!("xcl: error: invalid UTF-8 path");
        return flushed(1);
    };
    flushed(runtime.decompile_file(path))
}

/// Disassembles source text.
///
/// # Safety
/// `ctx` must be a live runtime pointer; the strings must be valid UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xcl_toolchain_decompile_source(
    ctx: *mut Runtime,
    name: *const c_char,
    source: *const u8,
    source_len: usize,
) -> i32 {
    let Some(runtime) = (unsafe { runtime(ctx) }) else {
        return 1;
    };
    let (Some(name), Some(source)) = (unsafe { text(name) }, unsafe { bytes(source, source_len) })
    else {
        eprintln!("xcl: error: invalid UTF-8 source");
        return flushed(1);
    };
    flushed(runtime.decompile_source(name, source))
}

/// Lints a source file after removing import lines.
///
/// # Safety
/// `ctx` must be a live runtime pointer and `path` a valid UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xcl_toolchain_lint_file(ctx: *mut Runtime, path: *const c_char) -> i32 {
    let Some(runtime) = (unsafe { runtime(ctx) }) else {
        return 1;
    };
    let Some(path) = (unsafe { text(path) }) else {
        eprintln!("xcl: error: invalid UTF-8 path");
        return flushed(1);
    };
    flushed(runtime.lint_file(path))
}

/// Lints source text.
///
/// # Safety
/// `ctx` must be a live runtime pointer; the strings must be valid UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xcl_toolchain_lint_source(
    ctx: *mut Runtime,
    name: *const c_char,
    source: *const u8,
    source_len: usize,
) -> i32 {
    let Some(runtime) = (unsafe { runtime(ctx) }) else {
        return 1;
    };
    let (Some(name), Some(source)) = (unsafe { text(name) }, unsafe { bytes(source, source_len) })
    else {
        eprintln!("xcl: error: invalid UTF-8 source");
        return flushed(1);
    };
    flushed(runtime.lint_source(name, source))
}

/// Prints the lint catalogue.
///
/// # Safety
/// `ctx` must be a live runtime pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xcl_toolchain_list_lints(_ctx: *mut Runtime) -> i32 {
    print_lints();
    flushed(0)
}

/// Prints one rule's documentation. Returns non-zero for an unknown code.
///
/// # Safety
/// `ctx` must be a live runtime pointer and `code` a valid UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xcl_toolchain_explain(_ctx: *mut Runtime, code: *const c_char) -> i32 {
    let Some(code) = (unsafe { text(code) }) else {
        return flushed(1);
    };
    flushed(explain_code(code))
}

/// Compiles a source file to `.xclb` bytecode without running it.
///
/// `src` is the path to the `.xcl` source file. `out` is the output `.xclb`
/// path.  Imports are resolved exactly like `run_file`.  Returns `0` on
/// success, non-zero on error.
///
/// # Safety
/// `ctx` must be a live runtime pointer; the strings must be valid UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xcl_toolchain_compile_file(
    ctx: *mut Runtime,
    src: *const c_char,
    out: *const c_char,
) -> i32 {
    let Some(runtime) = (unsafe { runtime(ctx) }) else {
        return 1;
    };
    let (Some(src), Some(out)) = (unsafe { text(src) }, unsafe { text(out) }) else {
        eprintln!("xcl: error: invalid UTF-8 path");
        return flushed(1);
    };
    flushed(runtime.compile_file(src, out))
}

/// Decodes a `.xclb` bytecode file and runs it on a VM built from the
/// selected features.  Does not touch the lexer/parser/compiler.
///
/// # Safety
/// `ctx` must be a live runtime pointer and `path` a valid UTF-8 string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xcl_toolchain_run_bytecode(ctx: *mut Runtime, path: *const c_char) -> i32 {
    let Some(runtime) = (unsafe { runtime(ctx) }) else {
        return 1;
    };
    let Some(path) = (unsafe { text(path) }) else {
        eprintln!("xcl: error: invalid UTF-8 path");
        return flushed(1);
    };
    flushed(runtime.run_bytecode(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    #[test]
    fn abi_reports_its_version() {
        assert_eq!(xcl_toolchain_abi_version(), ABI_VERSION);
    }

    #[test]
    fn feature_selection_uses_exact_names() {
        let mut runtime = Runtime::default();
        assert_eq!(runtime.set_features(Some("std"), 2), 0);
        assert_eq!(runtime.set_features(Some("std_util"), 2), 1);
        assert_eq!(runtime.set_features(Some("missing_feature"), 2), 1);
    }

    #[test]
    fn exported_run_source_returns_exit_codes() {
        let ctx = unsafe { xcl_toolchain_new() };
        assert!(!ctx.is_null());
        unsafe { xcl_toolchain_set_quiet(ctx, 1) };

        let name = CString::new("<test>").expect("static name");
        let good = b"$x = 1 + 2";
        let status = unsafe { xcl_toolchain_run_source(ctx, name.as_ptr(), good.as_ptr(), good.len()) };
        assert_eq!(status, 0);

        let bad = b"$x = ";
        let status = unsafe { xcl_toolchain_run_source(ctx, name.as_ptr(), bad.as_ptr(), bad.len()) };
        assert_eq!(status, 1);

        unsafe { xcl_toolchain_free(ctx) };
    }

    #[test]
    fn freeing_null_is_ignored() {
        unsafe { xcl_toolchain_free(std::ptr::null_mut()) };
    }
}
