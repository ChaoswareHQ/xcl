//! Heap object layouts behind a `Value` pointer.
//!
//! Every heap object starts with a [`HeapHeader`], so the kind, subtype and
//! variant can be read without knowing the concrete layout. Objects come from
//! the VM arena or are leaked for values that must outlive every VM.

use super::*;

/// Header prepended to every heap-allocated XCL value.
///
/// Uses a packed memory layout (`#[repr(C, packed)]`) of 8 bytes to eliminate
/// padding and ensure instant subtype/variant inspection.
#[repr(C, packed)]
pub struct HeapHeader {
    /// Object kind tag (e.g., string, array, map, func, handle).
    pub kind: u8,
    /// Internal GC/status flags.
    pub flags: u8,
    /// Type subtype identifier (e.g. [`SUBTYPE_RESULT`], [`SUBTYPE_ERROR`]).
    pub subtype: u16,
    /// Variant identifier (e.g. [`VARIANT_OK`], [`VARIANT_ERR`]).
    pub variant: u8,
}

/// Heap-allocated UTF-8 string object.
#[repr(C)]
pub struct HeapStr {
    /// Common object header.
    pub header: HeapHeader,
    /// FNV-1a hash of `data`, computed once at construction.
    ///
    /// Lookups otherwise re-hash the query key on every access.
    pub hash: u64,
    /// UTF-8 string buffer.
    ///
    /// Arena-owned (`from_str`, concatenation) or leaked (`from_str_boxed`,
    /// used for bytecode constants that must outlive every VM). With no GC
    /// there is no cheaper option: a `String` here leaked its buffer forever,
    /// which cost ~48 MB per `string_concat_10k` run.
    pub data: &'static str,
}

/// Heap-allocated array of dynamic values.
#[repr(C)]
pub struct HeapArray {
    /// Common object header.
    pub header: HeapHeader,
    /// Contiguous element buffer.
    pub data: Vec<Value>,
    /// Mutation counter, bumped whenever the elements change.
    ///
    /// The JIT caches "all elements are integers" keyed by this counter, so a
    /// short loop entered many times does not rescan the array.
    pub version: u64,
}

/// Build a `HeapStr` with a cached hash from arena-owned or leaked bytes.
#[inline(always)]
pub(super) fn new_heap_str(header: HeapHeader, data: &'static str) -> HeapStr {
    let hash = crate::runtime::vm::fast_hash(data.as_bytes());
    HeapStr { header, hash, data }
}

/// Heap-allocated function or closure value.
#[repr(C)]
pub struct HeapFunc {
    /// Common object header.
    pub header: HeapHeader,
    /// Function execution payload and environment captures.
    ///
    /// Declared functions are shared out of `Bytecode::user_functions` with only
    /// an `Arc` bump; a runtime closure owns its descriptor outright, so creating
    /// one does not allocate an `Arc`.
    pub data: FuncData,
}

/// How a [`HeapFunc`] owns its descriptor.
pub enum FuncData {
    /// Declared function, shared with the bytecode.
    Shared(Arc<FuncValue>),
    /// Runtime closure or lambda, owned by this heap object.
    Owned(FuncValue),
}

impl FuncData {
    /// Borrows the descriptor.
    #[inline(always)]
    pub fn get(&self) -> &FuncValue {
        match self {
            FuncData::Shared(f) => f,
            FuncData::Owned(f) => f,
        }
    }
}

/// Heap-allocated wrapper around an external OS resource handle.
#[repr(C)]
pub struct HeapHandle {
    /// Common object header.
    pub header: HeapHeader,
    /// Optional underlying handle resource (empty if already closed).
    pub data: Option<HandleKind>,
}

/// Kinds of operating system resources that can be wrapped by an XCL handle.
#[derive(Debug)]
pub enum HandleKind {
    /// An open file with its path recorded for diagnostics or reopening.
    #[cfg(feature = "std")]
    File {
        /// Standard library file handle.
        file: std::fs::File,
        /// Original filesystem path.
        path: String,
    },
    /// An open file descriptor (raw OS handle) for `no_std` environments.
    #[cfg(not(feature = "std"))]
    File {
        /// Raw file descriptor pointer.
        fd: RawPtr,
        /// Original filesystem path.
        path: String,
    },
    /// Standard input stream.
    Stdin,
    /// Standard output stream.
    Stdout,
    /// Standard error stream.
    Stderr,
    /// A spawned child process.
    #[cfg(feature = "std")]
    Process {
        /// Standard library child process handle.
        child: std::process::Child,
        /// Process identifier (PID).
        pid: u32,
    },
    /// A spawned child process for `no_std` environments.
    #[cfg(not(feature = "std"))]
    Process {
        /// Raw process handle.
        handle: RawPtr,
        /// Process identifier (PID).
        pid: u32,
    },
}
