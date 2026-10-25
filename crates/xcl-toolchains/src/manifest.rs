//! Renders the compiled plugin registry as a TOML manifest.
//!
//! The manifest is a *projection* of the registry, never a hand-written file.
//! [`#[xcl_plugin]`] records the plugin, its domains, and its permissions;
//! `#[xcl_fn]`, `#[xcl_impl]`, `#[xcl_trait]`, and `#[derive(Xcl)]` record the
//! functions, impls, traits, and types. [`render`] only formats what is already
//! there, so the manifest cannot drift from the code.
//!
//! The registration macro in the crate that owns the slices emits a
//! `manifest_toml()` wrapper, so a plugin library exposes the manifest without
//! writing or maintaining one:
//!
//! ```ignore
//! let toml = xcl_toolchain::manifest::manifest_toml();
//! assert!(toml.contains("name = \"std\""));
//! ```
//!
//! Reflected framework types (for example Bevy components discovered through
//! `bevy_reflect`) are runtime data and are not part of this static projection.

// The manifest is a cold path built one line at a time; `format!` keeps the
// renderer readable, and the extra temporary is irrelevant next to the I/O that
// follows. Every other pedantic lint still applies.
#![allow(clippy::format_push_string)]

// `alloc` re-exports needed by `no_std` builds (std prelude provides them).
#[cfg(not(feature = "std"))]
use alloc::{format, string::String, string::ToString, vec::Vec};

use crate::{
    Cap, CapSet, FeatureDescriptor, ImplDescriptor, NativeEntry, NativeKind, PluginDescriptor,
    TraitDescriptor, TypeDescriptor, TypeField, TypeKind, TypeVariant,
};

/// ABI version of the manifest format.
pub const MANIFEST_ABI: u32 = 1;

/// The registries a manifest is rendered from.
///
/// Each field is the distributed slice of the same name, so a caller passes
/// `&PLUGIN_REGISTRY`, `&NATIVE_REGISTRY`, and so on. Keeping this as a struct
/// rather than a long argument list makes it obvious which registries a manifest
/// covers, and lets a tool render a subset (one crate's entries) if it wants.
pub struct Registry<'a> {
    /// Version recorded as `[manifest] engine`.
    pub engine: &'a str,
    /// `PLUGIN_REGISTRY`.
    pub plugins: &'a [PluginDescriptor],
    /// `FEATURE_REGISTRY`.
    pub features: &'a [FeatureDescriptor],
    /// `NATIVE_REGISTRY`.
    pub natives: &'a [NativeEntry],
    /// `TYPE_REGISTRY`.
    pub types: &'a [TypeDescriptor],
    /// `TRAIT_REGISTRY`.
    pub traits: &'a [TraitDescriptor],
    /// `IMPL_REGISTRY`.
    pub impls: &'a [ImplDescriptor],
}

/// Renders a registry as a TOML document.
pub fn render(registry: &Registry<'_>) -> String {
    let mut out = String::new();
    out.push_str("# Generated from the compiled XCL registry. Do not edit.\n");
    out.push_str("# Regenerate with `xcl manifest`.\n\n");

    out.push_str("[manifest]\n");
    out.push_str(&format!("abi = {}\n", MANIFEST_ABI));
    out.push_str(&format!("engine = {}\n\n", toml_string(registry.engine)));

    render_plugins(&mut out, registry.plugins);
    render_features(&mut out, registry.features);
    render_functions(&mut out, registry.natives);
    render_types(&mut out, registry.types);
    render_traits(&mut out, registry.traits);
    render_impls(&mut out, registry.impls);

    out
}

/// Renders every plugin, feature, function, type, trait, and impl linked into
/// this build as a TOML document.
///
/// The registries live in the engine, so this projection is generated from the
/// binary rather than maintained as a file.
pub fn manifest_toml() -> String {
    render(&Registry {
        engine: env!("CARGO_PKG_VERSION"),
        plugins: &xcl_rt::PLUGIN_REGISTRY,
        features: &xcl_rt::FEATURE_REGISTRY,
        natives: &xcl_rt::NATIVE_REGISTRY,
        types: &xcl_rt::TYPE_REGISTRY,
        traits: &xcl_rt::TRAIT_REGISTRY,
        impls: &xcl_rt::IMPL_REGISTRY,
    })
}

fn render_plugins(out: &mut String, plugins: &[PluginDescriptor]) {
    let mut plugins: Vec<_> = plugins.iter().collect();
    plugins.sort_by_key(|plugin| plugin.name);
    for plugin in plugins {
        out.push_str("[[plugins]]\n");
        out.push_str(&format!("name = {}\n", toml_string(plugin.name)));
        out.push_str(&format!("summary = {}\n", toml_string(plugin.summary)));
        out.push_str(&format!(
            "domains = {}\n",
            string_array(plugin.domains.iter().map(|domain| domain.path))
        ));
        out.push_str(&format!(
            "permissions = {}\n\n",
            string_array(plugin.permissions.iter().copied())
        ));
    }
}

fn render_features(out: &mut String, features: &[FeatureDescriptor]) {
    let mut features: Vec<_> = features.iter().collect();
    features.sort_by_key(|feature| feature.name);
    for feature in features {
        out.push_str("[[features]]\n");
        out.push_str(&format!("name = {}\n", toml_string(feature.name)));
        out.push_str(&format!("summary = {}\n", toml_string(feature.summary)));
        out.push_str(&format!(
            "aliases = {}\n",
            string_array(feature.aliases.iter().copied())
        ));
        out.push_str(&format!(
            "domains = {}\n",
            string_array(feature.domains.iter().copied())
        ));
        out.push_str(&format!(
            "requires = {}\n",
            string_array(feature.requires.iter().copied())
        ));
        out.push_str(&format!("default = {}\n\n", feature.default));
    }
}

fn render_functions(out: &mut String, natives: &[NativeEntry]) {
    let mut functions: Vec<_> = natives.iter().collect();
    functions.sort_by_key(|entry| (entry.domain.unwrap_or(""), entry.method));
    for function in functions {
        out.push_str("[[functions]]\n");
        out.push_str(&format!(
            "name = {}\n",
            toml_string(&qualified(function.domain, function.method))
        ));
        if let Some(domain) = function.domain {
            out.push_str(&format!("domain = {}\n", toml_string(domain)));
        }
        out.push_str(&format!("method = {}\n", toml_string(function.method)));
        out.push_str(&format!(
            "kind = {}\n",
            toml_string(native_kind(function.kind))
        ));
        out.push_str(&format!("hot = {}\n", function.hot));
        out.push_str(&format!("auto_import = {}\n", function.auto_import));
        if !function.caps.is_empty() {
            out.push_str(&format!("caps = {}\n", cap_array(function.caps)));
        }
        if let Some(args) = function.args {
            out.push_str(&format!("args = {}\n", toml_string(args)));
        }
        if let Some(ret) = function.ret {
            out.push_str(&format!("ret = {}\n", toml_string(ret)));
        }
        if let Some(summary) = function.summary {
            out.push_str(&format!("summary = {}\n", toml_string(summary)));
        }
        if let Some(impl_type) = function.impl_type {
            out.push_str(&format!("impl_type = {}\n", toml_string(impl_type)));
        }
        if let Some(method) = function.override_method {
            out.push_str(&format!("override = {}\n", toml_string(method)));
        }
        out.push('\n');
    }
}

fn render_types(out: &mut String, types: &[TypeDescriptor]) {
    let mut types: Vec<_> = types.iter().collect();
    types.sort_by_key(|entry| (entry.domain.unwrap_or(""), entry.name));
    for ty in types {
        out.push_str("[[types]]\n");
        out.push_str(&format!("name = {}\n", toml_string(ty.name)));
        if let Some(domain) = ty.domain {
            out.push_str(&format!("domain = {}\n", toml_string(domain)));
        }
        out.push_str(&format!("kind = {}\n", toml_string(type_kind(ty.kind))));
        if !ty.caps.is_empty() {
            out.push_str(&format!("caps = {}\n", cap_array(ty.caps)));
        }
        if let Some(summary) = ty.summary {
            out.push_str(&format!("summary = {}\n", toml_string(summary)));
        }
        if let Some(parent) = ty.parent_type {
            out.push_str(&format!("parent = {}\n", toml_string(parent)));
        }
        if !ty.fields.is_empty() {
            out.push_str(&format!("fields = {}\n", field_array(ty.fields)));
        }
        if !ty.variants.is_empty() {
            out.push_str(&format!("variants = {}\n", variant_array(ty.variants)));
        }
        out.push('\n');
    }
}

fn render_traits(out: &mut String, traits: &[TraitDescriptor]) {
    let mut traits: Vec<_> = traits.iter().collect();
    traits.sort_by_key(|entry| (entry.domain.unwrap_or(""), entry.name));
    for tr in traits {
        out.push_str("[[traits]]\n");
        out.push_str(&format!("name = {}\n", toml_string(tr.name)));
        if let Some(domain) = tr.domain {
            out.push_str(&format!("domain = {}\n", toml_string(domain)));
        }
        if let Some(summary) = tr.summary {
            out.push_str(&format!("summary = {}\n", toml_string(summary)));
        }
        if !tr.methods.is_empty() {
            out.push_str(&format!(
                "methods = [{}]\n",
                method_array(
                    tr.methods
                        .iter()
                        .map(|method| { (method.name, method.args, method.ret, method.summary) })
                )
            ));
        }
        out.push('\n');
    }
}

fn render_impls(out: &mut String, impls: &[ImplDescriptor]) {
    let mut impls: Vec<_> = impls.iter().collect();
    impls.sort_by_key(|entry| {
        (
            entry.domain.unwrap_or(""),
            entry.type_name,
            entry.trait_name,
        )
    });
    for implementation in impls {
        out.push_str("[[impls]]\n");
        out.push_str(&format!(
            "type = {}\n",
            toml_string(implementation.type_name)
        ));
        out.push_str(&format!(
            "trait = {}\n",
            toml_string(implementation.trait_name)
        ));
        if let Some(domain) = implementation.domain {
            out.push_str(&format!("domain = {}\n", toml_string(domain)));
        }
        if !implementation.methods.is_empty() {
            out.push_str(&format!(
                "methods = [{}]\n",
                method_array(
                    implementation
                        .methods
                        .iter()
                        .map(|method| { (method.name, method.args, method.ret, method.summary) })
                )
            ));
        }
        out.push('\n');
    }
}

type MethodParts<'a> = (&'a str, Option<&'a str>, Option<&'a str>, Option<&'a str>);

/// Formats trait/impl method tables (`{ name = ..., args = ... }`).
fn method_array<'a>(methods: impl Iterator<Item = MethodParts<'a>>) -> String {
    methods
        .map(|(name, args, ret, summary)| {
            let mut entry = format!("{{ name = {}", toml_string(name));
            if let Some(args) = args {
                entry.push_str(&format!(", args = {}", toml_string(args)));
            }
            if let Some(ret) = ret {
                entry.push_str(&format!(", ret = {}", toml_string(ret)));
            }
            if let Some(summary) = summary {
                entry.push_str(&format!(", summary = {}", toml_string(summary)));
            }
            entry.push_str(" }");
            entry
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn qualified(domain: Option<&str>, method: &str) -> String {
    match domain {
        Some(domain) => format!("{domain}.{method}"),
        None => method.to_string(),
    }
}

fn native_kind(kind: NativeKind) -> &'static str {
    match kind {
        NativeKind::Pure => "pure",
        NativeKind::Ctx => "ctx",
    }
}

fn type_kind(kind: TypeKind) -> &'static str {
    match kind {
        TypeKind::Opaque => "opaque",
        TypeKind::Record => "record",
        TypeKind::Sum => "sum",
        TypeKind::Alias => "alias",
    }
}

fn string_array<'a>(items: impl IntoIterator<Item = &'a str>) -> String {
    let values: Vec<String> = items.into_iter().map(toml_string).collect();
    format!("[{}]", values.join(", "))
}

fn cap_array(caps: CapSet) -> String {
    let values: Vec<String> = Cap::ALL
        .iter()
        .filter(|cap| caps.has(**cap))
        .map(|cap| toml_string(&format!("{cap:?}").to_lowercase()))
        .collect();
    format!("[{}]", values.join(", "))
}

fn field_array(fields: &[TypeField]) -> String {
    let values: Vec<String> = fields
        .iter()
        .map(|field| {
            format!(
                "{{ name = {}, ty = {}, optional = {} }}",
                toml_string(field.name),
                toml_string(field.ty),
                field.optional
            )
        })
        .collect();
    format!("[{}]", values.join(", "))
}

fn variant_array(variants: &[TypeVariant]) -> String {
    let values: Vec<String> = variants
        .iter()
        .map(|variant| {
            format!(
                "{{ name = {}, fields = {} }}",
                toml_string(variant.name),
                field_array(variant.fields)
            )
        })
        .collect();
    format!("[{}]", values.join(", "))
}

/// Escapes a string for a TOML basic string.
fn toml_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => out.push_str(&format!("\\u{:04X}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DomainAlias;

    static DOMAINS: [DomainAlias; 1] = [DomainAlias {
        path: "std.util",
        domain: "util",
    }];

    static PLUGINS: [PluginDescriptor; 1] = [PluginDescriptor {
        name: "std",
        summary: "standard library",
        domains: &DOMAINS,
        permissions: &["console"],
    }];

    fn empty_registry() -> Registry<'static> {
        Registry {
            engine: "0.1.0",
            plugins: &[],
            features: &[],
            natives: &[],
            types: &[],
            traits: &[],
            impls: &[],
        }
    }

    #[test]
    fn an_empty_registry_still_has_a_header() {
        let toml = render(&empty_registry());
        assert!(toml.contains("[manifest]"), "{toml}");
        assert!(toml.contains("abi = 1"), "{toml}");
        assert!(toml.contains("engine = \"0.1.0\""), "{toml}");
        assert!(!toml.contains("[[plugins]]"), "{toml}");
    }

    #[test]
    fn a_plugin_carries_its_domains_and_permissions() {
        let mut registry = empty_registry();
        registry.plugins = &PLUGINS;
        let toml = render(&registry);
        assert!(toml.contains("name = \"std\""), "{toml}");
        assert!(toml.contains("domains = [\"std.util\"]"), "{toml}");
        assert!(toml.contains("permissions = [\"console\"]"), "{toml}");
    }

    #[test]
    fn strings_are_escaped() {
        assert_eq!(toml_string("a\"b"), "\"a\\\"b\"");
        assert_eq!(toml_string("a\nb"), "\"a\\nb\"");
        assert_eq!(toml_string("a\\b"), "\"a\\\\b\"");
    }
}
