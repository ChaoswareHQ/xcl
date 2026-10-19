//! Math module — Standard mathematical functions for XCL.
//!
//! Provides common unary math operations: sqrt, sin, cos, tan, exp, log, abs,
//! floor, ceil, round, tanh, log2, log10, cbrt.
//!
//! # Usage in XCL
//!
//! ```xcl
//! # Square root
//! math.sqrt 16        # → 4.0
//!
//! # Trigonometry
//! math.sin 0.0        # → 0.0
//! math.cos 0.0        # → 1.0
//!
//! # Logarithms (returns NaN for non-positive input)
//! math.log 1.0        # → 0.0
//! math.log2 8.0       # → 3.0
//! math.log10 100.0    # → 2.0
//!
//! # Rounding
//! math.floor 3.7      # → 3.0
//! math.ceil 3.2       # → 4.0
//! math.round 3.5      # → 4.0
//!
//! # Absolute value
//! math.abs (-10)        # → 10.0
//! ```

use xcl_macros::{Xcl, xcl_impl};

/// Math operations type.
#[derive(Xcl)]
pub struct Math;

#[xcl_impl]
impl Math {
    /// Returns the square root of the value.
    pub fn sqrt(x: f64) -> f64 {
        x.sqrt()
    }

    /// Returns the sine of the value (in radians).
    pub fn sin(x: f64) -> f64 {
        x.sin()
    }

    /// Returns the cosine of the value (in radians).
    pub fn cos(x: f64) -> f64 {
        x.cos()
    }

    /// Returns the tangent of the value (in radians).
    pub fn tan(x: f64) -> f64 {
        x.tan()
    }

    /// Returns e raised to the power of the value.
    pub fn exp(x: f64) -> f64 {
        x.exp()
    }

    /// Returns the natural logarithm of the value. Returns NaN for non-positive input.
    pub fn log(x: f64) -> f64 {
        if x > 0.0 { x.ln() } else { f64::NAN }
    }

    /// Returns the absolute value of the value.
    pub fn abs(x: f64) -> f64 {
        x.abs()
    }

    /// Returns the largest integer less than or equal to the value.
    pub fn floor(x: f64) -> f64 {
        x.floor()
    }

    /// Returns the smallest integer greater than or equal to the value.
    pub fn ceil(x: f64) -> f64 {
        x.ceil()
    }

    /// Returns the nearest integer to the value.
    pub fn round(x: f64) -> f64 {
        x.round()
    }

    /// Returns the hyperbolic tangent of the value (in radians).
    pub fn tanh(x: f64) -> f64 {
        x.tanh()
    }

    /// Returns the base-2 logarithm of the value. Returns NaN for non-positive input.
    pub fn log2(x: f64) -> f64 {
        if x > 0.0 { x.log2() } else { f64::NAN }
    }

    /// Returns the base-10 logarithm of the value. Returns NaN for non-positive input.
    pub fn log10(x: f64) -> f64 {
        if x > 0.0 { x.log10() } else { f64::NAN }
    }

    /// Returns the cube root of the value.
    pub fn cbrt(x: f64) -> f64 {
        x.cbrt()
    }
}

#[cfg(test)]
mod tests {
    use crate::evaluate_new;

    #[test]
    fn test_math_sqrt_int() {
        let result = evaluate_new("math.sqrt 16");
        assert!(result.is_float());
        assert!((result.as_f64() - 4.0).abs() < 1e-9);
    }

    #[test]
    fn test_math_abs_int() {
        let result = evaluate_new("math.abs (-10)");
        assert!(result.is_float());
        assert!((result.as_f64() - 10.0).abs() < 1e-9);
    }

    #[test]
    fn test_call_in_binary_add() {
        // $x = 1 + math.sqrt 16; $x  → 5.0
        let result = evaluate_new("$x = 1 + math.sqrt 16; $x");
        assert!(result.is_float());
        assert!((result.as_f64() - 5.0).abs() < 1e-9);
    }

    #[test]
    fn test_call_minus_literal() {
        // math.sqrt 16 - 1  → 3.0
        let result = evaluate_new("math.sqrt 16 - 1");
        assert!(result.is_float());
        assert!((result.as_f64() - 3.0).abs() < 1e-9);
    }

    #[test]
    fn test_call_result_in_var() {
        // $x = math.sqrt 16; $x  → 4.0
        let result = evaluate_new("$x = math.sqrt 16; $x");
        assert!(result.is_float());
        assert!((result.as_f64() - 4.0).abs() < 1e-9);
    }
}
