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
//!     string_values: Arc::new([]),
//!     local_names: Arc::new([]),
//!     global_names: Arc::new([]),
//!     user_functions: Vec::new(),
//!     closures: Vec::new(),
//! };
//! let output = xcl_debug::decompile::decompile(&bc);
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
    let a_str = format_operand_a(op, a, b, bc);
    let b_str = format_operand_b(op, b, bc);
    let c_str = format_operand_c(op, c, bc);

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
fn format_operand_a(op: u8, a: u32, b: u16, bc: &Bytecode) -> String {
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
        | OP_LOAD_LOCAL_PUSH_INT | OP_LOAD_LOCAL_STORE_LOCAL
        | OP_MUL_LOCAL_LOCAL | OP_DIV_LOCAL_LOCAL
        | OP_INC_LOCAL | OP_DEC_LOCAL | OP_INC_LOCAL_INT => {
            let a_name = bc.local_names.get(a as usize).map(|s| s.as_ref()).unwrap_or("?");
            format!("{} ({})", a, a_name)
        }
        OP_ADD_INT_STORE_LOCAL => {
            let a_name = bc.local_names.get(a as usize).map(|s| s.as_ref()).unwrap_or("?");
            format!("{} ({})", a, a_name)
        }
        OP_FOR_NUMBER_LOOP => {
            let var_slot = (a >> 16) as usize;
            let start_wide = (a & 0xFFFF) as usize;
            let end_wide = b as usize;
            let var_name = bc.local_names.get(var_slot).map(|s| s.as_ref()).unwrap_or("?");
            let start_val = bc.wide_operands.get(start_wide).copied().unwrap_or(0);
            let end_val = bc.wide_operands.get(end_wide).copied().unwrap_or(0);
            format!("{} ({}) {}..{}", var_slot, var_name, start_val, end_val)
        }
        OP_ADD_LOCAL_STORE => {
            let a_name = bc.local_names.get(a as usize).map(|s| s.as_ref()).unwrap_or("?");
            format!("{} ({})", a, a_name)
        }
        OP_ADD_INT_LOCAL | OP_SUB_INT_LOCAL | OP_MUL_INT_LOCAL => {
            let a_name = bc.local_names.get(a as usize).map(|s| s.as_ref()).unwrap_or("?");
            format!("{} ({})", a, a_name)
        }
        OP_WHILE_LT_INT => {
            let a_name = bc.local_names.get(a as usize).map(|s| s.as_ref()).unwrap_or("?");
            format!("{} ({}) < {}", a, a_name, b as i32 as i64)
        }
        OP_WHILE_LT_LOCAL => {
            let a_name = bc.local_names.get(a as usize).map(|s| s.as_ref()).unwrap_or("?");
            let b_name = bc.local_names.get(b as usize).map(|s| s.as_ref()).unwrap_or("?");
            format!("{} ({}) < {} ({})", a, a_name, b, b_name)
        }
        OP_SUB_LOCAL_STORE | OP_DIV_LOCAL_STORE => {
            let a_name = bc.local_names.get(a as usize).map(|s| s.as_ref()).unwrap_or("?");
            format!("{} ({})", a, a_name)
        }
        OP_ADD_LOCAL_LOCAL_STORE | OP_MUL_LOCAL_LOCAL_STORE
        | OP_SUB_LOCAL_LOCAL_STORE | OP_DIV_LOCAL_LOCAL_STORE => {
            let a_name = bc.local_names.get(a as usize).map(|s| s.as_ref()).unwrap_or("?");
            format!("{} ({})", a, a_name)
        }
        OP_MUL_LOCAL_STORE => {
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
        OP_LOAD_LOCAL_LOAD_LOCAL | OP_ADD_LOCAL_LOCAL | OP_SUB_LOCAL_LOCAL | OP_LT_LOCAL_LOCAL
        | OP_MUL_LOCAL_LOCAL | OP_DIV_LOCAL_LOCAL => {
            let b_name = bc.local_names.get(b as usize).map(|s| s.as_ref()).unwrap_or("?");
            format!("{} ({})", b, b_name)
        }
        OP_LOAD_LOCAL_PUSH_INT | OP_INC_LOCAL_INT => {
            format!("{}", b as i64)
        }
        OP_LOAD_LOCAL_STORE_LOCAL => {
            let b_name = bc.local_names.get(b as usize).map(|s| s.as_ref()).unwrap_or("?");
            format!("{} ({})", b, b_name)
        }
        OP_ADD_INT_STORE_LOCAL => {
            let b_name = bc.local_names.get(b as usize).map(|s| s.as_ref()).unwrap_or("?");
            format!("{} ({})", b, b_name)
        }
        OP_ADD_LOCAL_STORE => {
            let b_name = bc.local_names.get(b as usize).map(|s| s.as_ref()).unwrap_or("?");
            format!("{} ({})", b, b_name)
        }
        OP_ADD_INT_LOCAL | OP_SUB_INT_LOCAL | OP_MUL_INT_LOCAL => {
            format!("{}", b as i64)
        }
        OP_WHILE_LT_INT => {
            format!("{}", b as i32 as i64)
        }
        OP_SUB_LOCAL_STORE => {
            let b_name = bc.local_names.get(b as usize).map(|s| s.as_ref()).unwrap_or("?");
            format!("{} ({})", b, b_name)
        }
        OP_ADD_LOCAL_LOCAL_STORE | OP_MUL_LOCAL_LOCAL_STORE
        | OP_SUB_LOCAL_LOCAL_STORE | OP_DIV_LOCAL_LOCAL_STORE => {
            // b = src2 slot index
            let src2 = b as usize;
            let n2 = bc.local_names.get(src2).map(|s| s.as_ref()).unwrap_or("?");
            format!("{} ({})", src2, n2)
        }
        OP_MUL_LOCAL_STORE => {
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

/// Format operand c based on the opcode context.
fn format_operand_c(op: u8, c: u8, bc: &Bytecode) -> String {
    match op {
        // c = src1 slot index for 3-operand store opcodes
        OP_ADD_LOCAL_LOCAL_STORE | OP_MUL_LOCAL_LOCAL_STORE
        | OP_SUB_LOCAL_LOCAL_STORE | OP_DIV_LOCAL_LOCAL_STORE => {
            let src1 = c as usize;
            let n1 = bc.local_names.get(src1).map(|s| s.as_ref()).unwrap_or("?");
            format!("{} ({})", src1, n1)
        }
        _ => {
            if c != 0 {
                format!("{}", c)
            } else {
                String::new()
            }
        }
    }
}

/// Return the human-readable name for an opcode.
fn opcode_name(op: u8) -> &'static str {
    xcl_vm::opcode_name(op)
        .or_else(|| superinstruction_name(op))
        .or_else(|| xcl_plugins::registered_opcode_name(op))
        .unwrap_or("OP_UNKNOWN")
}

/// Fallback names for superinstructions not covered by the macro.
fn superinstruction_name(op: u8) -> Option<&'static str> {
    Some(match op {
        OP_LOAD_LOCAL_LOAD_LOCAL     => "OP_LOAD_LOCAL_LOAD_LOCAL",
        OP_ADD_LOCAL_LOCAL           => "OP_ADD_LOCAL_LOCAL",
        OP_SUB_LOCAL_LOCAL           => "OP_SUB_LOCAL_LOCAL",
        OP_LT_LOCAL_LOCAL            => "OP_LT_LOCAL_LOCAL",
        OP_LOAD_LOCAL_PUSH_INT       => "OP_LOAD_LOCAL_PUSH_INT",
        OP_LOAD_LOCAL_STORE_LOCAL    => "OP_LOAD_LOCAL_STORE_LOCAL",
        OP_CALL_METHOD               => "OP_CALL_METHOD",
        OP_INC_LOCAL                 => "OP_INC_LOCAL",
        OP_DEC_LOCAL                 => "OP_DEC_LOCAL",
        OP_MUL_LOCAL_LOCAL           => "OP_MUL_LOCAL_LOCAL",
        OP_DIV_LOCAL_LOCAL           => "OP_DIV_LOCAL_LOCAL",
        OP_ADD_INT_STORE_LOCAL       => "OP_ADD_INT_STORE_LOCAL",
        OP_INC_LOCAL_INT             => "OP_INC_LOCAL_INT",
        OP_FOR_NUMBER_LOOP           => "OP_FOR_NUMBER_LOOP",
        OP_ADD_INT_LOCAL             => "OP_ADD_INT_LOCAL",
        OP_ADD_LOCAL_STORE           => "OP_ADD_LOCAL_STORE",
        OP_SUB_INT_LOCAL             => "OP_SUB_INT_LOCAL",
        OP_MUL_INT_LOCAL             => "OP_MUL_INT_LOCAL",
        OP_WHILE_LT_INT              => "OP_WHILE_LT_INT",
        OP_WHILE_LT_LOCAL            => "OP_WHILE_LT_LOCAL",
        OP_MUL_LOCAL_STORE           => "OP_MUL_LOCAL_STORE",
        OP_DIV_LOCAL_STORE           => "OP_DIV_LOCAL_STORE",
        OP_SUB_LOCAL_STORE           => "OP_SUB_LOCAL_STORE",
        OP_ADD_LOCAL_LOCAL_STORE     => "OP_ADD_LOCAL_LOCAL_STORE",
        OP_MUL_LOCAL_LOCAL_STORE     => "OP_MUL_LOCAL_LOCAL_STORE",
        OP_SUB_LOCAL_LOCAL_STORE     => "OP_SUB_LOCAL_LOCAL_STORE",
        OP_DIV_LOCAL_LOCAL_STORE     => "OP_DIV_LOCAL_LOCAL_STORE",
        OP_GET_INDEX_LOCAL_LOCAL_STORE => "OP_GET_INDEX_LOCAL_LOCAL_STORE",
        OP_GET_FIELD_LOCAL_STORE     => "OP_GET_FIELD_LOCAL_STORE",
        OP_MOD_LOCAL_LOCAL_STORE     => "OP_MOD_LOCAL_LOCAL_STORE",
        OP_LOAD_CONST_LOCAL          => "OP_LOAD_CONST_LOCAL",
        OP_ARRAY_PUSH_LOCAL          => "OP_ARRAY_PUSH_LOCAL",
        _ => return None,
    })
}

/// Compile source to bytecode and decompile it, returning the decompiled output.
pub fn decompile_source(source: &str) -> String {
    let tokens = xcl_vm::lexer::lex(source, "<input>");
    let ast = xcl_vm::parser::parse(&tokens, source);
    let bc = compile(&ast);
    decompile(&bc)
}
