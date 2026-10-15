//! Error kind constants shared across all domains.
//!
//! Every error's `kind` field is one of these lowercase snake_case strings.
//! Kinds are grouped by domain family: universal (all domains), filesystem (io/fs),
//! and future groups for network, data format, database, and process.

/// Universal error kinds — used by every domain.
pub mod universal {
    pub const NOT_FOUND:         &str = "not_found";
    pub const PERMISSION_DENIED: &str = "permission_denied";
    pub const INVALID_INPUT:     &str = "invalid_input";
    pub const INVALID_DATA:      &str = "invalid_data";
    pub const TIMED_OUT:         &str = "timed_out";
    pub const INTERRUPTED:       &str = "interrupted";
    pub const WOULD_BLOCK:       &str = "would_block";
    pub const OUT_OF_MEMORY:     &str = "out_of_memory";
    pub const UNSUPPORTED:       &str = "unsupported";
    pub const IN_PROGRESS:       &str = "in_progress";
    pub const OTHER:             &str = "other";

    pub const ALL: &[&str] = &[
        NOT_FOUND, PERMISSION_DENIED, INVALID_INPUT, INVALID_DATA,
        TIMED_OUT, INTERRUPTED, WOULD_BLOCK, OUT_OF_MEMORY,
        UNSUPPORTED, IN_PROGRESS, OTHER,
    ];
}

/// Filesystem error kinds — used by io and fs domains.
pub mod filesystem {
    pub const NOT_A_DIRECTORY:       &str = "not_a_directory";
    pub const IS_A_DIRECTORY:        &str = "is_a_directory";
    pub const DIRECTORY_NOT_EMPTY:   &str = "directory_not_empty";
    pub const NOT_SEEKABLE:          &str = "not_seekable";
    pub const READ_ONLY_FILESYSTEM:  &str = "read_only_filesystem";
    pub const INVALID_FILENAME:      &str = "invalid_filename";
    pub const NAME_TOO_LONG:         &str = "name_too_long";
    pub const PATH_TOO_LONG:         &str = "path_too_long";
    pub const SYMLINK_LOOP:          &str = "symlink_loop";
    pub const QUOTA_EXCEEDED:        &str = "quota_exceeded";
    pub const FILESYSTEM_FULL:       &str = "filesystem_full";
    pub const RESOURCE_BUSY:         &str = "resource_busy";
    pub const EXECUTABLE_FILE_BUSY:  &str = "executable_file_busy";
    pub const ALREADY_EXISTS:        &str = "already_exists";
    pub const WRITE_ZERO:            &str = "write_zero";
    pub const ARGUMENT_LIST_TOO_LONG: &str = "argument_list_too_long";
    pub const TOO_MANY_OPEN_FILES:   &str = "too_many_open_files";

    pub const ALL: &[&str] = &[
        NOT_A_DIRECTORY, IS_A_DIRECTORY, DIRECTORY_NOT_EMPTY, NOT_SEEKABLE,
        READ_ONLY_FILESYSTEM, INVALID_FILENAME, NAME_TOO_LONG, PATH_TOO_LONG,
        SYMLINK_LOOP, QUOTA_EXCEEDED, FILESYSTEM_FULL, RESOURCE_BUSY,
        EXECUTABLE_FILE_BUSY, ALREADY_EXISTS, WRITE_ZERO,
        ARGUMENT_LIST_TOO_LONG, TOO_MANY_OPEN_FILES,
    ];
}

/// Placeholder groups for future domains.
pub mod network       { pub const ALL: &[&str] = &[]; }
pub mod data_format   { pub const ALL: &[&str] = &[]; }
pub mod database      { pub const ALL: &[&str] = &[]; }
pub mod process       { pub const ALL: &[&str] = &[]; }

/// Error kind group enum.
#[derive(Clone, Copy, PartialEq)]
pub enum Group {
    Universal,
    Filesystem,
}

impl Group {
    pub const ALL: &'static [Group] = &[Group::Universal, Group::Filesystem];

    /// Return all kind strings in this group.
    pub fn kinds(self) -> &'static [&'static str] {
        match self {
            Group::Universal  => universal::ALL,
            Group::Filesystem => filesystem::ALL,
        }
    }

    /// Return the group name.
    pub fn name(self) -> &'static str {
        match self {
            Group::Universal  => "universal",
            Group::Filesystem => "filesystem",
        }
    }
}

/// Check if a kind string is known in any group.
pub fn is_known_kind(kind: &str) -> bool {
    Group::ALL.iter().any(|g| g.kinds().contains(&kind))
}
