//! Complex number type backed by `num-complex`.

use std::collections::HashMap;
use xcl_macros::xcl_fn;
use xcl_vm::Value;

const SUBTYPE_COMPLEX: u16 = 130;

fn cm(c: num_complex::Complex64) -> Value {
    let mut m = HashMap::with_capacity(3);
    m.insert("re".into(), Value::from_f64(c.re));
    m.insert("im".into(), Value::from_f64(c.im));
    Value::from_map_boxed_typed_v(m, SUBTYPE_COMPLEX, 0)
}

fn gc(v: Value, name: &str) -> f64 {
    v.as_map().and_then(|m| m.get(name)).map(|val| val.as_float()).unwrap_or(0.0)
}

fn tc(v: Value) -> num_complex::Complex64 {
    num_complex::Complex64::new(gc(v, "re"), gc(v, "im"))
}

// ─── Constructors ───

#[xcl_fn]
fn math_complex(args: &[Value]) -> Value {
    cm(num_complex::Complex64::new(args[0].as_float(), args[1].as_float()))
}

#[xcl_fn]
fn math_complex_polar(args: &[Value]) -> Value {
    cm(num_complex::Complex64::from_polar(args[0].as_float(), args[1].as_float()))
}

// ─── Properties ───

#[xcl_fn]
fn complex_abs(args: &[Value]) -> Value {
    Value::from_f64(tc(args[0]).norm())
}

#[xcl_fn]
fn complex_phase(args: &[Value]) -> Value {
    Value::from_f64(tc(args[0]).arg())
}

// ─── Arithmetic ───

#[xcl_fn]
fn complex_add(args: &[Value]) -> Value {
    cm(tc(args[0]) + tc(args[1]))
}

#[xcl_fn]
fn complex_sub(args: &[Value]) -> Value {
    cm(tc(args[0]) - tc(args[1]))
}

#[xcl_fn]
fn complex_mul(args: &[Value]) -> Value {
    cm(tc(args[0]) * tc(args[1]))
}

#[xcl_fn]
fn complex_div(args: &[Value]) -> Value {
    cm(tc(args[0]) / tc(args[1]))
}

// ─── Trig ───

#[xcl_fn]
fn complex_sin(args: &[Value]) -> Value {
    cm(tc(args[0]).sin())
}

#[xcl_fn]
fn complex_cos(args: &[Value]) -> Value {
    cm(tc(args[0]).cos())
}

#[xcl_fn]
fn complex_tan(args: &[Value]) -> Value {
    cm(tc(args[0]).tan())
}

// ─── Utility ───

#[xcl_fn]
fn complex_conj(args: &[Value]) -> Value {
    cm(tc(args[0]).conj())
}

#[xcl_fn]
fn complex_log(args: &[Value]) -> Value {
    cm(tc(args[0]).ln())
}

#[xcl_fn]
fn complex_sqrt(args: &[Value]) -> Value {
    cm(tc(args[0]).sqrt())
}

#[xcl_fn]
fn complex_exp(args: &[Value]) -> Value {
    cm(tc(args[0]).exp())
}
