//! 64-bit packed virtual machine instruction encoding.
//!
//! Every [`Instr`] is a single transparent 64-bit integer (`u64`) partitioned into four fields:
//!
//! ```text
//!  63        56 55              40 39                              8 7        0
//! +------------+------------------+---------------------------------+----------+
//! |  c (8-bit) |    b (16-bit)    |           a (32-bit)            | op (8-bit|
//! +------------+------------------+---------------------------------+----------+
//! ```
//!
//! * **`op` (bits 0..8)**: 8-bit opcode identifying the instruction.
//! * **`a` (bits 8..40)**: 32-bit primary operand (jump destination instruction pointer,
//!   string pool index, wide operand table index, or local slot index).
//! * **`b` (bits 40..56)**: 16-bit secondary operand (argument count, second local slot, or small immediate).
//! * **`c` (bits 56..64)**: 8-bit auxiliary operand or configuration flag.

/// A 64-bit packed virtual machine instruction.
///
/// Encodes an 8-bit opcode, a 32-bit primary operand `a`, a 16-bit secondary operand `b`,
/// and an 8-bit auxiliary operand `c` into a single machine word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct Instr(pub u64);

impl Instr {
    /// Extracts the 8-bit opcode (`bits 0..8`).
    #[inline(always)]
    pub const fn op(self) -> u8 {
        (self.0 & 0xFF) as u8
    }

    /// Extracts the 32-bit primary operand `a` (`bits 8..40`).
    #[inline(always)]
    pub const fn a(self) -> u32 {
        ((self.0 >> 8) & 0xFFFF_FFFF) as u32
    }

    /// Extracts the 16-bit secondary operand `b` (`bits 40..56`).
    #[inline(always)]
    pub const fn b(self) -> u16 {
        ((self.0 >> 40) & 0xFFFF) as u16
    }

    /// Extracts the 8-bit auxiliary operand `c` (`bits 56..64`).
    #[inline(always)]
    pub const fn c(self) -> u8 {
        ((self.0 >> 56) & 0xFF) as u8
    }

    /// Packs raw `op`, `a`, `b`, and `c` components into a 64-bit [`Instr`].
    #[inline(always)]
    pub const fn make(op: u8, a: u32, b: u16, c: u8) -> Self {
        Instr((op as u64) | ((a as u64) << 8) | ((b as u64) << 40) | ((c as u64) << 56))
    }

    /// Creates an instruction with no operands (`a = 0`, `b = 0`, `c = 0`).
    #[inline(always)]
    pub const fn none(op: u8) -> Self {
        Self::make(op, 0, 0, 0)
    }

    /// Creates an instruction with a single 32-bit operand `a` (`b = 0`, `c = 0`).
    #[inline(always)]
    pub const fn a1(op: u8, a: u32) -> Self {
        Self::make(op, a, 0, 0)
    }

    /// Creates an instruction with two operands `a` (32-bit) and `b` (16-bit) (`c = 0`).
    #[inline(always)]
    pub const fn a2(op: u8, a: u32, b: u16) -> Self {
        Self::make(op, a, b, 0)
    }
}
