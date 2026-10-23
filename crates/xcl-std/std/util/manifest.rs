//! Generated plugin manifests.
//!
//! The manifest is a projection of the compiled registries, not a hand-written
//! file. `#[xcl_plugin]`, `#[xcl_fn]`, `#[xcl_impl]`, `#[xcl_trait]`, and
//! `#[derive(Xcl)]` already record the plugin name, domains, function
//! signatures, flags, and type shapes, so [`manifest_toml`] cannot drift from
//! the code.
//!
//! ```rust
//! let toml = xcl_std::manifest_toml();
//! assert!(toml.contains("name = \"std\""));
//! ```
//!
//! Reflected framework types (for example Bevy components discovered through
//! `bevy_reflect`) are runtime data and are not part of this static projection.

use xcl_vm::{Cap, NativeKind, TypeField, TypeKind, TypeVariant};

use crate::{
    FEATURE_REGISTRY, IMPL_REGISTRY, NATIVE_REGISTRY, PLUGIN_REGISTRY, TRAIT_REGISTRY,
    TYPE_REGISTRY,
};

/// ABI version of the manifest format.
pub const MANIFEST_ABI: u32 = 1;

/// Renders every plugin, feature, function, type, trait, and impl linked into
/// this build as a TOML document.
pub fn manifest_toml() -> String {
    let mut out = String::new();
    out.push_str("# Generated from the compiled XCL registry. Do not edit.\n");
    out.push_str("# Regenerate with `xcl manifest`.\n\n");

    out.push_str("[manifest]\n");
    out.push_str(&format!("abi = {}\n", MANIFEST_ABI));
    out.push_str(&format!(
        "engine = {}\n\n",
        toml_string(env!("CARGO_PKG_VERSION"))
    ));

    render_plugins(&mut out);
    render_features(&mut out);
    render_functions(&mut out);
    render_types(&mut out);
    render_traits(&mut out);
    render_impls(&mut out);

    out
}

fn render_plugins(out: &mut String) {
    let mut plugins: Vec<_> = PLUGIN_REGISTRY.iter().collect();
    plugins.sort_by_key(|plugin| plugin.name);
    for plugin in plugins {
        out.push_str("[[plugins]]\n");
        out.push_str(&format!("name = {}\n", toml_string(plugin.name)));
        out.push_str(&format!("summary = {}\n", toml_string(plugin.summary)));
        out.push_str(&format!(
            "domains = {}\n\n",
            string_array(plugin.domains.iter().map(|domain| domain.path))
        ));
    }
}

fn render_features(out: &mut String) {
    let mut features: Vec<_> = FEATURE_REGISTRY.iter().collect();
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

fn render_functions(out: &mut String) {
    let mut functions: Vec<_> = NATIVE_REGISTRY.iter().collect();
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

fn render_types(out: &mut String) {
    let mut types: Vec<_> = TYPE_REGISTRY.iter().collect();
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

fn render_traits(out: &mut String) {
    let mut traits: Vec<_> = TRAIT_REGISTRY.iter().collect();
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
            let methods: Vec<String> = tr
                .methods
                .iter()
                .map(|method| {
                    let mut entry = format!("{{ name = {}", toml_string(method.name));
                    if let Some(args) = method.args {
                        entry.push_str(&format!(", args = {}", toml_string(args)));
                    }
                    if let Some(ret) = method.ret {
                        entry.push_str(&format!(", ret = {}", toml_string(ret)));
                    }
                    if let Some(summary) = method.summary {
                        entry.push_str(&format!(", summary = {}", toml_string(summary)));
                    }
                    entry.push_str(" }");
                    entry
                })
                .collect();
            out.push_str(&format!("methods = [{}]\n", methods.join(", ")));
        }
        out.push('\n');
    }
}

fn render_impls(out: &mut String) {
    let mut impls: Vec<_> = IMPL_REGISTRY.iter().collect();
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
            let methods: Vec<String> = implementation
                .methods
                .iter()
                .map(|method| {
                    let mut entry = format!("{{ name = {}", toml_string(method.name));
                    if let Some(args) = method.args {
                        entry.push_str(&format!(", args = {}", toml_string(args)));
                    }
                    if let Some(ret) = method.ret {
                        entry.push_str(&format!(", ret = {}", toml_string(ret)));
                    }
                    if let Some(summary) = method.summary {
                        entry.push_str(&format!(", summary = {}", toml_string(summary)));
                    }
                    entry.push_str(" }");
                    entry
                })
                .collect();
            out.push_str(&format!("methods = [{}]\n", methods.join(", ")));
        }
        out.push('\n');
    }
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

fn cap_array(caps: xcl_vm::CapSet) -> String {
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

    #[test]
    fn the_manifest_lists_the_std_plugin_and_domain() {
        let toml = manifest_toml();
        assert!(toml.contains("[[plugins]]"), "{toml}");
        assert!(toml.contains("name = \"std\""), "{toml}");
        assert!(toml.contains("domains = [\"std.util\"]"), "{toml}");
        assert!(toml.contains("[[features]]"), "{toml}");
        assert!(toml.contains("domains = [\"util\"]"), "{toml}");
    }

    #[test]
    fn the_manifest_lists_print_and_input_with_signatures() {
        let toml = manifest_toml();
        assert!(toml.contains("name = \"util.print\""), "{toml}");
        assert!(toml.contains("name = \"util.input\""), "{toml}");
        assert!(toml.contains("hot = true"), "{toml}");
        assert!(toml.contains("auto_import = true"), "{toml}");
    }

    #[test]
    fn every_registered_native_appears_exactly_once() {
        let toml = manifest_toml();
        for native in NATIVE_REGISTRY.iter() {
            let name = qualified(native.domain, native.method);
            let needle = format!("name = \"{}\"", name.replace('"', "\\\""));
            assert_eq!(
                toml.matches(&needle).count(),
                1,
                "{name} must appear once in the manifest"
            );
        }
    }
}
