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
use std::fs;
use std::path::Path;




use crate::plugin_config::{Grants, Profile};
use crate::{Bytecode, FeatureDescriptor, Value, bytecode::compile as compile_ast};

/// How diagnostics are written.
#[derive(Clone, Copy, Default)]
pub(crate) enum OutputFormat {
    /// Compiler-style, for humans.
    #[default]
    Human,
    /// One JSON object per line on stdout, for tools.
    Json,
}

impl OutputFormat {
    pub(crate) fn from_i32(value: i32) -> Self {
        if value == 1 { Self::Json } else { Self::Human }
    }
}

/// Per-launcher configuration. One instance is created by `xcl_toolchain_new`.
pub struct Runtime {
    pub(crate) quiet: bool,
    pub(crate) format: OutputFormat,
    /// Canonical lint target and level (`allow`, `warn`, `deny`, `forbid`).
    levels: Vec<(String, String)>,
    /// Positional script arguments (`$1`, `$2`, ...).
    pub(crate) script_args: Vec<String>,
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
    pub(crate) fn set_profile(&mut self, name: &str) -> i32 {
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

    pub(crate) fn set_features(&mut self, csv: Option<&str>, mode: i32) -> i32 {
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

    pub(crate) fn run_file(&mut self, path: &str) -> i32 {
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

    pub(crate) fn run_source(&self, name: &str, source: &str) -> i32 {
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

    pub(crate) fn decompile_file(&self, path: &str) -> i32 {
        let source = match read_source(path) {
            Ok(source) => source,
            Err(code) => return code,
        };
        self.decompile_loaded(Some(Path::new(path)), path, &source)
    }

    pub(crate) fn decompile_source(&self, name: &str, source: &str) -> i32 {
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

    pub(crate) fn lint_file(&self, path: &str) -> i32 {
        let source = match read_source(path) {
            Ok(source) => source,
            Err(code) => return code,
        };
        let stripped = crate::imports::strip_imports(&source, path);
        self.lint_text(path, &stripped)
    }

    pub(crate) fn lint_source(&self, name: &str, source: &str) -> i32 {
        let stripped = crate::imports::strip_imports(source, name);
        self.lint_text(name, &stripped)
    }

    /// Compiles a source file to `.xclb` bytecode without running it.
    pub(crate) fn compile_file(&self, src: &str, out: &str) -> i32 {
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
    pub(crate) fn run_bytecode(&self, path: &str) -> i32 {
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

    pub(crate) fn add_level(&mut self, target: &str, level: &str) -> i32 {
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
pub(crate) fn flushed(code: i32) -> i32 {
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

pub(crate) fn print_plugins(runtime: &Runtime) {
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

pub(crate) fn print_lints() {
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
pub(crate) fn explain_code(code: &str) -> i32 {
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

pub use crate::abi::*;
