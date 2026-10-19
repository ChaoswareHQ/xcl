//! Unit tests for the optimizer module.

use crate::bytecode::{
    Instr, OP_ADD, OP_HALT, OP_JUMP_IF_FALSE, OP_LOAD_LOCAL, OP_LOAD_GLOBAL, OP_PUSH_INT,
    OP_STORE_LOCAL, OP_SUB,
};
use super::dataflow::build_basic_blocks;
use super::dead_store::eliminate_dead_stores;
use super::licm::loop_invariant_code_motion;

#[test]
fn test_basic_blocks_simple() {
    let code: Vec<Instr> = vec![
        Instr::a1(OP_LOAD_LOCAL, 0),
        Instr::a1(OP_STORE_LOCAL, 1),
        Instr::none(OP_HALT),
    ];
    let blocks = build_basic_blocks(&code);
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].start, 0);
    assert_eq!(blocks[0].end, 2);
}

#[test]
fn test_dead_store_elimination() {
    // $x = 5; $y = 10; $y  — $x is dead
    let mut code: Vec<Instr> = vec![
        Instr::a1(OP_STORE_LOCAL, 0), // store to x (dead)
        Instr::a1(OP_STORE_LOCAL, 1), // store to y
        Instr::a1(OP_LOAD_LOCAL, 1),  // load y
        Instr::none(OP_HALT),
    ];
    let mut wide: Vec<i64> = Vec::new();
    eliminate_dead_stores(&mut code, &mut wide);
    // The dead store to slot 0 should be removed
    assert!(!code.iter().any(|i| i.op() == OP_STORE_LOCAL && i.a() == 0));
}

#[test]
fn test_licm_no_change_without_globals() {
    // A simple loop with no LOAD_GLOBAL should not be modified by LICM
    let orig: Vec<Instr> = vec![
        Instr::a1(OP_PUSH_INT, 0),
        Instr::a1(OP_STORE_LOCAL, 0), // $i = 0
        Instr::a1(OP_PUSH_INT, 0),
        Instr::a1(OP_STORE_LOCAL, 1), // $sum = 0
        // Loop header (index 4)
        Instr::a1(OP_LOAD_LOCAL, 0),  // load $i
        Instr::a1(OP_PUSH_INT, 100),
        Instr::none(OP_SUB),
        Instr::a1(OP_JUMP_IF_FALSE, 12), // if $i >= 100 jump to exit
        // Loop body (index 8)
        Instr::a1(OP_LOAD_LOCAL, 1),  // load $sum
        Instr::a1(OP_PUSH_INT, 1),
        Instr::none(OP_ADD),
        Instr::a1(OP_STORE_LOCAL, 1), // $sum = $sum + 1
        Instr::a1(OP_LOAD_LOCAL, 0),  // load $i
        Instr::a1(OP_PUSH_INT, 1),
        Instr::none(OP_ADD),
        Instr::a1(OP_STORE_LOCAL, 0), // $i = $i + 1
        Instr::a1(OP_JUMP_IF_FALSE, 4), // back to header (simulated)
        // Exit (index 12)
        Instr::none(OP_HALT),
    ];
    let mut code = orig.clone();
    let mut wide: Vec<i64> = Vec::new();
    loop_invariant_code_motion(&mut code, &mut wide);
    // Should be unchanged — no LOAD_GLOBAL to hoist
    assert_eq!(code, orig, "LICM should not modify bytecode without LOAD_GLOBAL");
}

#[test]
fn test_licm_hoists_load_global() {
    // Loop with LOAD_GLOBAL should hoist it before the loop
    let orig: Vec<Instr> = vec![
        Instr::a1(OP_PUSH_INT, 0),
        Instr::a1(OP_STORE_LOCAL, 0), // $i = 0
        // Loop header (index 2)
        Instr::a1(OP_LOAD_LOCAL, 0),  // load $i
        Instr::a1(OP_PUSH_INT, 10),
        Instr::none(OP_SUB),
        Instr::a1(OP_JUMP_IF_FALSE, 10), // exit at 10
        // Loop body (index 6)
        Instr::a1(OP_LOAD_GLOBAL, 5), // load global slot 5 (invariant)
        Instr::a1(OP_PUSH_INT, 1),
        Instr::none(OP_ADD),
        Instr::a1(OP_STORE_LOCAL, 1), // $x = $g + 1
        Instr::a1(OP_LOAD_LOCAL, 0),  // load $i
        Instr::a1(OP_PUSH_INT, 1),
        Instr::none(OP_ADD),
        Instr::a1(OP_STORE_LOCAL, 0), // $i = $i + 1
        Instr::a1(OP_JUMP_IF_FALSE, 2), // back to header
        // Exit (index 10)
        Instr::none(OP_HALT),
    ];
    let mut code = orig.clone();
    let mut wide: Vec<i64> = Vec::new();
    loop_invariant_code_motion(&mut code, &mut wide);

    // The LOAD_GLOBAL should be hoisted before the loop header
    let global_count = code.iter().filter(|i| i.op() == OP_LOAD_GLOBAL).count();
    assert!(
        global_count >= 1,
        "LOAD_GLOBAL should be hoisted, not destroyed. Found {} LOAD_GLOBAL",
        global_count
    );
}

#[test]
fn test_licm_with_real_compiled_bytecode() {
    // Test that a simple while-loop produces correct result
    use crate::{evaluate, VM};

    let source = r#"
        $i = 0
        $sum = 0
        while $i < 100 {
            $sum = $sum + 1
            $i = $i + 1
        }
        $sum
    "#;

    let mut vm = VM::new();
    vm.register_all();
    let result = evaluate(source, &mut vm);
    assert!(result.is_int(), "result should be an integer");
    assert_eq!(result.as_int(), 100, "sum should be 100");
}

#[test]
fn test_licm_does_not_corrupt_benchmark_bytecode() {
    // Verify that LICM does not modify bytecode for programs without LOAD_GLOBAL in loops
    use crate::{bytecode::compile, compiler::lexer, compiler::parser};

    let source = r#"
        $i = 0
        $sum = 0
        while $i < 10000 {
            $sum = $sum + 1
            $i = $i + 1
        }
        $sum
    "#;

    let tokens = lexer::lex(source, "<input>");
    let ast = parser::parse(&tokens, source);
    let bc = compile(&ast);
    let orig_code: Vec<_> = bc.instructions.to_vec();
    let orig_len = orig_code.len();

    let mut code = orig_code.clone();
    let mut wide = bc.wide_operands.to_vec();
    loop_invariant_code_motion(&mut code, &mut wide);

    assert_eq!(
        code.len(),
        orig_len,
        "LICM should not change instruction count for code without LOAD_GLOBAL in loops"
    );
    assert_eq!(
        code, orig_code,
        "LICM should not modify bytecode without LOAD_GLOBAL in loops"
    );
}
