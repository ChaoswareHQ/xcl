//! # XCL Bytecode Decompiler
//!
//! Decompiles XCL bytecode instructions back to readable pseudo-assembly.
//!
//! ## Overview
//!
//! The decompiler reads a `Bytecode` structure and produces human-readable output
//! showing each instruction with its operands resolved to names where possible
//! (local variable names, string literals, etc.).
//!
//! ## Example
//!
//! ```
//! use xcl_vm::{Bytecode, Instr};
//! use std::sync::Arc;
//!
//! let bc = Bytecode {
//!     instructions: Arc::from(vec![Instr::a1(4, 42), Instr::none(17), Instr::none(34)]),
//!     wide_operands: Arc::new([]),
//!     string_table: Arc::new([]),
//!     local_names: Arc::new([]),
//!     user_functions: Vec::new(),
//!     closures: Vec::new(),
//! };
//! let output = xcl_decompile::decompile(&bc);
//! assert!(output.contains("OP_PUSH_INT"));
//! ```

use xcl_vm::bytecode::*;
use xcl_vm::Bytecode;

/// Decompile bytecode to a human-readable string.
pub fn decompile(bc: &Bytecode) -> String {
    let mut out = String::new();
    decompile_bytecode(&mut out, bc, 0);
    out
}

/// Decompile bytecode with an indentation level (for nested functions).
fn decompile_bytecode(out: &mut String, bc: &Bytecode, indent: usize) {
    let pad = "  ".repeat(indent);

    // Header
    out.push_str(&format!(
        "{}{} instructions, {} wide operands, {} strings, {} locals\n",
        pad,
        bc.instructions.len(),
        bc.wide_operands.len(),
        bc.string_table.len(),
        bc.local_names.len()
    ));

    // Wide operands table
    if !bc.wide_operands.is_empty() {
        out.push_str(&format!("{}--- Wide Operands ---\n", pad));
        for (i, val) in bc.wide_operands.iter().enumerate() {
            out.push_str(&format!("{}  [{}] = {}\n", pad, i, val));
        }
    }

    // String table
    if !bc.string_table.is_empty() {
        out.push_str(&format!("{}--- String Table ---\n", pad));
        for (i, s) in bc.string_table.iter().enumerate() {
            out.push_str(&format!("{}  [{}] = \"{}\"\n", pad, i, s));
        }
    }

    // Local names
    if !bc.local_names.is_empty() {
        out.push_str(&format!("{}--- Local Variables ---\n", pad));
        for (i, name) in bc.local_names.iter().enumerate() {
            out.push_str(&format!("{}  [{}] = {}\n", pad, i, name));
        }
    }

    // Instructions
    out.push_str(&format!("{}--- Instructions ---\n", pad));
    for (ip, instr) in bc.instructions.iter().enumerate() {
        decompile_instruction(out, instr, ip, bc);
    }

    // User functions
    for (name, func) in &bc.user_functions {
        out.push_str(&format!("{}\n{}--- Function: {} ---\n", pad, pad, name));
        if let Some(ref func_bc) = func.bytecode {
            decompile_bytecode(out, func_bc, indent + 1);
        }
    }

    // Closures
    for (i, closure) in bc.closures.iter().enumerate() {
        out.push_str(&format!("{}\n{}--- Closure #{}: {} ---\n", pad, pad, i, closure.name));
        if let Some(ref closure_bc) = closure.bytecode {
            decompile_bytecode(out, closure_bc, indent + 1);
        }
    }
}

/// Decompile a single instruction.
fn decompile_instruction(out: &mut String, instr: &Instr, ip: usize, bc: &Bytecode) {
    let op = instr.op();
    let a = instr.a();
    let b = instr.b();
    let c = instr.c();

    // Format operand a based on opcode
    let a_str = format_operand_a(op, a, bc);
    let b_str = format_operand_b(op, b, bc);
    let c_str = if c != 0 { format!("{}", c) } else { String::new() };

    let op_name = opcode_name(op);
    let args = match (a_str.as_str(), b_str.as_str(), c_str.as_str()) {
        ("", "", "") => String::new(),
        (a, "", "") => format!(" a={}", a),
        (a, b, "") => format!(" a={} b={}", a, b),
        (a, b, c) => format!(" a={} b={} c={}", a, b, c),
    };

    out.push_str(&format!("{:04} {}{}\n", ip, op_name, args));
}

/// Format operand a based on the opcode context.
fn format_operand_a(op: u8, a: u32, bc: &Bytecode) -> String {
    match op {
        // String table indices
        OP_PUSH_STR | OP_GET_FIELD => {
            if let Some(s) = bc.string_table.get(a as usize) {
                format!("\"{}\"", s)
            } else {
                format!("{}", a)
            }
        }
        // Local variable indices
        OP_LOAD_LOCAL | OP_STORE_LOCAL => {
            if let Some(name) = bc.local_names.get(a as usize) {
                format!("{} ({})", a, name)
            } else {
                format!("{}", a)
            }
        }
        // Jump targets (instruction pointers)
        OP_JUMP | OP_JUMP_IF_FALSE | OP_JUMP_IF_NIL => {
            format!("{}", a)
        }
        // Integer constants
        OP_PUSH_INT => {
            format!("{}", a as i64)
        }
        // Wide operand indices
        OP_PUSH_INT_WIDE | OP_PUSH_FLOAT => {
            if let Some(val) = bc.wide_operands.get(a as usize) {
                if op == OP_PUSH_FLOAT {
                    format!("{:.1} (bits: {})", f64::from_bits(*val as u64), val)
                } else {
                    format!("{}", val)
                }
            } else {
                format!("{}", a)
            }
        }
        // Call arg count
        OP_CALL => {
            format!("{}", a)
        }
        // Array/dict size
        OP_MAKE_ARRAY | OP_MAKE_DICT => {
            format!("{}", a)
        }
        // Lambda index
        OP_LOAD_LAMBDA => {
            format!("{}", a)
        }
        // Superinstructions with two local indices
        OP_LOAD_LOCAL_LOAD_LOCAL => {
            let a_name = bc.local_names.get(a as usize).map(|s| s.as_ref()).unwrap_or("?");
            format!("{} ({})", a, a_name)
        }
        OP_ADD_LOCAL_LOCAL | OP_SUB_LOCAL_LOCAL | OP_LT_LOCAL_LOCAL
        | OP_LOAD_LOCAL_PUSH_INT | OP_LOAD_LOCAL_STORE_LOCAL => {
            let a_name = bc.local_names.get(a as usize).map(|s| s.as_ref()).unwrap_or("?");
            format!("{} ({})", a, a_name)
        }
        OP_CALL_METHOD => {
            if let Some(s) = bc.string_table.get(a as usize) {
                format!("\"{}\"", s)
            } else {
                format!("{}", a)
            }
        }
        OP_TRY_START => {
            format!("{}", a)
        }
        _ => {
            if a != 0 {
                format!("{}", a)
            } else {
                String::new()
            }
        }
    }
}

/// Format operand b based on the opcode context.
fn format_operand_b(op: u8, b: u16, bc: &Bytecode) -> String {
    match op {
        // Second local index in superinstructions
        OP_LOAD_LOCAL_LOAD_LOCAL | OP_ADD_LOCAL_LOCAL | OP_SUB_LOCAL_LOCAL | OP_LT_LOCAL_LOCAL => {
            let b_name = bc.local_names.get(b as usize).map(|s| s.as_ref()).unwrap_or("?");
            format!("{} ({})", b, b_name)
        }
        OP_LOAD_LOCAL_PUSH_INT => {
            format!("{}", b as i64)
        }
        OP_LOAD_LOCAL_STORE_LOCAL => {
            let b_name = bc.local_names.get(b as usize).map(|s| s.as_ref()).unwrap_or("?");
            format!("{} ({})", b, b_name)
        }
        OP_CALL_METHOD => {
            format!("{}", b)
        }
        _ => {
            if b != 0 {
                format!("{}", b)
            } else {
                String::new()
            }
        }
    }
}

/// Return the human-readable name for an opcode.
fn opcode_name(op: u8) -> &'static str {
    match op {
        OP_NOP                       => "OP_NOP",
        OP_PUSH_NIL                  => "OP_PUSH_NIL",
        OP_PUSH_TRUE                 => "OP_PUSH_TRUE",
        OP_PUSH_FALSE                => "OP_PUSH_FALSE",
        OP_PUSH_INT                  => "OP_PUSH_INT",
        OP_PUSH_INT_WIDE             => "OP_PUSH_INT_WIDE",
        OP_PUSH_FLOAT                => "OP_PUSH_FLOAT",
        OP_PUSH_STR                  => "OP_PUSH_STR",
        OP_LOAD_LOCAL                => "OP_LOAD_LOCAL",
        OP_STORE_LOCAL               => "OP_STORE_LOCAL",
        OP_LOAD_GLOBAL               => "OP_LOAD_GLOBAL",
        OP_STORE_GLOBAL              => "OP_STORE_GLOBAL",
        OP_ADD                       => "OP_ADD",
        OP_SUB                       => "OP_SUB",
        OP_MUL                       => "OP_MUL",
        OP_DIV                       => "OP_DIV",
        OP_MOD                       => "OP_MOD",
        OP_NEG                       => "OP_NEG",
        OP_EQ                        => "OP_EQ",
        OP_NEQ                       => "OP_NEQ",
        OP_LT                        => "OP_LT",
        OP_GT                        => "OP_GT",
        OP_LE                        => "OP_LE",
        OP_GE                        => "OP_GE",
        OP_AND                       => "OP_AND",
        OP_OR                        => "OP_OR",
        OP_NOT                       => "OP_NOT",
        OP_IN                        => "OP_IN",
        OP_JUMP                      => "OP_JUMP",
        OP_JUMP_IF_FALSE             => "OP_JUMP_IF_FALSE",
        OP_JUMP_IF_NIL               => "OP_JUMP_IF_NIL",
        OP_CALL                      => "OP_CALL",
        OP_RETURN                    => "OP_RETURN",
        OP_HALT                      => "OP_HALT",
        OP_MAKE_ARRAY                => "OP_MAKE_ARRAY",
        OP_MAKE_DICT                 => "OP_MAKE_DICT",
        OP_GET_INDEX                 => "OP_GET_INDEX",
        OP_GET_FIELD                 => "OP_GET_FIELD",
        OP_ARRAY_PUSH                => "OP_ARRAY_PUSH",
        OP_LEN                       => "OP_LEN",
        OP_FOR_ITER_START            => "OP_FOR_ITER_START",
        OP_FOR_ITER_NEXT             => "OP_FOR_ITER_NEXT",
        OP_FOR_RANGE_START           => "OP_FOR_RANGE_START",
        OP_FOR_RANGE_NEXT            => "OP_FOR_RANGE_NEXT",
        OP_LOAD_LAMBDA               => "OP_LOAD_LAMBDA",
        OP_TRY_START                 => "OP_TRY_START",
        OP_TRY_END                   => "OP_TRY_END",
        OP_THROW                     => "OP_THROW",
        OP_CATCH                     => "OP_CATCH",
        OP_COALESCE                  => "OP_COALESCE",
        OP_TYPE_TEST                 => "OP_TYPE_TEST",
        // Superinstructions
        OP_LOAD_LOCAL_LOAD_LOCAL     => "OP_LOAD_LOCAL_LOAD_LOCAL",
        OP_ADD_LOCAL_LOCAL           => "OP_ADD_LOCAL_LOCAL",
        OP_SUB_LOCAL_LOCAL           => "OP_SUB_LOCAL_LOCAL",
        OP_LT_LOCAL_LOCAL            => "OP_LT_LOCAL_LOCAL",
        OP_LOAD_LOCAL_PUSH_INT       => "OP_LOAD_LOCAL_PUSH_INT",
        OP_LOAD_LOCAL_STORE_LOCAL    => "OP_LOAD_LOCAL_STORE_LOCAL",
        OP_CALL_METHOD               => "OP_CALL_METHOD",
        _                            => "OP_UNKNOWN",
    }
}

/// Compile source to bytecode and decompile it, returning the decompiled output.
pub fn decompile_source(source: &str) -> String {
    let tokens = xcl_vm::lexer::lex(source, "<input>");
    let ast = xcl_vm::parser::parse(&tokens, source);
    let bc = compile(&ast);
    decompile(&bc)
}
