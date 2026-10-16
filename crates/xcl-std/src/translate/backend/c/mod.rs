//! C backend module for the translator.
//!
//! Converts IR to C source code with support for C99, C11, C17, and C23 standards.
//! Each version backend uses only the keywords and features available in that standard.
//!
//! # C Standard Keywords
//!
//! | Standard | Keywords Added |
//! |----------|----------------|
//! | C89      | 32 base keywords (auto, break, case, char, const, etc.) |
//! | C99      | inline, restrict, _Bool, _Complex, _Imaginary |
//! | C11      | _Alignas, _Alignof, _Atomic, _Generic, _Noreturn, _Static_assert, _Thread_local |
//! | C17      | (maintenance release, no new keywords) |
//! | C23      | alignas, alignof, bool, static_assert, thread_local, true, false, typeof, typeof_unqual, nullptr, constexpr, _BitInt, _Decimal32, _Decimal64, _Decimal128 |
//!
//! # Version-Specific Features
//!
//! - **C99**: `//` comments, `long long`, VLAs, inline functions, `_Bool`
//! - **C11**: Atomics, threads, Unicode, `_Static_assert`, `_Generic`
//! - **C17**: Bug fixes only, identical feature set to C11
//! - **C23**: `auto` type inference, `true`/`false` keywords, `nullptr`, `typeof`, `_BitInt(N)`, `constexpr`

pub mod v99;
pub mod v11;
pub mod v17;
pub mod v23;
pub mod idioms;
