use std::collections::HashMap;

use xcl_macros::{xcl_fn, xcl_type};
use xcl_vm::{Value, WellKnown};

#[xcl_type(
    name = "Result",
    kind = "Sum",
    summary = "A success or failure. Construct with Ok(value) or Err(message, code). Destructure with match, or by checking $r.tag.",
    variants(
        Ok { value: "any" },
        Err { message: "str", code: "i64" },
    ),
)]
pub struct ResultType;

#[xcl_fn(name = "Ok", args = "value: any", ret = "Result")]
fn ok_fn(value: Value) -> Value {
    let mut m = HashMap::with_capacity(2);
    m.insert("tag".into(), Value::from_str_boxed("Ok".into()));
    m.insert("value".into(), value);
    Value::from_map_boxed(m)
}

#[xcl_fn(name = "Err", args = "message: str; code: i64", ret = "Result")]
fn err_fn(message: &str, code: i64) -> Value {
    let mut m = HashMap::with_capacity(3);
    m.insert("tag".into(), Value::from_str_boxed("Err".into()));
    m.insert("message".into(), Value::from_str_boxed(message.into()));
    m.insert("code".into(), Value::from_i48(code));
    Value::from_map_boxed(m)
}

pub fn register_well_known(vm: &mut xcl_vm::VM) {
    vm.set_well_known(WellKnown {
        ok_ctor: Some("Ok"),
        err_ctor: Some("Err"),
        result_type: Some("Result"),
        tag_field: Some("tag"),
        value_field: Some("value"),
        message_field: Some("message"),
        code_field: Some("code"),
    });
}
