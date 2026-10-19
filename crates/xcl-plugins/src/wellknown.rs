//! Well-known types and constructor registry.
//!
//! # Overview
//!
//! The `WellKnown` struct lives in `xcl-plugins` as plugin-owned convention
//! metadata. It records the names of well-known constructors and fields (e.g.,
//! `Ok`, `Err`, `Result`) so that plugin code can reference them consistently.
//!
//! The VM core (`xcl-vm`) does **not** depend on these names. Plugins that
//! provide result-like types populate [`WellKnown`] during registration so
//! higher-level tooling can identify constructors and variants by name.
//!
//! # Examples
//!
//! ```
//! use xcl_plugins::WellKnown;
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

/// Records the names of well-known types and constructors as plugin-owned convention metadata.
///
/// The VM core does not depend on these names. Language plugins populate
/// these fields during registration so higher-level tooling can
/// identify constructors and variants by name.
///
/// Starts with all slots set to `None` by default.
///
/// # Examples
///
/// ```
/// use xcl_plugins::WellKnown;
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
    /// use xcl_plugins::WellKnown;
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
    /// use xcl_plugins::WellKnown;
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
