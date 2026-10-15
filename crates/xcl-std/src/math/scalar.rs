//! Scalar math functions: constants, trig, hyperbolic, log, exp, rounding, utility.
//!
//! All functions are registered under the `math.*` domain prefix.

use xcl_macros::xcl_fn;
use xcl_vm::Value;

// ─── Constants ───

/// π (pi) — 3.141592653589793
#[xcl_fn(name = "math.pi", ret = "float")]
fn math_pi() -> Value {
    Value::from_f64(std::f64::consts::PI)
}

/// e (Euler's number) — 2.718281828459045
#[xcl_fn(name = "math.e", ret = "float")]
fn math_e() -> Value {
    Value::from_f64(std::f64::consts::E)
}

/// τ (tau) — 2π = 6.283185307179586
#[xcl_fn(name = "math.tau", ret = "float")]
fn math_tau() -> Value {
    Value::from_f64(std::f64::consts::TAU)
}

// ─── Trigonometry (radians) ───

/// Sine of x (radians).
#[xcl_fn(name = "math.sin", args = "x: float", ret = "float")]
fn math_sin(args: &[Value]) -> Value {
    Value::from_f64(args[0].as_float().sin())
}

/// Cosine of x (radians).
#[xcl_fn(name = "math.cos", args = "x: float", ret = "float")]
fn math_cos(args: &[Value]) -> Value {
    Value::from_f64(args[0].as_float().cos())
}

/// Tangent of x (radians).
#[xcl_fn(name = "math.tan", args = "x: float", ret = "float")]
fn math_tan(args: &[Value]) -> Value {
    Value::from_f64(args[0].as_float().tan())
}

/// Arc sine, returns radians in [-π/2, π/2].
#[xcl_fn(name = "math.asin", args = "x: float", ret = "float")]
fn math_asin(args: &[Value]) -> Value {
    Value::from_f64(args[0].as_float().asin())
}

/// Arc cosine, returns radians in [0, π].
#[xcl_fn(name = "math.acos", args = "x: float", ret = "float")]
fn math_acos(args: &[Value]) -> Value {
    Value::from_f64(args[0].as_float().acos())
}

/// Arc tangent, returns radians in [-π/2, π/2].
#[xcl_fn(name = "math.atan", args = "x: float", ret = "float")]
fn math_atan(args: &[Value]) -> Value {
    Value::from_f64(args[0].as_float().atan())
}

/// Arc tangent of y/x, returns radians in [-π, π].
#[xcl_fn(name = "math.atan2", args = "y: float; x: float", ret = "float")]
fn math_atan2(args: &[Value]) -> Value {
    Value::from_f64(args[0].as_float().atan2(args[1].as_float()))
}

// ─── Hyperbolic ───

/// Hyperbolic sine.
#[xcl_fn(name = "math.sinh", args = "x: float", ret = "float")]
fn math_sinh(args: &[Value]) -> Value {
    Value::from_f64(args[0].as_float().sinh())
}

/// Hyperbolic cosine.
#[xcl_fn(name = "math.cosh", args = "x: float", ret = "float")]
fn math_cosh(args: &[Value]) -> Value {
    Value::from_f64(args[0].as_float().cosh())
}

/// Hyperbolic tangent.
#[xcl_fn(name = "math.tanh", args = "x: float", ret = "float")]
fn math_tanh(args: &[Value]) -> Value {
    Value::from_f64(args[0].as_float().tanh())
}

/// Inverse hyperbolic sine.
#[xcl_fn(name = "math.asinh", args = "x: float", ret = "float")]
fn math_asinh(args: &[Value]) -> Value {
    Value::from_f64(args[0].as_float().asinh())
}

/// Inverse hyperbolic cosine.
#[xcl_fn(name = "math.acosh", args = "x: float", ret = "float")]
fn math_acosh(args: &[Value]) -> Value {
    Value::from_f64(args[0].as_float().acosh())
}

/// Inverse hyperbolic tangent.
#[xcl_fn(name = "math.atanh", args = "x: float", ret = "float")]
fn math_atanh(args: &[Value]) -> Value {
    Value::from_f64(args[0].as_float().atanh())
}

// ─── Logarithms ───

/// Natural logarithm (ln).
#[xcl_fn(name = "math.log", args = "x: float", ret = "float")]
fn math_log(args: &[Value]) -> Value {
    Value::from_f64(args[0].as_float().ln())
}

/// Log base 2.
#[xcl_fn(name = "math.log2", args = "x: float", ret = "float")]
fn math_log2(args: &[Value]) -> Value {
    Value::from_f64(args[0].as_float().log2())
}

/// Log base 10.
#[xcl_fn(name = "math.log10", args = "x: float", ret = "float")]
fn math_log10(args: &[Value]) -> Value {
    Value::from_f64(args[0].as_float().log10())
}

/// Log of x in arbitrary base.
#[xcl_fn(name = "math.log_n", args = "x: float; base: float", ret = "float")]
fn math_log_n(args: &[Value]) -> Value {
    Value::from_f64(args[0].as_float().log(args[1].as_float()))
}

// ─── Exponential ───

/// e^x.
#[xcl_fn(name = "math.exp", args = "x: float", ret = "float")]
fn math_exp(args: &[Value]) -> Value {
    Value::from_f64(args[0].as_float().exp())
}

/// x^y.
#[xcl_fn(name = "math.pow", args = "x: float; y: float", ret = "float")]
fn math_pow(args: &[Value]) -> Value {
    Value::from_f64(args[0].as_float().powf(args[1].as_float()))
}

/// Square root.
#[xcl_fn(name = "math.sqrt", args = "x: float", ret = "float")]
fn math_sqrt(args: &[Value]) -> Value {
    Value::from_f64(args[0].as_float().sqrt())
}

/// Cube root.
#[xcl_fn(name = "math.cbrt", args = "x: float", ret = "float")]
fn math_cbrt(args: &[Value]) -> Value {
    Value::from_f64(args[0].as_float().cbrt())
}

// ─── Rounding ───

/// Largest integer ≤ x.
#[xcl_fn(name = "math.floor", args = "x: float", ret = "int")]
fn math_floor(args: &[Value]) -> Value {
    Value::from_i48(args[0].as_float().floor() as i64)
}

/// Smallest integer ≥ x.
#[xcl_fn(name = "math.ceil", args = "x: float", ret = "int")]
fn math_ceil(args: &[Value]) -> Value {
    Value::from_i48(args[0].as_float().ceil() as i64)
}

/// Nearest integer, ties to even.
#[xcl_fn(name = "math.round", args = "x: float", ret = "int")]
fn math_round(args: &[Value]) -> Value {
    Value::from_i48(args[0].as_float().round() as i64)
}

/// Truncate toward zero.
#[xcl_fn(name = "math.trunc", args = "x: float", ret = "int")]
fn math_trunc(args: &[Value]) -> Value {
    Value::from_i48(args[0].as_float().trunc() as i64)
}

/// Fractional part of x.
#[xcl_fn(name = "math.fract", args = "x: float", ret = "float")]
fn math_fract(args: &[Value]) -> Value {
    Value::from_f64(args[0].as_float().fract())
}

// ─── Utility ───

/// Absolute value.
#[xcl_fn(name = "math.abs", args = "x: float", ret = "float")]
fn math_abs(args: &[Value]) -> Value {
    Value::from_f64(args[0].as_float().abs())
}

/// Sign of x: -1, 0, or 1.
#[xcl_fn(name = "math.sign", args = "x: float", ret = "float")]
fn math_sign(args: &[Value]) -> Value {
    Value::from_f64(args[0].as_float().signum())
}

/// Greatest common divisor of two integers.
#[xcl_fn(name = "math.gcd", args = "a: int; b: int", ret = "int")]
fn math_gcd(args: &[Value]) -> Value {
    let mut a = args[0].as_i48().unsigned_abs();
    let mut b = args[1].as_i48().unsigned_abs();
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    Value::from_i48(a as i64)
}

/// Least common multiple of two integers.
#[xcl_fn(name = "math.lcm", args = "a: int; b: int", ret = "int")]
fn math_lcm(args: &[Value]) -> Value {
    let a = args[0].as_i48().unsigned_abs();
    let b = args[1].as_i48().unsigned_abs();
    if a == 0 || b == 0 {
        return Value::from_i48(0);
    }
    let mut x = a;
    let mut y = b;
    while y != 0 {
        let t = y;
        y = x % y;
        x = t;
    }
    Value::from_i48((a / x * b) as i64)
}

/// Minimum of two values.
#[xcl_fn(name = "math.min", args = "a: float; b: float", ret = "float")]
fn math_min(args: &[Value]) -> Value {
    Value::from_f64(args[0].as_float().min(args[1].as_float()))
}

/// Maximum of two values.
#[xcl_fn(name = "math.max", args = "a: float; b: float", ret = "float")]
fn math_max(args: &[Value]) -> Value {
    Value::from_f64(args[0].as_float().max(args[1].as_float()))
}

/// Clamp x to the range [min, max].
#[xcl_fn(name = "math.clamp", args = "x: float; min: float; max: float", ret = "float")]
fn math_clamp(args: &[Value]) -> Value {
    let x = args[0].as_float();
    let lo = args[1].as_float();
    let hi = args[2].as_float();
    Value::from_f64(x.clamp(lo, hi))
}

/// Linear interpolation: a + t * (b - a).
#[xcl_fn(name = "math.lerp", args = "a: float; b: float; t: float", ret = "float")]
fn math_lerp(args: &[Value]) -> Value {
    let a = args[0].as_float();
    let b = args[1].as_float();
    let t = args[2].as_float();
    Value::from_f64(a + t * (b - a))
}

/// Convert degrees to radians.
#[xcl_fn(name = "math.deg_to_rad", args = "degrees: float", ret = "float")]
fn math_deg_to_rad(args: &[Value]) -> Value {
    Value::from_f64(args[0].as_float().to_radians())
}

/// Convert radians to degrees.
#[xcl_fn(name = "math.rad_to_deg", args = "radians: float", ret = "float")]
fn math_rad_to_deg(args: &[Value]) -> Value {
    Value::from_f64(args[0].as_float().to_degrees())
}

// ─── Random ───

/// Random float in [0.0, 1.0).
#[xcl_fn(name = "math.random", ret = "float")]
fn math_random() -> Value {
    Value::from_f64(rand::random::<f64>())
}

/// Random integer in [min, max).
#[xcl_fn(name = "math.random_int", args = "min: int; max: int", ret = "int")]
fn math_random_int(args: &[Value]) -> Value {
    let lo = args[0].as_i48();
    let hi = args[1].as_i48();
    let range = (hi - lo) as u64;
    let r = rand::random::<u64>() % range;
    Value::from_i48(lo + r as i64)
}
