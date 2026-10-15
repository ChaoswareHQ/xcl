/// Marker struct that records the names of well-known types and constructors.
///
/// Starts with all slots `None` on a bare VM. `xcl-std` fills them in at
/// registration time so the compiler and VM can recognize `Ok`/`Err`/`Result`
/// by name without `xcl-vm` knowing about `Result` at compile time.
#[derive(Default, Clone, Copy)]
pub struct WellKnown {
    /// Name of the success constructor, typically "Ok".
    pub ok_ctor: Option<&'static str>,

    /// Name of the failure constructor, typically "Err".
    pub err_ctor: Option<&'static str>,

    /// Name of the result type, typically "Result".
    pub result_type: Option<&'static str>,

    /// Field name that holds the variant tag, typically "tag".
    pub tag_field: Option<&'static str>,

    /// Field name that holds the Ok payload, typically "value".
    pub value_field: Option<&'static str>,

    /// Field name that holds the Err message, typically "message".
    pub message_field: Option<&'static str>,

    /// Field name that holds the Err code, typically "code".
    pub code_field: Option<&'static str>,
}

impl WellKnown {
    /// True if both ok_ctor and err_ctor are set.
    #[inline]
    pub fn has_result(&self) -> bool {
        self.ok_ctor.is_some() && self.err_ctor.is_some()
    }

    /// Look at a constructor name.
    /// Returns `Some(true)` for Ok, `Some(false)` for Err, `None` otherwise.
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
