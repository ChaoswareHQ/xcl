//! Standardized error kind constants and error domain groupings.
//!
//! # Overview
//!
//! In XCL, runtime error values produced by domains (I/O, filesystem, network, etc.)
//! carry a standardized string `kind` attribute formatted in `snake_case`.
//!
//! Standardizing error kinds across domains allows user scripts to write consistent
//! pattern matching and error handling logic without having to depend on raw OS errno
//! codes or platform-specific numbers:
//!
//! ```xcl
//! match result {
//!     Err { kind: "not_found", message } => print("File missing: " + message),
//!     Err { kind, message } => print("Failed with: " + kind),
//!     Ok(data) => print("Read success!"),
//! }
//! ```
//!
//! # Error Groups
//!
//! Error kinds are organized by domain families:
//!
//! * [`universal`](crate::errors::universal) - Cross-cutting error kinds applicable to any domain (e.g., `not_found`, `timed_out`).
//! * [`filesystem`](crate::errors::filesystem) - Error kinds specific to filesystem and file I/O operations (e.g., `not_a_directory`, `quota_exceeded`).
//! * `network`, `data_format`, `database`, `process` - Reserved for future domain extensions.
//!
//! # Examples
//!
//! Checking whether an error kind string is a recognized standard error:
//!
//! ```
//! use xcl_vm::errors::{is_known_kind, universal, filesystem};
//!
//! assert!(is_known_kind(universal::NOT_FOUND));
//! assert!(is_known_kind(filesystem::IS_A_DIRECTORY));
//! assert!(!is_known_kind("unknown_custom_error"));
//! ```

/// Universal error kinds applicable to all domains.
///
/// These errors represent generic failure modes such as missing resources,
/// permission failures, memory exhaustion, or network/operation timeouts.
pub mod universal {
    /// Resource or entity could not be found.
    pub const NOT_FOUND: &str = "not_found";
    /// The caller lacked necessary permissions to perform the operation.
    pub const PERMISSION_DENIED: &str = "permission_denied";
    /// An input parameter was malformed or failed validation.
    pub const INVALID_INPUT: &str = "invalid_input";
    /// Data received or read was corrupted or unexpected.
    pub const INVALID_DATA: &str = "invalid_data";
    /// The operation timed out before completion.
    pub const TIMED_OUT: &str = "timed_out";
    /// An ongoing operation was interrupted (e.g., by a signal).
    pub const INTERRUPTED: &str = "interrupted";
    /// The operation would block on a non-blocking resource.
    pub const WOULD_BLOCK: &str = "would_block";
    /// System or process memory was exhausted during the operation.
    pub const OUT_OF_MEMORY: &str = "out_of_memory";
    /// The requested operation or feature is not supported on this platform.
    pub const UNSUPPORTED: &str = "unsupported";
    /// An asynchronous or background operation is still in progress.
    pub const IN_PROGRESS: &str = "in_progress";
    /// A miscellaneous or generic error occurred.
    pub const OTHER: &str = "other";

    /// Complete list of all universal error kind strings.
    pub const ALL: &[&str] = &[
        NOT_FOUND,
        PERMISSION_DENIED,
        INVALID_INPUT,
        INVALID_DATA,
        TIMED_OUT,
        INTERRUPTED,
        WOULD_BLOCK,
        OUT_OF_MEMORY,
        UNSUPPORTED,
        IN_PROGRESS,
        OTHER,
    ];
}

/// Filesystem error kinds used by I/O and filesystem domains.
///
/// These errors describe specific filesystem boundary conditions such as
/// directory structure mismatches, read-only media, or storage quotas.
pub mod filesystem {
    /// Expected a directory, but found a file or other object.
    pub const NOT_A_DIRECTORY: &str = "not_a_directory";
    /// Expected a file, but found a directory.
    pub const IS_A_DIRECTORY: &str = "is_a_directory";
    /// Directory cannot be removed because it still contains files or subdirectories.
    pub const DIRECTORY_NOT_EMPTY: &str = "directory_not_empty";
    /// Stream or file descriptor does not support seek operations.
    pub const NOT_SEEKABLE: &str = "not_seekable";
    /// Write operation attempted on a read-only filesystem or file.
    pub const READ_ONLY_FILESYSTEM: &str = "read_only_filesystem";
    /// The provided filename contains illegal characters or components.
    pub const INVALID_FILENAME: &str = "invalid_filename";
    /// A single filename component exceeds the maximum allowed length.
    pub const NAME_TOO_LONG: &str = "name_too_long";
    /// The full file path exceeds the maximum path length allowed by the OS.
    pub const PATH_TOO_LONG: &str = "path_too_long";
    /// Symbolic link resolution encountered a circular reference.
    pub const SYMLINK_LOOP: &str = "symlink_loop";
    /// User or group disk quota has been exceeded.
    pub const QUOTA_EXCEEDED: &str = "quota_exceeded";
    /// The storage device has no remaining free space.
    pub const FILESYSTEM_FULL: &str = "filesystem_full";
    /// The resource is locked or currently in use by another process.
    pub const RESOURCE_BUSY: &str = "resource_busy";
    /// Cannot execute an executable file that is currently open for writing.
    pub const EXECUTABLE_FILE_BUSY: &str = "executable_file_busy";
    /// Destination file or directory already exists.
    pub const ALREADY_EXISTS: &str = "already_exists";
    /// A write operation returned zero bytes written.
    pub const WRITE_ZERO: &str = "write_zero";
    /// Process spawn argument list exceeds operating system limits.
    pub const ARGUMENT_LIST_TOO_LONG: &str = "argument_list_too_long";
    /// The process or system has opened the maximum allowed number of file descriptors.
    pub const TOO_MANY_OPEN_FILES: &str = "too_many_open_files";

    /// Complete list of all filesystem error kind strings.
    pub const ALL: &[&str] = &[
        NOT_A_DIRECTORY,
        IS_A_DIRECTORY,
        DIRECTORY_NOT_EMPTY,
        NOT_SEEKABLE,
        READ_ONLY_FILESYSTEM,
        INVALID_FILENAME,
        NAME_TOO_LONG,
        PATH_TOO_LONG,
        SYMLINK_LOOP,
        QUOTA_EXCEEDED,
        FILESYSTEM_FULL,
        RESOURCE_BUSY,
        EXECUTABLE_FILE_BUSY,
        ALREADY_EXISTS,
        WRITE_ZERO,
        ARGUMENT_LIST_TOO_LONG,
        TOO_MANY_OPEN_FILES,
    ];
}

/// Placeholder error kinds for future network domain extensions.
pub mod network {
    /// Complete list of network error kinds (currently empty).
    pub const ALL: &[&str] = &[];
}

/// Placeholder error kinds for future data format/serialization domain extensions.
pub mod data_format {
    /// Complete list of data format error kinds (currently empty).
    pub const ALL: &[&str] = &[];
}

/// Placeholder error kinds for future database domain extensions.
pub mod database {
    /// Complete list of database error kinds (currently empty).
    pub const ALL: &[&str] = &[];
}

/// Placeholder error kinds for future process domain extensions.
pub mod process {
    /// Complete list of process error kinds (currently empty).
    pub const ALL: &[&str] = &[];
}

/// Error domain classification group.
///
/// Enumerates error families to allow querying error sets by domain category.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Group {
    /// Universal error group applicable across all domains.
    Universal,
    /// Filesystem error group for file and directory I/O.
    Filesystem,
}

impl Group {
    /// All recognized error domain groups.
    pub const ALL: &'static [Group] = &[Group::Universal, Group::Filesystem];

    /// Returns a slice of all error kind strings belonging to this group.
    ///
    /// # Examples
    ///
    /// ```
    /// use xcl_vm::errors::Group;
    ///
    /// let universal_kinds = Group::Universal.kinds();
    /// assert!(universal_kinds.contains(&"not_found"));
    /// ```
    pub fn kinds(self) -> &'static [&'static str] {
        match self {
            Group::Universal => universal::ALL,
            Group::Filesystem => filesystem::ALL,
        }
    }

    /// Returns the human-readable identifier for this group.
    ///
    /// # Examples
    ///
    /// ```
    /// use xcl_vm::errors::Group;
    ///
    /// assert_eq!(Group::Universal.name(), "universal");
    /// assert_eq!(Group::Filesystem.name(), "filesystem");
    /// ```
    pub fn name(self) -> &'static str {
        match self {
            Group::Universal => "universal",
            Group::Filesystem => "filesystem",
        }
    }
}

/// Checks whether an error kind string is defined in any recognized error group.
///
/// # Arguments
///
/// * `kind` - The error kind string to validate (e.g. `"not_found"`).
///
/// # Returns
///
/// `true` if `kind` matches any recognized error constant, otherwise `false`.
///
/// # Examples
///
/// ```
/// use xcl_vm::errors::is_known_kind;
///
/// assert!(is_known_kind("not_found"));
/// assert!(is_known_kind("is_a_directory"));
/// assert!(!is_known_kind("non_existent_kind"));
/// ```
pub fn is_known_kind(kind: &str) -> bool {
    Group::ALL.iter().any(|g| g.kinds().contains(&kind))
}
