//! Argument parsing utilities for XCL functions.
//!
//! # Overview
//!
//! In XCL, functions can receive both positional arguments and named arguments.
//! At runtime, named arguments are passed as a trailing dictionary (heap map)
//! in the argument slice.
//!
//! Native function bindings use [`split_named_args`] to separate normal positional
//! arguments from named arguments by checking whether the last argument is a map
//! whose keys match any expected parameter names.
//!
//! # Examples
//!
//! ```
//! use xcl_vm::utils::args::split_named_args;
//! use xcl_vm::runtime::Value;
//! use hashbrown::HashMap;
//!
//! // Calling with positional arguments only:
//! let pos_args = [Value::from_i48(10), Value::from_i48(20)];
//! let (pos, named) = split_named_args(&pos_args, &["verbose", "limit"]);
//! assert_eq!(pos.len(), 2);
//! assert!(named.is_empty());
//! ```

use crate::runtime::value::Value;
use hashbrown::HashMap;

/// Splits a slice of function arguments into positional arguments and a map of named arguments.
///
/// If the last element in `args` is a map containing at least one key present in `known`,
/// it is extracted as the named arguments dictionary and omitted from the returned positional slice.
/// Otherwise, all arguments in `args` are returned as positional arguments, and an empty map is returned.
///
/// # Arguments
///
/// * `args` - The full slice of arguments passed to the function.
/// * `known` - A list of expected named argument keys.
///
/// # Returns
///
/// A tuple `(&[Value], HashMap<&str, Value>)` containing:
/// 1. The positional arguments slice.
/// 2. A map of recognized named arguments mapping parameter name to [`Value`].
///
/// # Examples
///
/// ```
/// use xcl_vm::utils::args::split_named_args;
/// use xcl_vm::runtime::Value;
/// use hashbrown::HashMap;
///
/// let args = [Value::from_i48(1), Value::from_i48(2)];
/// let known_params = ["timeout", "retries"];
///
/// let (positional, named) = split_named_args(&args, &known_params);
/// assert_eq!(positional.len(), 2);
/// assert!(named.is_empty());
/// ```
pub fn split_named_args<'a>(
    args: &'a [Value],
    known: &[&str],
) -> (&'a [Value], HashMap<&'a str, Value>) {
    let Some(last) = args.last() else {
        return (args, HashMap::new());
    };
    let Some(map) = last.as_map() else {
        return (args, HashMap::new());
    };
    if !map.keys().any(|k| known.contains(&k.as_str())) {
        return (args, HashMap::new());
    }
    let named = map.iter().map(|(k, v)| (k.as_str(), *v)).collect();
    (&args[..args.len() - 1], named)
}
