use crate::value::Value;
use std::collections::HashMap;

pub fn split_named_args<'a>(
    args:  &'a [Value],
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
