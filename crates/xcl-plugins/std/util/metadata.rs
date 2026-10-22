//! Script metadata from the leading `# key: value` comment block.
//!
//! ```xcl
//! # name: hello
//! # version: 1.2.0
//! # author: ada
//!
//! print util.metadata            # {author: ada, name: hello, version: 1.2.0}
//! print (util.metadata_get "name")   # hello
//! ```
//!
//! Only the leading block is read: scanning stops at the first line that is
//! neither blank nor a `#` comment. This makes the cost proportional to the
//! header, not to the script.
//!
//! `xcl_plugins::evaluate` installs the header before running the script. Code
//! that calls `xcl_vm::evaluate` directly (without the plugin layer) sees an
//! empty metadata map.

use std::cell::RefCell;
use std::collections::HashMap;

use xcl_macros::{Xcl, xcl_fn, xcl_impl};
use xcl_vm::Value;

use super::Util;

#[derive(Default)]
struct Meta {
    pairs: Vec<(String, String)>,
    cached: Option<Value>,
}

thread_local! {
    static META: RefCell<Meta> = RefCell::new(Meta::default());
}

/// Replaces the thread's metadata with the header of `source`.
pub(crate) fn install(source: &str) {
    let mut pairs = Vec::new();
    for line in source.lines() {
        let line = line.trim_start();
        if line.is_empty() {
            continue;
        }
        let Some(rest) = line.strip_prefix('#') else {
            break;
        };
        let rest = rest.trim();
        if rest.is_empty() {
            continue;
        }
        if let Some((key, value)) = rest.split_once(':') {
            let key = key.trim();
            if !key.is_empty() {
                pairs.push((key.to_string(), value.trim().to_string()));
            }
        }
    }
    META.with(|meta| {
        let mut meta = meta.borrow_mut();
        meta.pairs = pairs;
        meta.cached = None;
    });
}

#[xcl_impl]
impl Util {
    /// The script header as a map of strings. Empty when the script has none.
    pub fn metadata() -> Value {
        all()
    }

    /// One header field, or `none` when the key is absent.
    pub fn metadata_get(key: String) -> Value {
        get(key.as_str())
    }
}

/// Metadata as its own domain: `metadata.get "name"`, `metadata.all`, `metadata.has "name"`.
#[derive(Xcl)]
pub struct Metadata;

#[xcl_impl]
impl Metadata {
    /// One header field, or `none` when the key is absent.
    pub fn get(key: String) -> Value {
        get(key.as_str())
    }

    /// The whole header as a map.
    pub fn all() -> Value {
        all()
    }

    /// True when the header defines `key`.
    pub fn has(key: String) -> bool {
        all()
            .as_map()
            .map(|map| map.contains_key(key.as_str()))
            .unwrap_or(false)
    }
}

/// Long-form aliases: `util.metadata.get` / `util.metadata.all`.
#[xcl_fn(name = "util.metadata.get", hot)]
pub fn util_metadata_get(key: String) -> Value {
    get(key.as_str())
}

#[xcl_fn(name = "util.metadata.all", hot)]
pub fn util_metadata_all() -> Value {
    all()
}

fn get(key: &str) -> Value {
    all()
        .as_map()
        .and_then(|map| map.get(key).copied())
        .unwrap_or(Value::nil())
}

fn all() -> Value {
    META.with(|meta| {
        let mut meta = meta.borrow_mut();
        if let Some(value) = meta.cached {
            return value;
        }
        let mut map = HashMap::with_capacity(meta.pairs.len());
        for (key, value) in &meta.pairs {
            map.insert(key.clone(), Value::from_str_boxed(value.clone()));
        }
        let value = Value::from_map_boxed(map);
        meta.cached = Some(value);
        value
    })
}

#[cfg(test)]
mod tests {
    use crate::evaluate_new;

    #[test]
    fn header_round_trip() {
        let src = "# name: demo\n# version: 1.0\n\nutil.metadata_get \"name\"";
        assert_eq!(evaluate_new(src).as_str(), Some("demo"));

        let src = "# name: demo\n# version: 1.0\nutil.metadata_get \"version\"";
        assert_eq!(evaluate_new(src).as_str(), Some("1.0"));

        let src = "# name: demo\nutil.metadata_get \"missing\"";
        assert!(evaluate_new(src).is_nil());
    }

    #[test]
    fn metadata_stops_at_code() {
        let src = "$x = 1\n# name: not-a-header\nutil.metadata_get \"name\"";
        assert!(evaluate_new(src).is_nil());
    }

    #[test]
    fn metadata_map_has_entries() {
        let src = "# name: demo\n# author: ada\nutil.metadata";
        let map = evaluate_new(src).as_map().expect("map");
        assert_eq!(map.len(), 2);
        assert_eq!(map.get("name").map(|v| v.as_str()), Some(Some("demo")));
    }

    #[test]
    fn metadata_domain_mirrors_the_util_api() {
        let src = "# name: demo\nmetadata.get \"name\"";
        assert_eq!(evaluate_new(src).as_str(), Some("demo"));

        let src = "# name: demo\nmetadata.has \"name\"";
        assert!(evaluate_new(src).as_bool());

        let src = "# name: demo\nmetadata.has \"missing\"";
        assert!(!evaluate_new(src).as_bool());

        let src = "# name: demo\n# author: ada\nmetadata.all";
        assert_eq!(evaluate_new(src).as_map().map(|map| map.len()), Some(2));
    }

    #[test]
    fn long_form_metadata_aliases() {
        let src = "# name: demo\nutil.metadata.get \"name\"";
        assert_eq!(evaluate_new(src).as_str(), Some("demo"));

        let src = "# name: demo\n# author: ada\nutil.metadata.all";
        assert_eq!(evaluate_new(src).as_map().map(|map| map.len()), Some(2));
    }

    #[test]
    fn metadata_domain_without_header_is_empty() {
        assert!(evaluate_new("metadata.get \"name\"").is_nil());
        assert!(!evaluate_new("metadata.has \"name\"").as_bool());
        assert_eq!(
            evaluate_new("metadata.all").as_map().map(|map| map.len()),
            Some(0)
        );
    }

    #[test]
    fn zero_arg_domain_call_works_in_expression_position() {
        // Regression: `f (metadata.all)` compiled the dotted name as a field
        // read on an uninitialized local and produced `none`.
        let src = "# name: demo\nidentity($v) { return $v }\nidentity (metadata.all)";
        assert_eq!(evaluate_new(src).as_map().map(|map| map.len()), Some(1));

        let src = "# name: demo\nidentity($v) { return $v }\nidentity metadata.all";
        assert_eq!(evaluate_new(src).as_map().map(|map| map.len()), Some(1));

        let src = "# name: demo\nidentity($v) { return $v }\nidentity (util.metadata.all)";
        assert_eq!(evaluate_new(src).as_map().map(|map| map.len()), Some(1));
    }
}
