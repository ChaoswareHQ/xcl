//! `.xclb` binary bytecode serialization.
//!
//! Encodes a [`Bytecode`] to a compact little-endian binary format that the
//! engine can decode without a compiler. The format carries a checksum, so a
//! truncated or corrupted file is rejected before anything executes.
//!
//! # Why values are tagged
//!
//! A [`Value`] is a NaN-boxed word; for heap values the word *is* a pointer.
//! Writing those words to disk would hand the next process a stale address, so
//! every serialized value is written as a tag plus a portable payload:
//!
//! | tag | payload | meaning |
//! |---|---|---|
//! | 0 | — | `nil` |
//! | 1 | — | `false` |
//! | 2 | — | `true` |
//! | 3 | `i64` | integer |
//! | 4 | `u64` (float bits) | float |
//! | 5 | `u32` | index into `string_table` |
//!
//! Anything else (maps, arrays, handles, closures) cannot be a compile-time
//! closure environment, and encoding it is an error rather than silent data
//! loss.
//!
//! # Layout
//!
//! ```text
//! magic (4) = "XCLB"
//! format_version (2)      = 1
//! flags (2)               = 0
//! engine_abi (2)          = ENGINE_ABI
//! reserved (2)            = 0
//! source_name             = u32 len + UTF-8 (currently empty)
//! instructions            = u32 count + count * u64
//! wide_operands           = u32 count + count * i64
//! string_table            = u32 count + (u32 len + UTF-8) each
//! local_names             = u32 count + (u32 len + UTF-8) each
//! global_names            = u32 count + (u32 len + UTF-8) each
//! function_by_name        = u32 count + count * u32
//! user_functions          = u32 count + (u32 name len + UTF-8 + function body) each
//! closures                = u32 count + function body each
//! field_chains            = u32 count + chain each
//! checksum (8)            = FNV-1a over every preceding byte
//! ```
//!
//! A function body is `name`, `params`, `closure_captures`, `closure_env`
//! (count + tagged values), `uses_argv` (u8), and an optional nested `Bytecode`
//! as `present (u8)`, `u32 len`, bytes.
//!
//! A field chain is `u32 steps`, then each step as
//! `key (tagged value)`, `rhs_key (tagged value)`, `op (u8)`, `kind (u8)`,
//! `operand (u32)`.

use core::fmt;

#[cfg(not(feature = "std"))]
use alloc::{
    boxed::Box,
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};
#[cfg(feature = "std")]
use std::{
    boxed::Box,
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};

use super::{Bytecode, FieldChain, FieldStep, Instr};
use crate::runtime::value::{FuncValue, Value};

const MAGIC: [u8; 4] = *b"XCLB";
const FORMAT_VERSION: u16 = 1;

/// ABI of the instruction set and value encoding stored in a `.xclb` file.
///
/// Bump this whenever an opcode meaning, an operand layout, or a tag changes:
/// a file written by another engine ABI is rejected instead of misread.
pub const ENGINE_ABI: u16 = 1;

/// Largest file accepted, in bytes.
const MAX_FILE_SIZE: usize = 256 * 1024 * 1024;

/// Deepest nested function body accepted (`fn a() { fn b() { ... } }`).
const MAX_DEPTH: u8 = 64;

// ── Errors ──────────────────────────────────────────────────────────────────

/// Why a `.xclb` file was rejected, or why it could not be written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BytecodeError {
    /// Fewer bytes than the fixed header.
    TooShort,
    /// Magic bytes are not `XCLB`.
    BadMagic,
    /// `format_version` is not the one this engine writes.
    BadFormatVersion(u16),
    /// The file needs a different engine ABI.
    EngineAbiMismatch {
        /// ABI recorded in the file.
        file: u16,
        /// ABI this engine implements.
        expected: u16,
    },
    /// A section ends before its declared count is satisfied.
    TruncatedSection,
    /// The file is larger than [`MAX_FILE_SIZE`].
    FileTooLarge,
    /// FNV-1a checksum does not match the contents.
    ChecksumMismatch,
    /// A string is not valid UTF-8.
    InvalidUtf8,
    /// A value tag is not one this format defines.
    UnknownValueTag(u8),
    /// Bytes remain after the last section.
    TrailingBytes,
    /// A value cannot be represented in `.xclb` (a map, array, handle, …).
    UnserializableValue,
    /// A string value is not interned in `string_table`.
    StringNotInterned,
    /// Function bodies are nested deeper than [`MAX_DEPTH`].
    TooDeep,
    /// Writing or reading the file failed.
    Io(String),
}

impl fmt::Display for BytecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooShort => write!(f, "file is too short to contain an XCLB header"),
            Self::BadMagic => write!(f, "not an XCLB file (bad magic bytes)"),
            Self::BadFormatVersion(version) => write!(
                f,
                "unsupported XCLB format version {version} (this build reads {FORMAT_VERSION})"
            ),
            Self::EngineAbiMismatch { file, expected } => write!(
                f,
                "XCLB engine ABI mismatch: file is {file}, this engine is {expected}"
            ),
            Self::TruncatedSection => write!(f, "XCLB section is truncated"),
            Self::FileTooLarge => write!(f, "XCLB file exceeds the 256 MB limit"),
            Self::ChecksumMismatch => write!(f, "XCLB checksum mismatch — the file is corrupted"),
            Self::InvalidUtf8 => write!(f, "XCLB contains invalid UTF-8"),
            Self::UnknownValueTag(tag) => write!(f, "XCLB contains an unknown value tag {tag}"),
            Self::TrailingBytes => write!(f, "XCLB has trailing bytes after the last section"),
            Self::UnserializableValue => {
                write!(
                    f,
                    "XCLB cannot store a non-primitive value in a closure environment"
                )
            }
            Self::StringNotInterned => {
                write!(f, "XCLB string value is not in the string table")
            }
            Self::TooDeep => write!(f, "XCLB function bodies are nested too deeply"),
            Self::Io(message) => write!(f, "cannot read or write XCLB file: {message}"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for BytecodeError {}

// ── FNV-1a ──────────────────────────────────────────────────────────────────

const FNV_OFFSET: u64 = 0xCBF2_9CE4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01B3;

fn fnv1a(data: &[u8]) -> u64 {
    let mut hash = FNV_OFFSET;
    for &byte in data {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

// ── Writing ─────────────────────────────────────────────────────────────────

fn put_u8(out: &mut Vec<u8>, value: u8) {
    out.push(value);
}

fn put_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_i64(out: &mut Vec<u8>, value: i64) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_str(out: &mut Vec<u8>, value: &str) {
    let len = u32::try_from(value.len()).unwrap_or(u32::MAX);
    put_u32(out, len);
    out.extend_from_slice(value.as_bytes());
}

fn put_str_array(out: &mut Vec<u8>, values: &[String]) {
    put_u32(out, u32::try_from(values.len()).unwrap_or(u32::MAX));
    for value in values {
        put_str(out, value);
    }
}

fn put_box_str_array(out: &mut Vec<u8>, values: &[Box<str>]) {
    put_u32(out, u32::try_from(values.len()).unwrap_or(u32::MAX));
    for value in values {
        put_str(out, value);
    }
}

/// Writes one value using the tagged, pointer-free encoding.
fn put_value(out: &mut Vec<u8>, value: Value, strings: &[Box<str>]) -> Result<(), BytecodeError> {
    if value.is_nil() {
        put_u8(out, 0);
    } else if value.is_bool() {
        put_u8(out, if value.as_bool() { 2 } else { 1 });
    } else if value.is_int() {
        put_u8(out, 3);
        put_i64(out, value.as_i48());
    } else if value.is_float() {
        put_u8(out, 4);
        put_u64(out, value.as_f64().to_bits());
    } else if let Some(text) = value.as_str() {
        let index = strings
            .iter()
            .position(|entry| entry.as_ref() == text)
            .ok_or(BytecodeError::StringNotInterned)?;
        put_u8(out, 5);
        put_u32(out, u32::try_from(index).unwrap_or(u32::MAX));
    } else {
        return Err(BytecodeError::UnserializableValue);
    }
    Ok(())
}

fn encode_func(
    out: &mut Vec<u8>,
    func: &FuncValue,
    strings: &[Box<str>],
    depth: u8,
) -> Result<(), BytecodeError> {
    if depth > MAX_DEPTH {
        return Err(BytecodeError::TooDeep);
    }

    put_str(out, &func.name);
    put_str_array(out, &func.params);
    put_str_array(out, &func.closure_captures);

    put_u32(
        out,
        u32::try_from(func.closure_env.len()).unwrap_or(u32::MAX),
    );
    for &value in func.closure_env.iter() {
        put_value(out, value, strings)?;
    }

    put_u8(out, u8::from(func.uses_argv));

    match func.bytecode.as_deref() {
        Some(bytecode) => {
            put_u8(out, 1);
            let encoded = encode_bytecode(bytecode, depth + 1)?;
            put_u32(
                out,
                u32::try_from(encoded.len()).map_err(|_| BytecodeError::FileTooLarge)?,
            );
            out.extend_from_slice(&encoded);
        }
        None => put_u8(out, 0),
    }
    Ok(())
}

fn encode_bytecode(bytecode: &Bytecode, depth: u8) -> Result<Vec<u8>, BytecodeError> {
    if depth > MAX_DEPTH {
        return Err(BytecodeError::TooDeep);
    }

    let mut out = Vec::new();

    out.extend_from_slice(&MAGIC);
    put_u16(&mut out, FORMAT_VERSION);
    put_u16(&mut out, 0); // flags
    put_u16(&mut out, ENGINE_ABI);
    put_u16(&mut out, 0); // reserved

    // `Bytecode` does not carry a source name; the field is reserved so a later
    // format can add one without a breaking change.
    put_str(&mut out, "");

    put_u32(
        &mut out,
        u32::try_from(bytecode.instructions.len()).unwrap_or(u32::MAX),
    );
    for instruction in bytecode.instructions.iter() {
        put_u64(&mut out, instruction.0);
    }

    put_u32(
        &mut out,
        u32::try_from(bytecode.wide_operands.len()).unwrap_or(u32::MAX),
    );
    for operand in bytecode.wide_operands.iter() {
        put_i64(&mut out, *operand);
    }

    put_box_str_array(&mut out, &bytecode.string_table);
    put_box_str_array(&mut out, &bytecode.local_names);
    put_box_str_array(&mut out, &bytecode.global_names);

    put_u32(
        &mut out,
        u32::try_from(bytecode.function_by_name.len()).unwrap_or(u32::MAX),
    );
    for slot in bytecode.function_by_name.iter() {
        put_u32(&mut out, *slot);
    }

    put_u32(
        &mut out,
        u32::try_from(bytecode.user_functions.len()).unwrap_or(u32::MAX),
    );
    for (name, func) in bytecode.user_functions.iter() {
        put_str(&mut out, name);
        encode_func(&mut out, func, &bytecode.string_table, depth)?;
    }

    put_u32(
        &mut out,
        u32::try_from(bytecode.closures.len()).unwrap_or(u32::MAX),
    );
    for func in bytecode.closures.iter() {
        encode_func(&mut out, func, &bytecode.string_table, depth)?;
    }

    put_u32(
        &mut out,
        u32::try_from(bytecode.field_chains.len()).unwrap_or(u32::MAX),
    );
    for chain in bytecode.field_chains.iter() {
        put_u32(
            &mut out,
            u32::try_from(chain.steps.len()).unwrap_or(u32::MAX),
        );
        for step in chain.steps.iter() {
            put_value(&mut out, step.key, &bytecode.string_table)?;
            put_value(&mut out, step.rhs_key, &bytecode.string_table)?;
            put_u8(&mut out, step.op);
            put_u8(&mut out, step.kind);
            put_u32(&mut out, step.operand);
        }
    }

    let checksum = fnv1a(&out);
    put_u64(&mut out, checksum);
    Ok(out)
}

// ── Reading ─────────────────────────────────────────────────────────────────

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8], BytecodeError> {
        if self.remaining() < len {
            return Err(BytecodeError::TruncatedSection);
        }
        let slice = &self.data[self.pos..self.pos + len];
        self.pos += len;
        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8, BytecodeError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, BytecodeError> {
        let bytes = self.take(2)?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    fn u32(&mut self) -> Result<u32, BytecodeError> {
        let bytes = self.take(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn u64(&mut self) -> Result<u64, BytecodeError> {
        let bytes = self.take(8)?;
        Ok(u64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]))
    }

    fn i64(&mut self) -> Result<i64, BytecodeError> {
        let bytes = self.take(8)?;
        Ok(i64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]))
    }

    /// Reads a section count and rejects one larger than the bytes left.
    ///
    /// Every encoded element occupies at least one byte, so a count above the
    /// remaining length can only be a corrupt file — refuse it before
    /// allocating a table for it.
    fn count(&mut self) -> Result<usize, BytecodeError> {
        let count = self.u32()? as usize;
        if count > self.remaining() {
            return Err(BytecodeError::TruncatedSection);
        }
        Ok(count)
    }

    fn string(&mut self) -> Result<String, BytecodeError> {
        let len = self.u32()? as usize;
        let bytes = self.take(len)?;
        core::str::from_utf8(bytes)
            .map(str::to_string)
            .map_err(|_| BytecodeError::InvalidUtf8)
    }

    fn boxed_string(&mut self) -> Result<Box<str>, BytecodeError> {
        Ok(self.string()?.into_boxed_str())
    }

    fn string_array(&mut self) -> Result<Vec<String>, BytecodeError> {
        let count = self.count()?;
        let mut values = Vec::with_capacity(count);
        for _ in 0..count {
            values.push(self.string()?);
        }
        Ok(values)
    }

    fn boxed_string_array(&mut self) -> Result<Vec<Box<str>>, BytecodeError> {
        let count = self.count()?;
        let mut values = Vec::with_capacity(count);
        for _ in 0..count {
            values.push(self.boxed_string()?);
        }
        Ok(values)
    }
}

fn decode_value(reader: &mut Reader<'_>, strings: &[Box<str>]) -> Result<Value, BytecodeError> {
    match reader.u8()? {
        0 => Ok(Value::nil()),
        1 => Ok(Value::bool(false)),
        2 => Ok(Value::bool(true)),
        3 => Ok(Value::from_i48(reader.i64()?)),
        4 => Ok(Value::from_f64(f64::from_bits(reader.u64()?))),
        5 => {
            let index = reader.u32()? as usize;
            let text = strings.get(index).ok_or(BytecodeError::TruncatedSection)?;
            Ok(Value::from_str_boxed(text.to_string()))
        }
        tag => Err(BytecodeError::UnknownValueTag(tag)),
    }
}

fn decode_func(
    reader: &mut Reader<'_>,
    strings: &[Box<str>],
    depth: u8,
) -> Result<FuncValue, BytecodeError> {
    if depth > MAX_DEPTH {
        return Err(BytecodeError::TooDeep);
    }

    let name = reader.string()?;
    let params = reader.string_array()?;
    let closure_captures = reader.string_array()?;

    let env_count = reader.count()?;
    let mut closure_env = smallvec::SmallVec::with_capacity(env_count);
    for _ in 0..env_count {
        closure_env.push(decode_value(reader, strings)?);
    }

    let uses_argv = reader.u8()? != 0;

    let bytecode = if reader.u8()? != 0 {
        let len = reader.u32()? as usize;
        let nested = reader.take(len)?;
        Some(Arc::new(decode_bytecode(nested, depth + 1)?))
    } else {
        None
    };

    Ok(FuncValue {
        name: Arc::from(name),
        bytecode,
        params: Arc::from(params.into_boxed_slice()),
        closure_captures: Arc::from(closure_captures.into_boxed_slice()),
        closure_env,
        uses_argv,
    })
}

fn decode_bytecode(data: &[u8], depth: u8) -> Result<Bytecode, BytecodeError> {
    if depth > MAX_DEPTH {
        return Err(BytecodeError::TooDeep);
    }
    if data.len() > MAX_FILE_SIZE {
        return Err(BytecodeError::FileTooLarge);
    }
    if data.len() < 12 + 4 + 8 {
        return Err(BytecodeError::TooShort);
    }

    let body_len = data.len() - 8;
    let stored = u64::from_le_bytes(
        data[body_len..]
            .try_into()
            .map_err(|_| BytecodeError::TooShort)?,
    );
    if stored != fnv1a(&data[..body_len]) {
        return Err(BytecodeError::ChecksumMismatch);
    }

    let mut reader = Reader::new(&data[..body_len]);

    if reader.take(4)? != MAGIC {
        return Err(BytecodeError::BadMagic);
    }
    let format_version = reader.u16()?;
    if format_version != FORMAT_VERSION {
        return Err(BytecodeError::BadFormatVersion(format_version));
    }
    let _flags = reader.u16()?;
    let engine_abi = reader.u16()?;
    if engine_abi != ENGINE_ABI {
        return Err(BytecodeError::EngineAbiMismatch {
            file: engine_abi,
            expected: ENGINE_ABI,
        });
    }
    let _reserved = reader.u16()?;
    let _source_name = reader.string()?;

    let instruction_count = reader.count()?;
    let mut instructions = Vec::with_capacity(instruction_count);
    for _ in 0..instruction_count {
        instructions.push(Instr(reader.u64()?));
    }

    let wide_count = reader.count()?;
    let mut wide_operands = Vec::with_capacity(wide_count);
    for _ in 0..wide_count {
        wide_operands.push(reader.i64()?);
    }

    let string_table = reader.boxed_string_array()?;
    let local_names = reader.boxed_string_array()?;
    let global_names = reader.boxed_string_array()?;

    let by_name_count = reader.count()?;
    let mut function_by_name = Vec::with_capacity(by_name_count);
    for _ in 0..by_name_count {
        function_by_name.push(reader.u32()?);
    }

    let user_function_count = reader.count()?;
    let mut user_functions = Vec::with_capacity(user_function_count);
    for _ in 0..user_function_count {
        let name = reader.boxed_string()?;
        let func = decode_func(&mut reader, &string_table, depth)?;
        user_functions.push((name, Arc::new(func)));
    }

    let closure_count = reader.count()?;
    let mut closures = Vec::with_capacity(closure_count);
    for _ in 0..closure_count {
        closures.push(decode_func(&mut reader, &string_table, depth)?);
    }

    let chain_count = reader.count()?;
    let mut field_chains = Vec::with_capacity(chain_count);
    for _ in 0..chain_count {
        let step_count = reader.count()?;
        let mut steps = Vec::with_capacity(step_count);
        for _ in 0..step_count {
            let key = decode_value(&mut reader, &string_table)?;
            let rhs_key = decode_value(&mut reader, &string_table)?;
            steps.push(FieldStep {
                key,
                rhs_key,
                op: reader.u8()?,
                kind: reader.u8()?,
                operand: reader.u32()?,
            });
        }
        field_chains.push(FieldChain {
            steps: steps.into_boxed_slice(),
        });
    }

    if reader.remaining() != 0 {
        return Err(BytecodeError::TruncatedSection);
    }

    // Runtime-only tables are rebuilt, never read from the file.
    let string_values: Arc<[Value]> = Arc::from(
        string_table
            .iter()
            .map(|text| Value::from_str_boxed(text.to_string()))
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    );

    Ok(Bytecode {
        instructions: Arc::from(instructions.into_boxed_slice()),
        wide_operands: Arc::from(wide_operands.into_boxed_slice()),
        string_table: Arc::from(string_table.into_boxed_slice()),
        string_values,
        local_names: Arc::from(local_names.into_boxed_slice()),
        global_names: Arc::from(global_names.into_boxed_slice()),
        user_functions: Arc::from(user_functions.into_boxed_slice()),
        function_by_name: Arc::from(function_by_name.into_boxed_slice()),
        closures: Arc::from(closures.into_boxed_slice()),
        field_chains: Arc::from(field_chains.into_boxed_slice()),
    })
}

// ── Public API ──────────────────────────────────────────────────────────────

impl Bytecode {
    /// Serializes this bytecode to the `.xclb` binary format.
    ///
    /// # Errors
    ///
    /// Returns [`BytecodeError::UnserializableValue`] or
    /// [`BytecodeError::StringNotInterned`] when a compile-time closure
    /// environment holds a value the format cannot represent. Compile-time
    /// environments are normally empty; a hit here means the compiler stored a
    /// heap value in `Bytecode`, which is a bug rather than a file-size problem.
    pub fn encode(&self) -> Result<Vec<u8>, BytecodeError> {
        encode_bytecode(self, 0)
    }

    /// Decodes a `.xclb` blob.
    ///
    /// # Errors
    ///
    /// Returns a [`BytecodeError`] for a bad magic, a different format or
    /// engine ABI, truncation, a bad checksum, invalid UTF-8, or an unknown
    /// value tag.
    pub fn decode(data: &[u8]) -> Result<Self, BytecodeError> {
        decode_bytecode(data, 0)
    }

    /// Writes `self` to `path` in `.xclb` form.
    ///
    /// # Errors
    ///
    /// Returns [`BytecodeError::Io`] when the file cannot be written, and the
    /// encoding errors of [`Bytecode::encode`].
    #[cfg(feature = "std")]
    pub fn to_file(&self, path: &std::path::Path) -> Result<(), BytecodeError> {
        let bytes = self.encode()?;
        std::fs::write(path, bytes).map_err(|error| BytecodeError::Io(error.to_string()))
    }

    /// Reads and decodes a `.xclb` file.
    ///
    /// # Errors
    ///
    /// Returns [`BytecodeError::Io`] when the file cannot be read, and the
    /// decoding errors of [`Bytecode::decode`].
    #[cfg(feature = "std")]
    pub fn from_file(path: &std::path::Path) -> Result<Self, BytecodeError> {
        let data = std::fs::read(path).map_err(|error| BytecodeError::Io(error.to_string()))?;
        Self::decode(&data)
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn test_bytecode() -> Bytecode {
        let string_table: Arc<[Box<str>]> =
            Arc::from(vec!["hello".to_string().into_boxed_str()].into_boxed_slice());
        let string_values: Arc<[Value]> =
            Arc::from(vec![Value::from_str_boxed("hello".to_string())].into_boxed_slice());
        let instructions: Arc<[Instr]> =
            Arc::from(vec![Instr(0), Instr(1), Instr(u64::MAX)].into_boxed_slice());
        let wide_operands: Arc<[i64]> =
            Arc::from(vec![-1_i64, 0x0012_3456_789A_BCDE].into_boxed_slice());
        let local_names: Arc<[Box<str>]> =
            Arc::from(vec!["x".to_string().into_boxed_str()].into_boxed_slice());
        let global_names: Arc<[Box<str>]> =
            Arc::from(vec!["y".to_string().into_boxed_str()].into_boxed_slice());

        let function = FuncValue {
            name: Arc::from("greet"),
            bytecode: None,
            params: Arc::from(vec!["who".to_string()].into_boxed_slice()),
            closure_captures: Arc::from(vec!["g".to_string()].into_boxed_slice()),
            closure_env: smallvec::smallvec![
                Value::nil(),
                Value::bool(true),
                Value::from_i48(-7),
                Value::from_f64(1.5),
                Value::from_str_boxed("hello".to_string()),
            ],
            uses_argv: true,
        };

        let closure = FuncValue {
            name: Arc::from("closure_0"),
            bytecode: None,
            params: Arc::from([]),
            closure_captures: Arc::from([]),
            closure_env: smallvec::smallvec![],
            uses_argv: false,
        };

        let mut closure_with_body = closure;
        closure_with_body.bytecode = Some(Arc::new(Bytecode::default()));

        Bytecode {
            instructions,
            wide_operands,
            string_table,
            string_values,
            local_names,
            global_names,
            user_functions: Arc::from(
                vec![("greet".to_string().into_boxed_str(), Arc::new(function))].into_boxed_slice(),
            ),
            function_by_name: Arc::from(vec![0_u32, 1].into_boxed_slice()),
            closures: Arc::from(vec![closure_with_body].into_boxed_slice()),
            field_chains: Arc::from(
                vec![FieldChain {
                    steps: vec![FieldStep {
                        key: Value::from_str_boxed("hello".to_string()),
                        rhs_key: Value::nil(),
                        op: 1,
                        kind: 2,
                        operand: 9,
                    }]
                    .into_boxed_slice(),
                }]
                .into_boxed_slice(),
            ),
        }
    }

    #[test]
    fn round_trips_every_table() {
        let original = test_bytecode();
        let encoded = original.encode().expect("encode");
        assert_eq!(&encoded[..4], b"XCLB");

        let decoded = Bytecode::decode(&encoded).expect("decode");

        assert_eq!(
            decoded.instructions.as_ref(),
            original.instructions.as_ref()
        );
        assert_eq!(
            decoded.wide_operands.as_ref(),
            original.wide_operands.as_ref()
        );
        assert_eq!(
            decoded.string_table.as_ref(),
            original.string_table.as_ref()
        );
        assert_eq!(decoded.local_names.as_ref(), original.local_names.as_ref());
        assert_eq!(
            decoded.global_names.as_ref(),
            original.global_names.as_ref()
        );
        assert_eq!(
            decoded.function_by_name.as_ref(),
            original.function_by_name.as_ref()
        );
        assert_eq!(decoded.string_values.len(), original.string_values.len());

        assert_eq!(decoded.user_functions.len(), 1);
        let (name, function) = &decoded.user_functions[0];
        assert_eq!(name.as_ref(), "greet");
        assert_eq!(function.name.as_ref(), "greet");
        assert_eq!(function.params.as_ref(), &["who".to_string()]);
        assert_eq!(function.closure_captures.as_ref(), &["g".to_string()]);
        assert!(function.uses_argv);
        assert_eq!(function.closure_env.len(), 5);
        assert!(function.closure_env[0].is_nil());
        assert!(function.closure_env[1].as_bool());
        assert_eq!(function.closure_env[2].as_i48(), -7);
        assert!((function.closure_env[3].as_f64() - 1.5).abs() < f64::EPSILON);
        assert_eq!(function.closure_env[4].as_str(), Some("hello"));

        assert_eq!(decoded.closures.len(), 1);
        assert_eq!(decoded.closures[0].name.as_ref(), "closure_0");
        assert!(decoded.closures[0].bytecode.is_some());

        assert_eq!(decoded.field_chains.len(), 1);
        let step = &decoded.field_chains[0].steps[0];
        assert_eq!(step.key.as_str(), Some("hello"));
        assert!(step.rhs_key.is_nil());
        assert_eq!(step.op, 1);
        assert_eq!(step.kind, 2);
        assert_eq!(step.operand, 9);
    }

    #[test]
    fn an_empty_program_round_trips() {
        let encoded = Bytecode::default().encode().expect("encode");
        let decoded = Bytecode::decode(&encoded).expect("decode");
        assert!(decoded.instructions.is_empty());
        assert!(decoded.string_table.is_empty());
        assert!(decoded.user_functions.is_empty());
        assert!(decoded.field_chains.is_empty());
    }

    #[test]
    fn a_flipped_byte_is_rejected() {
        let mut encoded = test_bytecode().encode().expect("encode");
        let middle = encoded.len() / 2;
        encoded[middle] ^= 0xFF;
        assert_eq!(
            Bytecode::decode(&encoded).unwrap_err(),
            BytecodeError::ChecksumMismatch
        );
    }

    #[test]
    fn a_truncated_file_is_rejected() {
        let encoded = test_bytecode().encode().expect("encode");
        let short = &encoded[..encoded.len() - 4];
        assert!(Bytecode::decode(short).is_err());
    }

    #[test]
    fn a_bad_magic_is_rejected() {
        let mut encoded = test_bytecode().encode().expect("encode");
        encoded[0] = b'Z';
        // Keep the checksum valid so the magic check is what fires.
        let body = encoded.len() - 8;
        let checksum = fnv1a(&encoded[..body]);
        encoded[body..].copy_from_slice(&checksum.to_le_bytes());
        assert_eq!(
            Bytecode::decode(&encoded).unwrap_err(),
            BytecodeError::BadMagic
        );
    }

    #[test]
    fn a_future_engine_abi_is_refused() {
        let mut encoded = test_bytecode().encode().expect("encode");
        encoded[8..10].copy_from_slice(&(ENGINE_ABI + 1).to_le_bytes());
        let body = encoded.len() - 8;
        let checksum = fnv1a(&encoded[..body]);
        encoded[body..].copy_from_slice(&checksum.to_le_bytes());
        assert_eq!(
            Bytecode::decode(&encoded).unwrap_err(),
            BytecodeError::EngineAbiMismatch {
                file: ENGINE_ABI + 1,
                expected: ENGINE_ABI,
            }
        );
    }

    #[test]
    fn an_uninterned_string_in_the_environment_is_an_error() {
        // A string is a heap value; the format stores its string-table index.
        // If the string is not interned, encoding must refuse rather than write
        // an address that means nothing in the next process.
        let function = FuncValue {
            name: Arc::from("f"),
            bytecode: None,
            params: Arc::from(Vec::<String>::new().into_boxed_slice()),
            closure_captures: Arc::from(Vec::<String>::new().into_boxed_slice()),
            closure_env: smallvec::smallvec![Value::from_str_boxed("not interned".to_string())],
            uses_argv: false,
        };

        let bytecode = Bytecode {
            user_functions: Arc::from(
                vec![("f".to_string().into_boxed_str(), Arc::new(function))].into_boxed_slice(),
            ),
            ..Bytecode::default()
        };

        assert_eq!(bytecode.encode(), Err(BytecodeError::StringNotInterned));
    }
}
