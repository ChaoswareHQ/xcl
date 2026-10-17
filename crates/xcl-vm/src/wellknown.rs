//! Well-known types and constructor registry.
//!
//! # Overview
//!
//! The XCL VM core crate (`xcl-vm`) is intentionally decoupled from specific
//! standard library constructs like `Result`, `Ok`, and `Err`. Instead of hardcoding
//! type definitions into the bytecode compiler and VM runtime, [`WellKnown`]
//! acts as a registry for constructor and field names.
//!
//! Higher-level crates such as `xcl-plugins` populate these slots when registering
//! language plugins. The compiler and runtime then recognize `Ok(...)`, `Err(...)`,
//! and `?` unwrapping syntax without hard-coded dependencies.
//!
//! # Examples
//!
//! ```
//! use xcl_vm::WellKnown;
//!
//! let mut wk = WellKnown::default();
//! wk.ok_ctor = Some("Ok");
//! wk.err_ctor = Some("Err");
//! wk.result_type = Some("Result");
//! wk.value_field = Some("value");
//! wk.message_field = Some("message");
//! wk.code_field = Some("code");
//!
//! assert!(wk.has_result());
//! assert_eq!(wk.result_variant("Ok"), Some(true));
//! assert_eq!(wk.result_variant("Err"), Some(false));
//! assert_eq!(wk.result_variant("Other"), None);
//! ```

/// Records the names of well-known types and constructors recognized by the compiler and VM.
///
/// Starts with all slots set to `None` on a bare VM. Language plugins populate
/// these fields during registration so the bytecode compiler and runtime can
/// identify constructors and variants by name.
///
/// # Examples
///
/// ```
/// use xcl_vm::WellKnown;
///
/// let wk = WellKnown {
///     ok_ctor: Some("Ok"),
///     err_ctor: Some("Err"),
///     result_type: Some("Result"),
///     value_field: Some("value"),
///     message_field: Some("message"),
///     code_field: Some("code"),
/// };
///
/// assert!(wk.has_result());
/// ```
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct WellKnown {
    /// Name of the success constructor, typically `"Ok"`.
    pub ok_ctor: Option<&'static str>,

    /// Name of the failure constructor, typically `"Err"`.
    pub err_ctor: Option<&'static str>,

    /// Name of the result type, typically `"Result"`.
    pub result_type: Option<&'static str>,

    /// Field name that holds the `Ok` payload, typically `"value"`.
    pub value_field: Option<&'static str>,

    /// Field name that holds the `Err` message, typically `"message"`.
    pub message_field: Option<&'static str>,

    /// Field name that holds the `Err` code, typically `"code"`.
    pub code_field: Option<&'static str>,
}

impl WellKnown {
    /// Returns `true` if both `ok_ctor` and `err_ctor` are configured.
    ///
    /// # Examples
    ///
    /// ```
    /// use xcl_vm::WellKnown;
    ///
    /// let mut wk = WellKnown::default();
    /// assert!(!wk.has_result());
    ///
    /// wk.ok_ctor = Some("Ok");
    /// wk.err_ctor = Some("Err");
    /// assert!(wk.has_result());
    /// ```
    #[inline]
    pub fn has_result(&self) -> bool {
        self.ok_ctor.is_some() && self.err_ctor.is_some()
    }

    /// Tests whether a constructor name corresponds to an `Ok` or `Err` variant.
    ///
    /// # Arguments
    ///
    /// * `name` - The constructor name to test.
    ///
    /// # Returns
    ///
    /// - `Some(true)` if `name` matches [`WellKnown::ok_ctor`].
    /// - `Some(false)` if `name` matches [`WellKnown::err_ctor`].
    /// - `None` if `name` does not match either constructor.
    ///
    /// # Examples
    ///
    /// ```
    /// use xcl_vm::WellKnown;
    ///
    /// let wk = WellKnown {
    ///     ok_ctor: Some("Ok"),
    ///     err_ctor: Some("Err"),
    ///     ..Default::default()
    /// };
    ///
    /// assert_eq!(wk.result_variant("Ok"), Some(true));
    /// assert_eq!(wk.result_variant("Err"), Some(false));
    /// assert_eq!(wk.result_variant("Some"), None);
    /// ```
    #[inline]
    pub fn result_variant(&self, name: &str) -> Option<bool> {
        if Some(name) == self.ok_ctor {
            return Some(true);
        }
        if Some(name) == self.err_ctor {
            return Some(false);
        }
        None
    }
}
