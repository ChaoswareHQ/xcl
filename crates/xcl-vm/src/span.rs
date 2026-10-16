#[cfg(not(feature = "std"))]
use alloc::string::String;

/// Lightweight source location using smaller integer types.
/// Uses u32 for line/col to reduce struct size from 40 bytes to 32 bytes on 64-bit.
/// File is stored as String for compatibility but could be &'static str for compile-time known files.
#[derive(Debug, Clone)]
#[repr(C)]
pub struct Span {
    pub file: String,
    pub line: u32,
    pub col:  u32,
}

impl Span {
    #[inline]
    pub fn new(file: String, line: u32, col: u32) -> Self {
        Self { file, line, col }
    }

    #[inline]
    pub fn empty() -> Self {
        Self {
            file: String::new(),
            line: 1,
            col:  1,
        }
    }
}
