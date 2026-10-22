//! XCL Bytecode Debugger — disassemble, trace, and profile bytecode for performance analysis.
//!
//! # Features
//!
//! - **Disassemble**: Show all bytecode instructions with decoded operands
//! - **Trace**: Step through execution showing stack state changes
//! - **Profile**: Measure per-opcode execution time and frequency
//! - **Stats**: Show opcode frequency distribution and hot paths
//!
//! # Usage
//!
//! ```bash
//! # Disassemble a file
//! xcl-bytecode disassemble example/benchmark.xcl
//!
//! # Trace execution (first 100 steps)
//! xcl-bytecode trace example/test_lambda.xcl --steps 100
//!
//! # Profile execution with timing
//! xcl-bytecode profile example/benchmark.xcl
//!
//! # Show opcode statistics
//! xcl-bytecode stats example/benchmark.xcl
//! ```

use std::fs;
use xcl_vm::bytecode::{Bytecode, compile};
use xcl_vm::lexer::lex;
use xcl_vm::parser::parse;

/// XCL Bytecode Debugger — diagnose why XCL is slower than Luau
struct Cli {
    command: Command,
}

enum Command {
    Disassemble {
        file: String,
        raw: bool,
    },
    Trace {
        file: String,
        steps: usize,
        stack_only: bool,
    },
    Profile {
        file: String,
        runs: usize,
    },
    Stats {
        file: String,
    },
    Help,
    Version,
}

const USAGE: &str = "
Usage: xcl-bytecode <COMMAND> [OPTIONS]

Commands:
  disassemble <file> [--raw]      Disassemble bytecode instructions
  trace <file> [-s N] [--stack-only]
                                  Trace execution step by step (steps 0 = unlimited)
  profile <file> [-r N]           Profile execution with per-opcode timing
  stats <file>                    Show opcode frequency statistics

Options:
  -h, --help       Show this help message
  -v, --version    Show version information
";

fn fail(message: &str) -> ! {
    eprintln!("xcl-bytecode: error: {message}");
    eprintln!("Try 'xcl-bytecode --help' for usage.");
    std::process::exit(1)
}

fn parse_args<I: IntoIterator<Item = String>>(iter: I) -> Cli {
    let mut args = iter.into_iter().skip(1);
    let Some(command) = args.next() else {
        fail("missing command");
    };

    if command == "-h" || command == "--help" {
        return Cli {
            command: Command::Help,
        };
    }
    if command == "-v" || command == "--version" {
        return Cli {
            command: Command::Version,
        };
    }

    let mut file: Option<String> = None;
    let mut raw = false;
    let mut stack_only = false;
    let mut steps = 0usize;
    let mut runs = 3usize;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--raw" => raw = true,
            "--stack-only" => stack_only = true,
            "-s" | "--steps" => {
                steps = args
                    .next()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or_else(|| fail("steps requires a number"));
            }
            "-r" | "--runs" => {
                runs = args
                    .next()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or_else(|| fail("runs requires a number"));
            }
            other if other.starts_with('-') => fail(&format!("unknown option '{other}'")),
            other => {
                if file.is_some() {
                    fail(&format!("unexpected argument '{other}'"));
                }
                file = Some(other.to_string());
            }
        }
    }

    let Some(file) = file else {
        fail(&format!("'{command}' requires a file"));
    };

    let command = match command.as_str() {
        "disassemble" | "dis" => Command::Disassemble { file, raw },
        "trace" => Command::Trace {
            file,
            steps,
            stack_only,
        },
        "profile" => Command::Profile { file, runs },
        "stats" => Command::Stats { file },
        other => fail(&format!("unknown command '{other}'")),
    };

    Cli { command }
}

fn get_opcode_name(op: u8) -> &'static str {
    xcl_vm::opcode_name(op)
        .or_else(|| superinstruction_name(op))
        .or_else(|| xcl_plugins::registered_opcode_name(op))
        .unwrap_or("UNKNOWN")
}

fn superinstruction_name(op: u8) -> Option<&'static str> {
    Some(match op {
        32 => "OP_CALL",
        38 => "OP_GET_FIELD",
        42 => "OP_FOR_ITER_START",
        43 => "OP_FOR_ITER_NEXT",
        44 => "OP_FOR_RANGE_START",
        45 => "OP_FOR_RANGE_NEXT",
        52 => "OP_TYPE_TEST",
        53 => "OP_TYPE_CAST",
        60 => "OP_LOAD_LOCAL_LOAD_LOCAL",
        61 => "OP_ADD_LOCAL_LOCAL",
        62 => "OP_SUB_LOCAL_LOCAL",
        63 => "OP_LT_LOCAL_LOCAL",
        64 => "OP_LOAD_LOCAL_PUSH_INT",
        65 => "OP_LOAD_LOCAL_STORE_LOCAL",
        66 => "OP_CALL_METHOD",
        80 => "OP_INC_LOCAL",
        81 => "OP_DEC_LOCAL",
        82 => "OP_MUL_LOCAL_LOCAL",
        83 => "OP_DIV_LOCAL_LOCAL",
        84 => "OP_ADD_INT_STORE_LOCAL",
        85 => "OP_INC_LOCAL_INT",
        90 => "OP_FOR_NUMBER_LOOP",
        91 => "OP_ADD_INT_LOCAL",
        92 => "OP_ADD_LOCAL_STORE",
        93 => "OP_SUB_INT_LOCAL",
        94 => "OP_MUL_INT_LOCAL",
        95 => "OP_WHILE_LT_INT",
        96 => "OP_MUL_LOCAL_STORE",
        97 => "OP_DIV_LOCAL_STORE",
        98 => "OP_SUB_LOCAL_STORE",
        99 => "OP_ADD_LOCAL_LOCAL_STORE",
        100 => "OP_MUL_LOCAL_LOCAL_STORE",
        101 => "OP_SUB_LOCAL_LOCAL_STORE",
        102 => "OP_DIV_LOCAL_LOCAL_STORE",
        103 => "OP_WHILE_LT_LOCAL",
        105 => "OP_GET_INDEX_LOCAL_LOCAL_STORE",
        106 => "OP_GET_FIELD_LOCAL_STORE",
        107 => "OP_MOD_LOCAL_LOCAL_STORE",
        108 => "OP_LOAD_CONST_LOCAL",
        109 => "OP_ARRAY_PUSH_LOCAL",
        _ => return None,
    })
}

fn load_and_compile(path: &str) -> Bytecode {
    let source =
        fs::read_to_string(path).unwrap_or_else(|e| panic!("Failed to read {}: {}", path, e));
    let tokens = lex(&source, path);
    let ast = parse(&tokens, &source);
    compile(&ast)
}

fn cmd_disassemble(bc: &Bytecode, raw: bool) {
    println!("=== XCL Bytecode Disassembly ===");
    println!("Instructions: {}", bc.instructions.len());
    println!("Wide operands: {}", bc.wide_operands.len());
    println!("String table: {}", bc.string_table.len());
    println!("Local names: {}", bc.local_names.len());
    println!("Global names: {}", bc.global_names.len());
    println!("Closures: {}", bc.closures.len());
    println!();

    print_code(bc, raw);

    for (name, func) in bc.user_functions.iter() {
        let Some(body) = func.bytecode.as_ref() else {
            continue;
        };
        println!();
        println!(
            "--- function {} ({} instructions) ---",
            name,
            body.instructions.len()
        );
        println!();
        print_code(body, raw);
    }
}

/// Arithmetic code carried in the `c` operand of the fused field opcodes.
fn field_op_name(c: u8) -> &'static str {
    match c {
        0 => "add",
        1 => "sub",
        2 => "mul",
        _ => "?",
    }
}

fn field_name(bc: &Bytecode, idx: u32) -> &str {
    bc.string_table
        .get(idx as usize)
        .map_or("?", |s| s.as_ref())
}

fn print_code(bc: &Bytecode, raw: bool) {
    if !bc.local_names.is_empty() {
        println!("--- Local Variables ---");
        for (i, name) in bc.local_names.iter().enumerate() {
            println!("  [{}] {}", i, name);
        }
        println!();
    }

    if !bc.global_names.is_empty() {
        println!("--- Global Variables ---");
        for (i, name) in bc.global_names.iter().enumerate() {
            println!("  [{}] {}", i, name);
        }
        println!();
    }

    println!("--- Instructions ---");
    for (i, instr) in bc.instructions.iter().enumerate() {
        let op = instr.op();
        let a = instr.a();
        let b = instr.b();
        let c = instr.c();
        let name = get_opcode_name(op);

        let operands = match op {
            // No operands
            0 | 1 | 2 | 3 | 17 | 26 | 33 | 34 | 56 => String::new(),

            // Fused field read-modify-write: `a` names the destination field.
            55 => format!(" pop().{} = pop()", field_name(bc, a)),
            57 => format!(
                " {}.{} {} {} (b={})",
                "recv",
                field_name(bc, a),
                field_op_name(c),
                b,
                b
            ),
            58 => format!(
                " recv.{} {} ${} (b={})",
                field_name(bc, a),
                field_op_name(c),
                bc.local_names.get(b as usize).map_or("?", |s| s.as_ref()),
                b
            ),
            59 => format!(
                " recv.{} {} recv.{}",
                field_name(bc, a),
                field_op_name(c),
                field_name(bc, b as u32)
            ),

            // Self-receiver forms: the receiver is `$self`.
            71 => format!(" $self.{} {} {}", field_name(bc, a), field_op_name(c), b),
            72 => format!(
                " $self.{} {} ${}",
                field_name(bc, a),
                field_op_name(c),
                bc.local_names.get(b as usize).map_or("?", |s| s.as_ref())
            ),
            73 => format!(
                " $self.{} {} $self.{}",
                field_name(bc, a),
                field_op_name(c),
                field_name(bc, b as u32)
            ),
            74 => format!(" $self.{}", field_name(bc, a)),
            75 => format!(
                " ${} {} top",
                bc.local_names.get(a as usize).map_or("?", |s| s.as_ref()),
                field_op_name(c)
            ),
            76 => format!(" {}.<method> {}", field_name(bc, a), b as i16),
            77 => format!(
                " ${}[${}]",
                bc.local_names.get(a as usize).map_or("?", |s| s.as_ref()),
                bc.local_names.get(b as usize).map_or("?", |s| s.as_ref())
            ),
            78 => format!(" return $self.{}", field_name(bc, a)),
            79 => format!(" chain[{}]", a),
            86 => format!(" chain[{}] then return $self.{}", a, field_name(bc, b as u32)),
            87 => format!(
                " ${}[${}].{} {}",
                bc.local_names.get((b >> 8) as usize).map_or("?", |s| s.as_ref()),
                bc.local_names.get((b & 0xff) as usize).map_or("?", |s| s.as_ref()),
                field_name(bc, a),
                c as i8
            ),

            // Single operand a
            4 | 5 | 6 | 7 | 8 | 9 | 10 | 11 | 28 | 29 | 30 | 35 | 36 | 37 | 38 | 46 | 52 | 53
            | 80 | 81 => match op {
                7 => format!(
                    " a={} \"{}\"",
                    a,
                    bc.string_table.get(a as usize).map_or("?", |s| s.as_ref())
                ),
                8 => format!(
                    " a={} ${}",
                    a,
                    bc.local_names.get(a as usize).map_or("$?", |s| s.as_ref())
                ),
                9 => format!(
                    " a={} ${}",
                    a,
                    bc.local_names.get(a as usize).map_or("$?", |s| s.as_ref())
                ),
                10 => format!(
                    " a={} @{}",
                    a,
                    bc.global_names.get(a as usize).map_or("?", |s| s.as_ref())
                ),
                11 => format!(
                    " a={} @{}",
                    a,
                    bc.global_names.get(a as usize).map_or("?", |s| s.as_ref())
                ),
                5 | 6 => format!(
                    " a={} (wide[{}]={})",
                    a,
                    a,
                    bc.wide_operands.get(a as usize).unwrap_or(&i64::MIN)
                ),
                _ => format!(" a={}", a),
            },

            // Two operands a, b
            32 | 60 | 61 | 62 | 63 | 64 | 65 | 66 | 82 | 83 | 92 | 96 | 97 | 98 | 99 | 100
            | 101 | 102 | 103 | 105 | 106 | 107 | 109 => match op {
                60..=65 | 82 | 83 | 92 | 96..=103 | 105..=107 | 109 => {
                    format!(
                        " a=${} b=${}",
                        bc.local_names.get(a as usize).map_or("$?", |s| s.as_ref()),
                        bc.local_names.get(b as usize).map_or("$?", |s| s.as_ref())
                    )
                }
                66 => format!(
                    " a=\"{}\" b={}",
                    bc.string_table.get(a as usize).map_or("?", |s| s.as_ref()),
                    b
                ),
                _ => format!(" a={} b={}", a, b),
            },

            // Three operands a, b, c
            84 | 85 | 91 | 93 | 94 | 108 => match op {
                108 => {
                    let kind = if c == 1 { "float" } else { "int" };
                    format!(
                        " a=${} b=wide[{}] c={}",
                        bc.local_names.get(a as usize).map_or("$?", |s| s.as_ref()),
                        b,
                        kind
                    )
                }
                _ => format!(" a={} b={} c={}", a, b, c),
            },

            // For loop special
            90 => format!(
                " a=${} b=-{} c={}",
                bc.local_names.get(a as usize).map_or("$?", |s| s.as_ref()),
                b,
                c
            ),

            _ => format!(" a={} b={} c={}", a, b, c),
        };

        if raw {
            println!("  {:04}  {:016X}  {}{}", i, instr.0, name, operands);
        } else {
            println!("  {:04}  {}{}", i, name, operands);
        }
    }
}

fn cmd_trace(bc: &Bytecode, max_steps: usize, stack_only: bool) {
    println!("=== XCL Bytecode Trace ===");
    let steps_str = if max_steps == 0 {
        "unlimited"
    } else {
        &max_steps.to_string()
    };
    println!("Max steps: {}", steps_str);
    println!();

    // Simulate a simple trace by walking instructions
    let mut ip: usize = 0;
    let mut step = 0;
    let mut stack: Vec<String> = Vec::new();

    while ip < bc.instructions.len() {
        if max_steps > 0 && step >= max_steps {
            println!("  ... (truncated at {} steps)", max_steps);
            break;
        }

        let instr = bc.instructions[ip];
        let op = instr.op();
        let name = get_opcode_name(op);

        // Simulate stack changes
        let stack_before = stack.len();
        match op {
            1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 10 => stack.push(format!("[{}] ", name)),
            9 | 11 => {
                if !stack.is_empty() {
                    stack.pop();
                }
            }
            12..=27 => {
                if stack.len() >= 2 {
                    stack.pop();
                    stack.pop();
                    stack.push(format!("[{}] ", name));
                } else if stack.len() == 1 {
                    stack.pop();
                    stack.push(format!("[{}] ", name));
                }
            }
            32 => {
                let a = instr.a() as usize;
                if stack.len() >= a {
                    for _ in 0..a {
                        stack.pop();
                    }
                    stack.push("[call]".to_string());
                }
            }
            33 => {
                if !stack.is_empty() {
                    stack.pop();
                }
            }
            35 => {
                let a = instr.a() as usize;
                if stack.len() >= a {
                    for _ in 0..a {
                        stack.pop();
                    }
                    stack.push("[array]".to_string());
                }
            }
            36 => {
                let a = instr.a() as usize;
                if stack.len() >= a * 2 {
                    for _ in 0..a * 2 {
                        stack.pop();
                    }
                    stack.push("[dict]".to_string());
                }
            }
            37 | 38 => {
                if !stack.is_empty() {
                    stack.pop();
                    stack.push(format!("[{}] ", name));
                }
            }
            39 if !stack.is_empty() => {
                stack.pop();
            }
            39 => {}
            _ => {}
        }

        let stack_after = stack.len();
        let stack_change = if stack_after > stack_before {
            format!(" +{}", stack_after - stack_before)
        } else if stack_after < stack_before {
            format!(" {}", stack_after as i32 - stack_before as i32)
        } else {
            String::new()
        };

        if !stack_only || !stack_change.is_empty() {
            let stack_display = if stack_only && stack_change.is_empty() {
                String::new()
            } else {
                format!(" [stack:{}]", stack.len())
            };

            println!("  {:04}  {}{}{}", ip, name, stack_change, stack_display);
        }

        // Handle jumps
        match op {
            28 => {
                ip = instr.a() as usize;
            }
            29 | 30 => {
                // Conditional jump — simulate both paths
                ip += 1;
            }
            90 => {
                // For loop — jump back
                let jump_back = instr.b() as usize;
                if jump_back > 0 {
                    ip = if ip >= jump_back {
                        ip - jump_back
                    } else {
                        ip + 1
                    };
                } else {
                    ip += 1;
                }
            }
            _ => {
                ip += 1;
            }
        }

        step += 1;
    }

    println!();
    println!("Total steps traced: {}", step);
}

fn cmd_profile(bc: &Bytecode, runs: usize) {
    println!("=== XCL Bytecode Profile ===");
    println!("Runs: {}", runs);
    println!("Instructions: {}", bc.instructions.len());
    println!();

    // Count opcode frequency
    let mut opcode_counts: std::collections::HashMap<u8, usize> = std::collections::HashMap::new();
    for instr in &*bc.instructions {
        *opcode_counts.entry(instr.op()).or_insert(0) += 1;
    }

    // Categorize opcodes
    let mut categories: std::collections::HashMap<&'static str, usize> =
        std::collections::HashMap::new();
    for (&op, &count) in &opcode_counts {
        let category = match op {
            1..=7 => "Constants",
            8..=11 => "Variable Access",
            12..=17 => "Arithmetic",
            18..=27 => "Comparison/Logic",
            28..=30 => "Control Flow",
            32..=34 => "Calls",
            35..=41 => "Data Structures",
            42..=46 => "Iteration",
            60..=66 => "Fused Ops",
            70..=75 => "Fast Integer",
            80..=85 => "Superinstructions",
            90..=109 => "Optimized Ops",
            _ => "Other",
        };
        *categories.entry(category).or_insert(0) += count;
    }

    println!("--- Opcode Categories ---");
    let mut cat_vec: Vec<_> = categories.iter().collect();
    cat_vec.sort_by(|a, b| b.1.cmp(a.1));
    for (cat, count) in &cat_vec {
        let pct = (**count as f64 / bc.instructions.len() as f64) * 100.0;
        println!("  {:25} {:4} ({:5.1}%)", cat, count, pct);
    }

    println!();
    println!("--- Top Opcodes ---");
    let mut op_vec: Vec<_> = opcode_counts.iter().collect();
    op_vec.sort_by(|a, b| b.1.cmp(a.1));
    for (op, count) in &op_vec[..10.min(op_vec.len())] {
        let name = get_opcode_name(**op);
        let pct = (**count as f64 / bc.instructions.len() as f64) * 100.0;
        println!("  {:35} {:4} ({:5.1}%)", name, count, pct);
    }

    println!();
    println!("--- Performance Analysis ---");

    // Check for optimization opportunities
    let generic_arith = opcode_counts.get(&12).unwrap_or(&0)
        + opcode_counts.get(&13).unwrap_or(&0)
        + opcode_counts.get(&14).unwrap_or(&0)
        + opcode_counts.get(&15).unwrap_or(&0);
    let fast_int = opcode_counts.get(&70).unwrap_or(&0)
        + opcode_counts.get(&71).unwrap_or(&0)
        + opcode_counts.get(&72).unwrap_or(&0);
    let fused_ops = categories.get("Fused Ops").unwrap_or(&0)
        + categories.get("Superinstructions").unwrap_or(&0)
        + categories.get("Optimized Ops").unwrap_or(&0);

    let total_arith = generic_arith + fast_int;
    if total_arith > 0 {
        let fused_pct = (fused_ops as f64 / total_arith as f64 * 100.0).min(100.0);
        println!("  Arithmetic operations: {} total", total_arith);
        println!("    Generic (slow path): {}", generic_arith);
        println!("    Fast integer: {}", fast_int);
        println!("    Fused/optimized: {}", fused_ops);
        println!("    Fusion rate: {:.1}%", fused_pct);

        if generic_arith > fast_int {
            println!(
                "  ⚠️  Generic arithmetic > fast integer — peephole optimizer not fully utilized"
            );
        }
    }

    let _call_count = categories.get("Calls").unwrap_or(&0);
    let var_access = categories.get("Variable Access").unwrap_or(&0);
    if *var_access > 0 {
        let local_ops = opcode_counts.get(&60).unwrap_or(&0)
            + opcode_counts.get(&64).unwrap_or(&0)
            + opcode_counts.get(&65).unwrap_or(&0);
        println!("  Variable access: {} total", var_access);
        println!("    Fused local ops: {}", local_ops);
        if local_ops < var_access / 2 {
            println!("  ⚠️  Low local variable fusion — consider more fused ops");
        }
    }

    println!();
    println!("--- Estimated Hot Paths ---");
    // Find consecutive instruction patterns
    let instructions = &*bc.instructions;
    let mut patterns: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for i in 0..instructions.len().saturating_sub(2) {
        let key = format!(
            "{} -> {} -> {}",
            get_opcode_name(instructions[i].op()),
            get_opcode_name(instructions[i + 1].op()),
            get_opcode_name(instructions[i + 2].op())
        );
        *patterns.entry(key).or_insert(0) += 1;
    }
    let mut pat_vec: Vec<_> = patterns.iter().collect();
    pat_vec.sort_by(|a, b| b.1.cmp(a.1));
    for (pat, count) in &pat_vec[..5.min(pat_vec.len())] {
        println!("  ({}x) {}", count, pat);
    }
}

fn cmd_stats(bc: &Bytecode) {
    println!("=== XCL Bytecode Statistics ===");
    println!();

    println!("--- Code Size ---");
    let instr_bytes = bc.instructions.len() * 8; // 64-bit instructions
    let wide_bytes = bc.wide_operands.len() * 8;
    let string_bytes: usize = bc.string_table.iter().map(|s| s.len()).sum();
    let total = instr_bytes + wide_bytes + string_bytes;
    println!(
        "  Instructions:     {:4} ({} bytes)",
        bc.instructions.len(),
        instr_bytes
    );
    println!(
        "  Wide operands:    {:4} ({} bytes)",
        bc.wide_operands.len(),
        wide_bytes
    );
    println!(
        "  String table:     {:4} ({} bytes)",
        bc.string_table.len(),
        string_bytes
    );
    println!("  Local names:      {:4}", bc.local_names.len());
    println!("  Global names:     {:4}", bc.global_names.len());
    println!("  Closures:         {:4}", bc.closures.len());
    println!("  Total bytecode:   {} bytes", total);

    println!();
    println!("--- Instruction Mix ---");

    let mut opcode_counts: std::collections::HashMap<u8, usize> = std::collections::HashMap::new();
    for instr in &*bc.instructions {
        *opcode_counts.entry(instr.op()).or_insert(0) += 1;
    }

    let mut op_vec: Vec<_> = opcode_counts.iter().collect();
    op_vec.sort_by(|a, b| b.1.cmp(a.1));

    let max_count = op_vec.first().map(|(_, c)| *c).unwrap_or(&1);
    for (op, count) in &op_vec {
        let name = get_opcode_name(**op);
        let bar_len = (**count as f64 / *max_count as f64 * 40.0) as usize;
        let bar = "#".repeat(bar_len);
        let pct = (**count as f64 / bc.instructions.len() as f64) * 100.0;
        println!("  {:35} {:4} |{}| {:5.1}%", name, count, bar, pct);
    }

    println!();
    println!("--- Comparison with Luau ---");
    println!("  Luau uses inline caching for field access — XCL does not");
    println!("  Luau uses computed goto dispatch — XCL uses Rust match");
    println!("  XCL has fused superinstructions — Luau does not");
    println!("  XCL uses NaN-boxed values (8 bytes) — Luau uses 20+ bytes");
}

fn main() {
    let cli = parse_args(std::env::args());

    match cli.command {
        Command::Help => print!("{USAGE}"),
        Command::Version => println!("xcl-bytecode {}", env!("CARGO_PKG_VERSION")),
        Command::Disassemble { file, raw } => {
            let bc = load_and_compile(&file);
            cmd_disassemble(&bc, raw);
        }
        Command::Trace {
            file,
            steps,
            stack_only,
        } => {
            let bc = load_and_compile(&file);
            cmd_trace(&bc, steps, stack_only);
        }
        Command::Profile { file, runs } => {
            let bc = load_and_compile(&file);
            cmd_profile(&bc, runs);
        }
        Command::Stats { file } => {
            let bc = load_and_compile(&file);
            cmd_stats(&bc);
        }
    }
}
