//! `plugin.toml` — configuration for plugins that are not compiled in.
//!
//! # Two kinds of plugin, two places to configure them
//!
//! A **Rust library plugin** is linked into the runtime. Its description lives
//! in `Cargo.toml` under `[package.metadata.xcl.plugins.<name>]`, and the
//! `#[xcl_plugin]` macro bakes the same values into `PLUGIN_REGISTRY`. That is
//! the form `xcl list` and `xcl manifest` read, and it cannot drift from the
//! code because it *is* the code.
//!
//! An **external plugin** is a directory the runtime did not compile. It carries
//! a `plugin.toml` next to its entry script:
//!
//! ```toml
//! [plugin]
//! name = "greet"
//! entry = "main.xcl"
//! profile = "sandbox"
//! summary = "says hello"
//!
//! [permissions]
//! fs = ["read"]
//! ```
//!
//! and is run with `xcl <dir>`.
//!
//! # Why a hand-written parser
//!
//! `plugin.toml` is untrusted input: it arrives with the plugin. A strict
//! reader that understands exactly the keys below and rejects everything else
//! is smaller (no `toml` crate in the DLL) and safer (no silently ignored
//! keys, no dotted-key ambiguity, no date/time types) than a general one.
//!
//! # What is checked
//!
//! Only what can be enforced today: the **profile** decides which capabilities
//! are granted, and a plugin that declares a capability the profile does not
//! grant is refused before it runs. Execution limits (`steps`, `time`, `memory`)
//! need an interpreter budget and are deliberately not accepted yet: a key that
//! is parsed but ignored would be worse than no key at all.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// File name of an external plugin's configuration.
pub const FILE_NAME: &str = "plugin.toml";

/// Schema version understood by this build.
pub const ABI: u32 = 1;

/// Largest `plugin.toml` accepted, in bytes.
///
/// A configuration file is a page, not a payload.
pub const MAX_BYTES: u64 = 64 * 1024;

/// Entry script used when `[plugin] entry` is absent.
pub const DEFAULT_ENTRY: &str = "main.xcl";

/// How much of the host an external plugin may reach.
///
/// The default for an external plugin is [`Profile::Sandbox`]: a plugin that
/// was not compiled into the runtime is not trusted by default. A compiled
/// plugin keeps the host's profile, which defaults to [`Profile::Trusted`]
/// because the developer chose to link it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Profile {
    /// Console and clock only. No files, no network, no windows.
    Sandbox,
    /// Sandbox plus file access, one outbound network connection, and process
    /// launching — the profile a build/deploy script needs.
    Automation,
    /// Sandbox plus game assets. No files, network, or windows.
    GameMod,
    /// Everything. What the embedded runtime uses unless told otherwise.
    Trusted,
    /// Nothing. The script parses and compiles, then fails at its first effect.
    DenyAll,
}

impl Profile {
    /// Every profile, in the order `xcl list` should show them.
    pub const ALL: [Profile; 5] = [
        Profile::Sandbox,
        Profile::Automation,
        Profile::GameMod,
        Profile::Trusted,
        Profile::DenyAll,
    ];

    /// Parses a profile name. `game-mod` and `game_mod` are the same profile.
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "sandbox" => Profile::Sandbox,
            "automation" => Profile::Automation,
            "game-mod" | "game_mod" => Profile::GameMod,
            "trusted" => Profile::Trusted,
            "deny-all" | "deny_all" => Profile::DenyAll,
            _ => return None,
        })
    }

    /// The canonical name.
    pub fn name(self) -> &'static str {
        match self {
            Profile::Sandbox => "sandbox",
            Profile::Automation => "automation",
            Profile::GameMod => "game-mod",
            Profile::Trusted => "trusted",
            Profile::DenyAll => "deny-all",
        }
    }

    /// Capabilities granted before `[permissions]` is applied.
    pub fn grants(self) -> Grants {
        let caps: &[Capability] = match self {
            Profile::Sandbox => &[Capability::Console, Capability::Clock],
            Profile::Automation => &[
                Capability::Console,
                Capability::Clock,
                Capability::FsRead,
                Capability::FsWrite,
                Capability::NetConnect,
                Capability::Process,
            ],
            Profile::GameMod => &[Capability::Console, Capability::Clock, Capability::Assets],
            Profile::Trusted => return Grants::ALL,
            Profile::DenyAll => return Grants::EMPTY,
        };
        let mut grants = Grants::EMPTY;
        for cap in caps {
            grants.insert(*cap);
        }
        grants
    }

    /// A one-line description for `xcl list`.
    pub fn summary(self) -> &'static str {
        match self {
            Profile::Sandbox => "console and clock only",
            Profile::Automation => "console, clock, files, one connection, processes",
            Profile::GameMod => "console, clock, game assets",
            Profile::Trusted => "every capability",
            Profile::DenyAll => "no capabilities",
        }
    }
}

/// One thing a plugin can ask the host for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Capability {
    /// Standard input and output.
    Console,
    /// Wall-clock and monotonic time.
    Clock,
    /// Reading files the host has opened for the run.
    FsRead,
    /// Writing files.
    FsWrite,
    /// Opening outbound connections.
    NetConnect,
    /// Accepting inbound connections.
    NetListen,
    /// Creating windows and rendering.
    Gui,
    /// Reading keyboard, mouse, and gamepad devices.
    Input,
    /// Playing and recording audio.
    Audio,
    /// Loading game assets.
    Assets,
    /// Starting other processes.
    Process,
}

impl Capability {
    /// Every capability, in declaration order (the order `xcl list` shows).
    pub const ALL: [Capability; 11] = [
        Capability::Console,
        Capability::Clock,
        Capability::FsRead,
        Capability::FsWrite,
        Capability::NetConnect,
        Capability::NetListen,
        Capability::Gui,
        Capability::Input,
        Capability::Audio,
        Capability::Assets,
        Capability::Process,
    ];

    /// The name used in `plugin.toml`, `Cargo.toml`, and error messages.
    pub fn name(self) -> &'static str {
        match self {
            Capability::Console => "console",
            Capability::Clock => "clock",
            Capability::FsRead => "fs.read",
            Capability::FsWrite => "fs.write",
            Capability::NetConnect => "net.connect",
            Capability::NetListen => "net.listen",
            Capability::Gui => "gui",
            Capability::Input => "input",
            Capability::Audio => "audio",
            Capability::Assets => "assets",
            Capability::Process => "process",
        }
    }

    /// Parses a capability name.
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "console" => Capability::Console,
            "clock" => Capability::Clock,
            "fs.read" | "fs_read" => Capability::FsRead,
            "fs.write" | "fs_write" => Capability::FsWrite,
            "net.connect" | "net_connect" => Capability::NetConnect,
            "net.listen" | "net_listen" => Capability::NetListen,
            "gui" => Capability::Gui,
            "input" => Capability::Input,
            "audio" => Capability::Audio,
            "assets" => Capability::Assets,
            "process" => Capability::Process,
            _ => return None,
        })
    }
}

/// A set of capabilities.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct Grants(u16);

impl Grants {
    /// Nothing granted.
    pub const EMPTY: Grants = Grants(0);
    /// Everything granted.
    pub const ALL: Grants = Grants((1 << Capability::ALL.len()) - 1);

    /// True when `cap` is granted.
    #[inline]
    pub fn contains(self, cap: Capability) -> bool {
        self.0 & (1 << cap as u16) != 0
    }

    /// Grants `cap`.
    pub fn insert(&mut self, cap: Capability) {
        self.0 |= 1 << cap as u16;
    }

    /// Revokes `cap`.
    pub fn remove(&mut self, cap: Capability) {
        self.0 &= !(1 << cap as u16);
    }

    /// Granted capability names, in declaration order.
    pub fn names(self) -> Vec<&'static str> {
        Capability::ALL
            .iter()
            .filter(|cap| self.contains(**cap))
            .map(|cap| cap.name())
            .collect()
    }

    /// Names in `declared` this set does not cover.
    ///
    /// An unknown name counts as missing: a capability the host does not
    /// understand cannot be granted, so refusing is the only safe reading.
    pub fn missing<'a>(self, declared: &[&'a str]) -> Vec<&'a str> {
        declared
            .iter()
            .copied()
            .filter(|name| match Capability::from_name(name) {
                Some(cap) => !self.contains(cap),
                None => true,
            })
            .collect()
    }
}

/// A parsed `plugin.toml`.
#[derive(Clone, Debug)]
pub struct Config {
    /// Plugin name, used in diagnostics.
    pub name: String,
    /// One-line description.
    pub summary: String,
    /// Absolute or directory-relative path to the entry script.
    pub entry: PathBuf,
    /// Profile named by the manifest.
    pub profile: Profile,
    /// Capabilities after `[permissions]` is applied.
    pub grants: Grants,
}

impl Config {
    /// The entry script, for messages.
    pub fn entry_display(&self) -> String {
        self.entry.display().to_string()
    }

    /// A one-line identity, e.g. ``greet (says hello)``.
    pub fn label(&self) -> String {
        format!("{} ({})", self.name, self.summary)
    }
    /// Reads `<dir>/plugin.toml`.
    ///
    /// `dir` may be the plugin directory or the manifest itself.
    pub fn load(dir: &Path) -> Result<Self, Error> {
        let manifest = if dir.is_dir() {
            dir.join(FILE_NAME)
        } else {
            dir.to_path_buf()
        };
        let plugin_dir = manifest
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));

        let metadata = fs::metadata(&manifest).map_err(|source| match source.kind() {
            io::ErrorKind::NotFound => Error::Missing {
                dir: plugin_dir.clone(),
            },
            _ => Error::Io {
                path: manifest.clone(),
                source,
            },
        })?;
        if metadata.len() > MAX_BYTES {
            return Err(Error::TooLarge {
                path: manifest,
                limit: MAX_BYTES,
            });
        }

        let text = fs::read_to_string(&manifest).map_err(|source| Error::Io {
            path: manifest.clone(),
            source,
        })?;
        let mut config = Self::parse(&text, &plugin_dir)?;
        config.entry = plugin_dir.join(&config.entry);
        Ok(config)
    }

    /// Parses manifest text. `dir` resolves the entry path and names the file
    /// in errors.
    pub fn parse(text: &str, dir: &Path) -> Result<Self, Error> {
        let mut name: Option<String> = None;
        let mut summary = String::new();
        let mut entry = String::from(DEFAULT_ENTRY);
        let mut profile = Profile::Sandbox;
        let mut abi = ABI;
        let mut overrides: Vec<(Capability, bool)> = Vec::new();
        let mut table = Table::None;

        for (index, raw) in text.lines().enumerate() {
            let line = strip_comment(raw).trim();
            if line.is_empty() {
                continue;
            }
            let at = |message: String| Error::Syntax {
                path: dir.join(FILE_NAME),
                line: index + 1,
                message,
            };

            if let Some(rest) = line.strip_prefix('[') {
                let Some(header) = rest.strip_suffix(']') else {
                    return Err(at("unterminated table header".to_string()));
                };
                table = match header.trim() {
                    "plugin" => Table::Plugin,
                    "permissions" => Table::Permissions,
                    other => {
                        return Err(at(format!(
                            "unknown table `[{other}]`; expected `[plugin]` or `[permissions]`"
                        )));
                    }
                };
                continue;
            }

            let Some((key, value)) = line.split_once('=') else {
                return Err(at("expected `key = value`".to_string()));
            };
            let key = key.trim();
            let value = value.trim();

            match table {
                Table::None => {
                    return Err(at(format!(
                        "`{key}` appears before any table; start the file with `[plugin]`"
                    )));
                }
                Table::Plugin => match key {
                    "name" => name = Some(string(value).map_err(at)?),
                    "summary" => summary = string(value).map_err(at)?,
                    "entry" => entry = string(value).map_err(at)?,
                    "profile" => {
                        let text = string(value).map_err(at)?;
                        profile = Profile::from_name(&text).ok_or_else(|| {
                            at(format!(
                                "unknown profile `{text}`; expected one of {}",
                                Profile::ALL
                                    .iter()
                                    .map(|profile| profile.name())
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ))
                        })?;
                    }
                    "abi" => {
                        let parsed = integer(value).map_err(at)?;
                        abi = u32::try_from(parsed)
                            .map_err(|_| at("`abi` must be positive".to_string()))?;
                    }
                    other => {
                        return Err(at(format!(
                            "unknown key `{other}` in `[plugin]`; expected `name`, `entry`, `abi`, `profile`, or `summary`"
                        )));
                    }
                },
                Table::Permissions => {
                    let caps = permission(key)
                        .ok_or_else(|| {
                            at(format!(
                                "unknown permission `{key}`; expected one of console, clock, fs, net, gui, input, audio, assets, process"
                            ))
                        })?;
                    match parse_permission(value).map_err(at)? {
                        Grant::All => overrides.extend(caps.iter().map(|cap| (*cap, true))),
                        Grant::None => overrides.extend(caps.iter().map(|cap| (*cap, false))),
                        Grant::Only(scopes) => {
                            for scope in scopes {
                                match caps
                                    .iter()
                                    .find(|cap| cap.name().ends_with(&format!(".{scope}")))
                                {
                                    Some(cap) => overrides.push((*cap, true)),
                                    None => {
                                        return Err(at(format!(
                                            "`{key}` has no scope `{scope}`; expected {}",
                                            caps.iter()
                                                .map(|cap| cap
                                                    .name()
                                                    .rsplit_once('.')
                                                    .map(|(_, scope)| scope)
                                                    .unwrap_or(cap.name()))
                                                .collect::<Vec<_>>()
                                                .join(" or ")
                                        )));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        let Some(name) = name else {
            return Err(Error::Syntax {
                path: dir.join(FILE_NAME),
                line: 1,
                message: "missing `name` in `[plugin]`".to_string(),
            });
        };
        if name.trim().is_empty() {
            return Err(Error::Syntax {
                path: dir.join(FILE_NAME),
                line: 1,
                message: "`name` cannot be empty".to_string(),
            });
        }
        if entry.trim().is_empty() {
            return Err(Error::Syntax {
                path: dir.join(FILE_NAME),
                line: 1,
                message: "`entry` cannot be empty".to_string(),
            });
        }
        if abi != ABI {
            return Err(Error::Abi {
                path: dir.join(FILE_NAME),
                found: abi,
            });
        }
        if !Path::new(&entry).is_absolute() && entry.contains("..") {
            return Err(Error::Syntax {
                path: dir.join(FILE_NAME),
                line: 1,
                message: format!("`entry = {entry:?}` must stay inside the plugin directory"),
            });
        }

        let mut grants = profile.grants();
        for (cap, granted) in overrides {
            if granted {
                grants.insert(cap);
            } else {
                grants.remove(cap);
            }
        }
        if summary.is_empty() {
            summary = format!("{name} plugin");
        }

        Ok(Self {
            name,
            summary,
            entry: PathBuf::from(entry),
            profile,
            grants,
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Table {
    None,
    Plugin,
    Permissions,
}

/// One `[permissions]` entry.
enum Grant {
    All,
    None,
    /// Scope names for `fs` / `net`, e.g. `["read"]`.
    Only(Vec<String>),
}

/// Capabilities a permission key controls.
fn permission(key: &str) -> Option<&'static [Capability]> {
    Some(match key {
        "console" => &[Capability::Console],
        "clock" => &[Capability::Clock],
        "fs" | "filesystem" => &[Capability::FsRead, Capability::FsWrite],
        "net" | "network" => &[Capability::NetConnect, Capability::NetListen],
        "gui" => &[Capability::Gui],
        "input" => &[Capability::Input],
        "audio" => &[Capability::Audio],
        "assets" => &[Capability::Assets],
        "process" => &[Capability::Process],
        _ => return None,
    })
}

fn parse_permission(value: &str) -> Result<Grant, String> {
    match value {
        "true" => Ok(Grant::All),
        "false" => Ok(Grant::None),
        _ if value.starts_with('[') => {
            let items = string_array(value)?;
            if items.is_empty() {
                return Ok(Grant::None);
            }
            Ok(Grant::Only(items))
        }
        _ => Err(format!(
            "expected `true`, `false`, or a list of scopes, found `{value}`"
        )),
    }
}

/// Removes a trailing `#` comment that is outside a string.
fn strip_comment(line: &str) -> &str {
    let mut in_string = false;
    let mut escaped = false;
    for (index, ch) in line.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
        } else if ch == '"' {
            in_string = true;
        } else if ch == '#' {
            return &line[..index];
        }
    }
    line
}

/// Parses a quoted string, resolving `\\`, `\"`, `\n`, `\r`, and `\t`.
fn string(value: &str) -> Result<String, String> {
    if value.len() < 2 || !value.starts_with('"') || !value.ends_with('"') {
        return Err(format!("expected a quoted string, found `{value}`"));
    }
    let inner = &value[1..value.len() - 1];
    if !inner.contains('\\') {
        return Ok(inner.to_string());
    }

    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('\\') => out.push('\\'),
            Some('"') => out.push('"'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some(other) => return Err(format!("unknown escape `\\{other}`")),
            None => return Err("a trailing `\\` ends the string".to_string()),
        }
    }
    Ok(out)
}

/// Parses `["a", "b"]`.
fn string_array(value: &str) -> Result<Vec<String>, String> {
    let Some(inner) = value
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
    else {
        return Err(format!("expected a list, found `{value}`"));
    };
    let inner = inner.trim();
    if inner.is_empty() {
        return Ok(Vec::new());
    }

    let mut items = Vec::new();
    let mut chars = inner.char_indices().peekable();
    loop {
        while matches!(chars.peek(), Some((_, ch)) if ch.is_whitespace() || *ch == ',') {
            chars.next();
        }
        let Some((start, ch)) = chars.next() else {
            break;
        };
        if ch != '"' {
            return Err(format!("expected a quoted string, found `{ch}`"));
        }
        let mut text = String::new();
        let mut escaped = false;
        let mut closed = None;
        for (index, ch) in chars.by_ref() {
            if escaped {
                text.push(ch);
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                closed = Some(index);
                break;
            } else {
                text.push(ch);
            }
        }
        if closed.is_none() {
            return Err(format!("unterminated string at offset {start}"));
        }
        items.push(text);
    }
    Ok(items)
}

/// Parses a decimal integer, allowing `_` separators.
fn integer(value: &str) -> Result<i64, String> {
    let digits = value.trim().replace('_', "");
    if digits.is_empty() || !digits.chars().all(|ch| ch.is_ascii_digit()) {
        return Err(format!("expected an integer, found `{value}`"));
    }
    digits
        .parse()
        .map_err(|_| format!("integer `{value}` is too large"))
}

/// Why a `plugin.toml` was rejected.
#[derive(Debug)]
pub enum Error {
    /// No `plugin.toml` in the directory.
    Missing {
        /// Directory that was searched.
        dir: PathBuf,
    },
    /// The file could not be read.
    Io {
        /// Offending path.
        path: PathBuf,
        /// Operating-system error.
        source: io::Error,
    },
    /// The file is larger than [`MAX_BYTES`].
    TooLarge {
        /// Offending path.
        path: PathBuf,
        /// Size limit in bytes.
        limit: u64,
    },
    /// A line could not be understood.
    Syntax {
        /// Offending path.
        path: PathBuf,
        /// 1-based line number.
        line: usize,
        /// What is wrong.
        message: String,
    },
    /// The manifest was written for a different schema version.
    Abi {
        /// Offending path.
        path: PathBuf,
        /// Version the file asked for.
        found: u32,
    },
}

impl Error {
    /// Renders the error and the fix a user can apply.
    pub fn render(&self) -> String {
        match self {
            Error::Missing { dir } => format!(
                "xcl: error: `{}` not found in `{}`\n  = help: a plugin directory needs a `plugin.toml` \
                 with `[plugin] name = ...` and an entry script",
                FILE_NAME,
                dir.display()
            ),
            Error::Io { path, source } => {
                format!("xcl: error: cannot read `{}`: {source}", path.display())
            }
            Error::TooLarge { path, limit } => format!(
                "xcl: error: `{}` is larger than {limit} bytes\n  = help: a plugin manifest is a page, \
                 not a payload",
                path.display()
            ),
            Error::Syntax {
                path,
                line,
                message,
            } => format!("xcl: error: {}:{line}: {message}", path.display()),
            Error::Abi { path, found } => format!(
                "xcl: error: `{}` uses plugin.toml ABI {found}, this runtime understands {ABI}\n  \
                 = help: update the runtime, or set `abi = {ABI}` if the file is unchanged",
                path.display()
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<Config, Error> {
        Config::parse(text, Path::new("demo"))
    }

    #[test]
    fn a_minimal_manifest_defaults_to_a_sandbox() {
        let config = parse("[plugin]\nname = \"greet\"\n").expect("valid");
        assert_eq!(config.name, "greet");
        assert_eq!(config.profile, Profile::Sandbox);
        assert_eq!(config.entry, Path::new("main.xcl"));
        assert!(config.grants.contains(Capability::Console));
        assert!(!config.grants.contains(Capability::FsRead));
        assert!(!config.grants.contains(Capability::Gui));
    }

    #[test]
    fn a_profile_is_a_baseline_and_permissions_adjust_it() {
        let config = parse(
            "[plugin]\nname = \"build\"\nprofile = \"sandbox\"\n\n\
             [permissions]\nfs = [\"read\"]\nclock = false\n",
        )
        .expect("valid");
        assert!(config.grants.contains(Capability::FsRead));
        assert!(!config.grants.contains(Capability::FsWrite));
        assert!(!config.grants.contains(Capability::Clock));
        assert!(config.grants.contains(Capability::Console));
    }

    #[test]
    fn automation_grants_one_connection_but_not_inbound() {
        let config =
            parse("[plugin]\nname = \"deploy\"\nprofile = \"automation\"\n").expect("valid");
        assert!(config.grants.contains(Capability::NetConnect));
        assert!(!config.grants.contains(Capability::NetListen));
        assert!(config.grants.contains(Capability::Process));
        assert!(!config.grants.contains(Capability::Gui));
    }

    #[test]
    fn deny_all_grants_nothing() {
        let config = parse("[plugin]\nname = \"none\"\nprofile = \"deny-all\"\n").expect("valid");
        assert_eq!(config.grants.names(), Vec::<&str>::new());
    }

    #[test]
    fn comments_and_blank_lines_are_ignored() {
        let config =
            parse("# a plugin\n\n[plugin]\nname = \"greet\" # trailing\nentry = \"ai.xcl\"\n")
                .expect("valid");
        assert_eq!(config.name, "greet");
        assert_eq!(config.entry, Path::new("ai.xcl"));
    }

    #[test]
    fn an_unknown_key_is_an_error_naming_the_line() {
        let error = parse("[plugin]\nname = \"greet\"\nentry = \"a.xcl\"\nwat = 1\n")
            .expect_err("must fail");
        let message = error.render();
        assert!(message.contains(":4:"), "{message}");
        assert!(message.contains("unknown key `wat`"), "{message}");
    }

    #[test]
    fn an_unknown_table_is_an_error() {
        let error = parse("[limits]\nsteps = 10\n").expect_err("must fail");
        assert!(error.render().contains("unknown table `[limits]`"));
    }

    #[test]
    fn a_name_is_required() {
        let error = parse("[plugin]\nentry = \"a.xcl\"\n").expect_err("must fail");
        assert!(error.render().contains("missing `name`"));
    }

    #[test]
    fn a_future_abi_is_refused() {
        let error = parse("[plugin]\nname = \"x\"\nabi = 9\n").expect_err("must fail");
        assert!(error.render().contains("ABI 9"));
    }

    #[test]
    fn an_entry_may_not_escape_the_plugin_directory() {
        let error =
            parse("[plugin]\nname = \"x\"\nentry = \"../evil.xcl\"\n").expect_err("must fail");
        assert!(error.render().contains("must stay inside"));
    }

    #[test]
    fn a_scope_that_does_not_exist_is_refused() {
        let error = parse("[plugin]\nname = \"x\"\n[permissions]\nfs = [\"append\"]\n")
            .expect_err("must fail");
        assert!(error.render().contains("no scope `append`"), "{error:?}");
    }

    #[test]
    fn missing_reports_the_directory_and_the_fix() {
        let error = Config::load(Path::new("does-not-exist-anywhere")).expect_err("must fail");
        assert!(error.render().contains(FILE_NAME));
        assert!(error.render().contains("plugin directory"));
    }

    #[test]
    fn declared_permissions_are_checked_against_the_grant_set() {
        let config = parse("[plugin]\nname = \"gui\"\n").expect("valid");
        assert_eq!(config.grants.missing(&["console"]), Vec::<&str>::new());
        assert_eq!(config.grants.missing(&["console", "gui"]), vec!["gui"]);
        // A name the host does not know cannot be granted.
        assert_eq!(config.grants.missing(&["gpu"]), vec!["gpu"]);
    }

    #[test]
    fn every_profile_round_trips_through_its_name() {
        for profile in Profile::ALL {
            assert_eq!(Profile::from_name(profile.name()), Some(profile));
        }
    }

    #[test]
    fn trusted_grants_every_capability() {
        let grants = Profile::Trusted.grants();
        for cap in Capability::ALL {
            assert!(grants.contains(cap), "{} must be granted", cap.name());
        }
    }
}
