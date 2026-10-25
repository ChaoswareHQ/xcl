//! The standard library: the `util` domain (`print`, `input`).
//!
//! `print` and `input` are the two things the language itself assumes are
//! present, so they ship inside the engine rather than in an optional plugin.
//! A host that does not want them can build a VM without installing the `std`
//! feature (see [`crate::new_vm`] and `register_domains`).
//!
//! `#[xcl_plugin]` on [`Util`] declares the plugin descriptor, the installable
//! `std` feature, and the registry entries in one place. The modules only attach
//! methods with `#[xcl_impl]` / `#[xcl_fn]`.
//!
//! # Permissions
//!
//! `console` is the whole capability surface: this domain reads stdin and writes
//! stdout and touches nothing else. A profile that does not grant `console`
//! cannot install `std`.

mod input;
mod print;

use xcl_macros::xcl_plugin;

/// The `std` plugin and its `util` domain.
#[xcl_plugin(
    default,
    name = "std",
    summary = "standard library — print, input",
    permissions = "console"
)]
pub struct Util;

#[cfg(test)]
mod tests {
    #[test]
    fn the_standard_library_registers() {
        let vm = crate::new_vm();
        assert!(vm.plugins().iter().any(|plugin| plugin.name == "std"));
        assert_eq!(vm.plugin_of("util"), Some("std"));
        assert_eq!(vm.plugin_of("std.util"), Some("std"));
        for name in ["print", "util.print", "util.input", "util.readline"] {
            assert!(
                vm.hot_table().slot(name).is_some(),
                "{name} should be callable"
            );
        }
    }

    #[test]
    fn std_declares_only_the_console_capability() {
        let vm = crate::new_vm();
        let std = vm
            .plugins()
            .iter()
            .find(|plugin| plugin.name == "std")
            .expect("std is registered");
        assert_eq!(std.permissions, &["console"]);
    }
}
