//! Java backend module.
//!
//! Supports all Java LTS versions: 8, 11, 17, 21, and 25.
//! Each version uses only the keywords and features available in that release.

pub mod v8;
pub mod v11;
pub mod v17;
pub mod v21;
pub mod v25;
pub mod idioms;
pub mod types;
pub mod stdlib;
