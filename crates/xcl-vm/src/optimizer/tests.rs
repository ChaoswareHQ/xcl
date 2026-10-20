//! Unit tests for the optimizer module.

use super::dataflow::build_basic_blocks;
use super::dead_store::eliminate_dead_stores;
use super::licm::loop_invariant_code_motion;
use crate::bytecode::Instr;
use crate::runtime::vm::ops::consts::{
    OP_ADD, OP_ADD_FIELD_LOCAL_STORE, OP_ADD_FLOAT_LOCAL, OP_ADD_INT_LOCAL, OP_ADD_INT_STORE_LOCAL,
    OP_ADD_LOCAL_LOCAL, OP_ADD_LOCAL_LOCAL_STORE, OP_ADD_LOCAL_STORE, OP_ARRAY_PUSH_LOCAL,
    OP_DEC_LOCAL, OP_DIV_FLOAT_LOCAL, OP_DIV_LOCAL_LOCAL, OP_DIV_LOCAL_LOCAL_STORE,
    OP_DIV_LOCAL_STORE, OP_FOR_NUMBER_LOOP, OP_GET_FIELD_LOCAL_STORE,
    OP_GET_INDEX_LOCAL_LOCAL_STORE, OP_HALT, OP_IF_EQ_INT, OP_IF_LT_INT, OP_INC_LOCAL,
    OP_INC_LOCAL_INT, OP_JUMP, OP_JUMP_IF_FALSE, OP_LOAD_CONST_LOCAL, OP_LOAD_GLOBAL,
    OP_LOAD_LOCAL, OP_LOAD_LOCAL_LOAD_LOCAL, OP_LOAD_LOCAL_PUSH_INT, OP_LOAD_LOCAL_STORE_LOCAL,
    OP_LT_LOCAL_LOCAL, OP_MOD_EQ_INT, OP_MOD_LOCAL_LOCAL_STORE, OP_MUL_FLOAT_LOCAL,
    OP_MUL_INT_LOCAL, OP_MUL_LOCAL_LOCAL, OP_MUL_LOCAL_LOCAL_STORE, OP_MUL_LOCAL_STORE,
    OP_PUSH_INT, OP_STORE_LOCAL, OP_SUB, OP_SUB_FLOAT_LOCAL, OP_SUB_INT_LOCAL, OP_SUB_LOCAL_LOCAL,
    OP_SUB_LOCAL_LOCAL_STORE, OP_SUB_LOCAL_STORE, OP_WHILE_LT_INT, OP_WHILE_LT_INT_WIDE,
    OP_WHILE_LT_LOCAL,
};
use crate::runtime::vm::ops::meta;

#[test]
fn test_basic_blocks_simple() {
    let code: Vec<Instr> = vec![
        Instr::a1(OP_LOAD_LOCAL, 0),
        Instr::a1(OP_STORE_LOCAL, 1),
        Instr::none(OP_HALT),
    ];
    let blocks = build_basic_blocks(&code, &[]);
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].start, 0);
    assert_eq!(blocks[0].end, 2);
}

#[test]
fn test_basic_blocks_while_exit() {
    // Simulate: while $i < 3 { $x = $a[$i]; $i++ } ; LOAD_LOCAL $x
    // IP 0: WHILE_LT_INT a=1, b=3, c=0 (exit_wide_idx=0, wide[0]=4)
    // IP 1: GET_INDEX_LOCAL_LOCAL_STORE a=2, b=1, c=0
    // IP 2: INC_LOCAL a=1
    // IP 3: JUMP a=0
    // IP 4: LOAD_LOCAL a=2
    // IP 5: HALT
    let wide = [4i64];
    let code: Vec<Instr> = vec![
        Instr::make(OP_WHILE_LT_INT, 1, 3, 0),
        Instr::make(OP_GET_INDEX_LOCAL_LOCAL_STORE, 2, 1, 0),
        Instr::a1(OP_INC_LOCAL, 1),
        Instr::a1(OP_JUMP, 0),
        Instr::a1(OP_LOAD_LOCAL, 2),
        Instr::none(OP_HALT),
    ];
    let blocks = build_basic_blocks(&code, &wide);
    // Block 0: IP 0 (WHILE_LT_INT) - successors should include IP 1 (body) and IP 4 (exit)
    let block0 = &blocks.iter().find(|b| b.start == 0).unwrap();
    assert!(
        block0.successors.contains(&1),
        "WHILE should have body successor"
    );
    assert!(
        block0.successors.contains(&4),
        "WHILE should have exit successor"
    );
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
    eliminate_dead_stores(&mut code, &mut wide, 0);
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
        Instr::a1(OP_LOAD_LOCAL, 0), // load $i
        Instr::a1(OP_PUSH_INT, 100),
        Instr::none(OP_SUB),
        Instr::a1(OP_JUMP_IF_FALSE, 12), // if $i >= 100 jump to exit
        // Loop body (index 8)
        Instr::a1(OP_LOAD_LOCAL, 1), // load $sum
        Instr::a1(OP_PUSH_INT, 1),
        Instr::none(OP_ADD),
        Instr::a1(OP_STORE_LOCAL, 1), // $sum = $sum + 1
        Instr::a1(OP_LOAD_LOCAL, 0),  // load $i
        Instr::a1(OP_PUSH_INT, 1),
        Instr::none(OP_ADD),
        Instr::a1(OP_STORE_LOCAL, 0),   // $i = $i + 1
        Instr::a1(OP_JUMP_IF_FALSE, 4), // back to header (simulated)
        // Exit (index 12)
        Instr::none(OP_HALT),
    ];
    let mut code = orig.clone();
    let mut wide: Vec<i64> = Vec::new();
    loop_invariant_code_motion(&mut code, &mut wide);
    // Should be unchanged — no LOAD_GLOBAL to hoist
    assert_eq!(
        code, orig,
        "LICM should not modify bytecode without LOAD_GLOBAL"
    );
}

#[test]
fn test_licm_hoists_load_global() {
    // Loop with LOAD_GLOBAL should hoist it before the loop
    let orig: Vec<Instr> = vec![
        Instr::a1(OP_PUSH_INT, 0),
        Instr::a1(OP_STORE_LOCAL, 0), // $i = 0
        // Loop header (index 2)
        Instr::a1(OP_LOAD_LOCAL, 0), // load $i
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
        Instr::a1(OP_STORE_LOCAL, 0),   // $i = $i + 1
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
    use crate::{VM, evaluate};

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
    let result = evaluate(source, &mut vm);
    assert!(result.is_int(), "result should be an integer");
    assert_eq!(result.as_int(), 100, "sum should be 100");
}

#[test]
fn test_shift_operators() {
    // Test shift left, shift right, and bitwise AND
    use crate::{VM, evaluate};

    let mut vm = VM::new();

    // Left shift: 5 << 3 = 40
    let result = evaluate("5 << 3", &mut vm);
    assert_eq!(result.as_int(), 40, "5 << 3 should be 40");

    // Right shift: 40 >> 2 = 10
    let result = evaluate("40 >> 2", &mut vm);
    assert_eq!(result.as_int(), 10, "40 >> 2 should be 10");

    // Bitwise AND: 5 & 3 = 1
    let result = evaluate("5 & 3", &mut vm);
    assert_eq!(result.as_int(), 1, "5 & 3 should be 1");
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

#[test]
fn test_branch_kind_consistency() {
    // Verify BRANCH_KIND table agrees with the handler set:
    // - Every opcode with kind 2 (wide) must be one of the known wide-branch opcodes
    // - branch_kind is 0 for non-branch opcodes like OP_ADD
    // - branch_kind is 1 for absolute-branch opcodes like OP_JUMP

    // Non-branch opcodes must be 0
    assert_eq!(meta::branch_kind(OP_ADD), 0, "OP_ADD must not be a branch");
    assert_eq!(
        meta::branch_kind(OP_HALT),
        0,
        "OP_HALT must not be a branch"
    );
    assert_eq!(
        meta::branch_kind(OP_LOAD_LOCAL),
        0,
        "OP_LOAD_LOCAL must not be a branch"
    );
    assert_eq!(
        meta::branch_kind(OP_STORE_LOCAL),
        0,
        "OP_STORE_LOCAL must not be a branch"
    );
    assert_eq!(
        meta::branch_kind(OP_PUSH_INT),
        0,
        "OP_PUSH_INT must not be a branch"
    );

    // Absolute branch opcodes must be 1
    assert_eq!(
        meta::branch_kind(OP_JUMP),
        1,
        "OP_JUMP must be an absolute branch"
    );
    assert_eq!(
        meta::branch_kind(OP_JUMP_IF_FALSE),
        1,
        "OP_JUMP_IF_FALSE must be an absolute branch"
    );

    // Wide branch opcodes must be 2
    assert_eq!(
        meta::branch_kind(OP_WHILE_LT_INT),
        2,
        "OP_WHILE_LT_INT must be a wide branch"
    );

    // BRANCH_KIND array must be 256 entries
    assert_eq!(meta::BRANCH_KIND.len(), 256);

    // branch_kind helper must match the array
    for i in 0..=255u8 {
        assert_eq!(
            meta::branch_kind(i),
            meta::BRANCH_KIND[i as usize],
            "branch_kind({}) mismatch",
            i
        );
    }
}

/// Expand a slot mask into slot indices using a test instruction (a=1, b=2, c=3).
fn mask_to_slots(mask: u8, instr: Instr) -> Vec<u32> {
    let mut slots = Vec::new();
    if mask & 1 != 0 {
        slots.push(instr.a());
    }
    if mask & 2 != 0 {
        slots.push(instr.b() as u32);
    }
    if mask & 4 != 0 {
        slots.push(instr.c() as u32);
    }
    slots
}

/// Legacy dataflow use-set for an opcode (from the hardcoded match arms).
fn legacy_dataflow_use(op: u8, instr: Instr) -> Vec<u32> {
    match op {
        OP_LOAD_LOCAL
        | OP_LOAD_LOCAL_PUSH_INT
        | OP_MOD_EQ_INT
        | OP_WHILE_LT_INT
        | OP_WHILE_LT_INT_WIDE
        | OP_IF_LT_INT
        | OP_IF_EQ_INT => vec![instr.a()],
        OP_ADD_LOCAL_LOCAL
        | OP_SUB_LOCAL_LOCAL
        | OP_MUL_LOCAL_LOCAL
        | OP_DIV_LOCAL_LOCAL
        | OP_LT_LOCAL_LOCAL
        | OP_LOAD_LOCAL_LOAD_LOCAL
        | OP_WHILE_LT_LOCAL => vec![instr.a(), instr.b() as u32],
        OP_ADD_LOCAL_STORE | OP_SUB_LOCAL_STORE | OP_MUL_LOCAL_STORE | OP_DIV_LOCAL_STORE => {
            vec![instr.a(), instr.b() as u32]
        }
        OP_ADD_LOCAL_LOCAL_STORE
        | OP_SUB_LOCAL_LOCAL_STORE
        | OP_MUL_LOCAL_LOCAL_STORE
        | OP_DIV_LOCAL_LOCAL_STORE
        | OP_MOD_LOCAL_LOCAL_STORE
        | OP_GET_INDEX_LOCAL_LOCAL_STORE => vec![instr.b() as u32, instr.c() as u32],
        OP_GET_FIELD_LOCAL_STORE => vec![instr.c() as u32],
        OP_ADD_FIELD_LOCAL_STORE => vec![instr.a(), instr.c() as u32],
        OP_ARRAY_PUSH_LOCAL => vec![instr.a(), instr.b() as u32],
        OP_ADD_INT_LOCAL | OP_SUB_INT_LOCAL | OP_MUL_INT_LOCAL | OP_ADD_FLOAT_LOCAL
        | OP_SUB_FLOAT_LOCAL | OP_MUL_FLOAT_LOCAL | OP_DIV_FLOAT_LOCAL | OP_INC_LOCAL
        | OP_DEC_LOCAL | OP_INC_LOCAL_INT => vec![instr.a()],
        OP_ADD_INT_STORE_LOCAL => vec![instr.b() as u32],
        // LOAD_LOCAL_STORE_LOCAL: reads slot a (src), writes slot b (dest)
        OP_LOAD_LOCAL_STORE_LOCAL => vec![instr.a()],
        OP_FOR_NUMBER_LOOP => vec![instr.a() >> 16],
        _ => Vec::new(),
    }
}

/// Legacy dataflow def-set for an opcode (from the hardcoded match arms).
fn legacy_dataflow_def(op: u8, instr: Instr) -> Vec<u32> {
    match op {
        OP_STORE_LOCAL | OP_LOAD_CONST_LOCAL | OP_ADD_INT_LOCAL | OP_SUB_INT_LOCAL
        | OP_MUL_INT_LOCAL | OP_ADD_FLOAT_LOCAL | OP_SUB_FLOAT_LOCAL | OP_MUL_FLOAT_LOCAL
        | OP_DIV_FLOAT_LOCAL | OP_INC_LOCAL | OP_DEC_LOCAL | OP_INC_LOCAL_INT => vec![instr.a()],
        // LOAD_LOCAL_STORE_LOCAL: writes slot b (dest)
        OP_LOAD_LOCAL_STORE_LOCAL => vec![instr.b() as u32],
        OP_ADD_LOCAL_STORE | OP_SUB_LOCAL_STORE | OP_MUL_LOCAL_STORE | OP_DIV_LOCAL_STORE => {
            vec![instr.a()]
        }
        OP_ADD_LOCAL_LOCAL_STORE
        | OP_SUB_LOCAL_LOCAL_STORE
        | OP_MUL_LOCAL_LOCAL_STORE
        | OP_DIV_LOCAL_LOCAL_STORE
        | OP_GET_INDEX_LOCAL_LOCAL_STORE
        | OP_GET_FIELD_LOCAL_STORE
        | OP_ADD_FIELD_LOCAL_STORE
        | OP_MOD_LOCAL_LOCAL_STORE => vec![instr.a()],
        OP_ARRAY_PUSH_LOCAL => vec![instr.a()],
        OP_ADD_INT_STORE_LOCAL => vec![instr.a()],
        OP_FOR_NUMBER_LOOP => vec![instr.a() >> 16],
        _ => Vec::new(),
    }
}

#[test]
fn slot_metadata_matches_legacy_dataflow() {
    // For every opcode 0..=255, assert that meta::reads/writes produce the same
    // slot sets as the legacy hardcoded lists in dataflow.rs.
    let instr = Instr::make(0, 1, 2, 3); // a=1, b=2, c=3

    for op in 0..=255u8 {
        // Skip OP_FOR_NUMBER_LOOP — it's a packed-operand opt-out
        if op == OP_FOR_NUMBER_LOOP {
            continue;
        }

        let meta_reads = mask_to_slots(meta::reads(op), instr);
        let meta_writes = mask_to_slots(meta::writes(op), instr);
        let legacy_use = legacy_dataflow_use(op, instr);
        let legacy_def = legacy_dataflow_def(op, instr);

        assert_eq!(
            meta_reads, legacy_use,
            "OP {} (0x{:02x}): reads mismatch: meta={:?}, legacy_use={:?}",
            op, op, meta_reads, legacy_use
        );
        assert_eq!(
            meta_writes, legacy_def,
            "OP {} (0x{:02x}): writes mismatch: meta={:?}, legacy_def={:?}",
            op, op, meta_writes, legacy_def
        );
    }

    // OP_FOR_NUMBER_LOOP must be excluded and asserted separately
    assert_eq!(
        meta::reads(OP_FOR_NUMBER_LOOP),
        0,
        "FOR_NUMBER_LOOP must opt out of reads"
    );
    assert_eq!(
        meta::writes(OP_FOR_NUMBER_LOOP),
        0,
        "FOR_NUMBER_LOOP must opt out of writes"
    );
}

/// Legacy constant_prop invalidation set (slots whose constants are invalidated).
fn legacy_constant_prop_invalidate(op: u8, instr: Instr) -> Vec<u32> {
    match op {
        OP_STORE_LOCAL => vec![instr.a()],
        OP_LOAD_CONST_LOCAL => vec![instr.a()],
        OP_INC_LOCAL | OP_DEC_LOCAL => vec![instr.a()],
        OP_ADD_LOCAL_STORE | OP_SUB_LOCAL_STORE | OP_MUL_LOCAL_STORE | OP_DIV_LOCAL_STORE => {
            vec![instr.a()]
        }
        OP_ADD_LOCAL_LOCAL_STORE
        | OP_SUB_LOCAL_LOCAL_STORE
        | OP_MUL_LOCAL_LOCAL_STORE
        | OP_DIV_LOCAL_LOCAL_STORE
        | OP_MOD_LOCAL_LOCAL_STORE
        | OP_GET_INDEX_LOCAL_LOCAL_STORE
        | OP_GET_FIELD_LOCAL_STORE
        | OP_ADD_FIELD_LOCAL_STORE
        | OP_ADD_INT_STORE_LOCAL
        | OP_INC_LOCAL_INT => {
            vec![instr.a()]
        }
        OP_ARRAY_PUSH_LOCAL => vec![instr.a()],
        OP_FOR_NUMBER_LOOP => vec![instr.a() >> 16],
        OP_ADD_INT_LOCAL | OP_SUB_INT_LOCAL | OP_MUL_INT_LOCAL => vec![instr.a()],
        OP_ADD_FLOAT_LOCAL | OP_SUB_FLOAT_LOCAL | OP_MUL_FLOAT_LOCAL | OP_DIV_FLOAT_LOCAL => {
            vec![instr.a()]
        }
        // LOAD_LOCAL_STORE_LOCAL: writes slot b (dest)
        OP_LOAD_LOCAL_STORE_LOCAL => vec![instr.b() as u32],
        _ => Vec::new(),
    }
}

#[test]
fn slot_metadata_matches_legacy_constant_prop() {
    // Assert that meta::writes covers all slots invalidated by constant_prop.
    let instr = Instr::make(0, 1, 2, 3);

    for op in 0..=255u8 {
        if op == OP_FOR_NUMBER_LOOP {
            continue;
        }

        let meta_writes = mask_to_slots(meta::writes(op), instr);
        let legacy_invalidate = legacy_constant_prop_invalidate(op, instr);

        // Every slot invalidated by constant_prop must appear in meta::writes
        for slot in &legacy_invalidate {
            assert!(
                meta_writes.contains(slot),
                "OP {} (0x{:02x}): constant_prop invalidates slot {} but meta::writes={:?}",
                op,
                op,
                slot,
                meta_writes
            );
        }
    }

    // OP_FOR_NUMBER_LOOP special case
    assert_eq!(meta::writes(OP_FOR_NUMBER_LOOP), 0);
}

/// Legacy CSE defined_slots (from cse.rs::defined_slots).
fn legacy_cse_defined_slots(op: u8, instr: Instr) -> Vec<u32> {
    match op {
        OP_STORE_LOCAL | OP_LOAD_CONST_LOCAL | OP_ADD_LOCAL_STORE | OP_SUB_LOCAL_STORE
        | OP_MUL_LOCAL_STORE | OP_DIV_LOCAL_STORE | OP_ADD_INT_LOCAL | OP_SUB_INT_LOCAL
        | OP_MUL_INT_LOCAL | OP_ADD_FLOAT_LOCAL | OP_SUB_FLOAT_LOCAL | OP_MUL_FLOAT_LOCAL
        | OP_DIV_FLOAT_LOCAL | OP_INC_LOCAL | OP_DEC_LOCAL | OP_INC_LOCAL_INT => vec![instr.a()],
        // LOAD_LOCAL_STORE_LOCAL: writes slot b (dest)
        OP_LOAD_LOCAL_STORE_LOCAL => vec![instr.b() as u32],
        OP_ADD_LOCAL_LOCAL_STORE
        | OP_SUB_LOCAL_LOCAL_STORE
        | OP_MUL_LOCAL_LOCAL_STORE
        | OP_DIV_LOCAL_LOCAL_STORE
        | OP_GET_INDEX_LOCAL_LOCAL_STORE
        | OP_GET_FIELD_LOCAL_STORE
        | OP_ADD_FIELD_LOCAL_STORE
        | OP_MOD_LOCAL_LOCAL_STORE => vec![instr.a()],
        OP_ARRAY_PUSH_LOCAL => vec![instr.a()],
        // ADD_INT_STORE_LOCAL: writes slot a (dest)
        OP_ADD_INT_STORE_LOCAL => vec![instr.a()],
        OP_FOR_NUMBER_LOOP => vec![instr.a() >> 16],
        _ => Vec::new(),
    }
}

#[test]
fn slot_metadata_matches_legacy_cse() {
    let instr = Instr::make(0, 1, 2, 3);

    for op in 0..=255u8 {
        if op == OP_FOR_NUMBER_LOOP {
            continue;
        }

        let meta_writes = mask_to_slots(meta::writes(op), instr);
        let legacy_defs = legacy_cse_defined_slots(op, instr);

        assert_eq!(
            meta_writes, legacy_defs,
            "OP {} (0x{:02x}): CSE defined_slots mismatch: meta_writes={:?}, legacy={:?}",
            op, op, meta_writes, legacy_defs
        );
    }

    // OP_FOR_NUMBER_LOOP special case
    assert_eq!(meta::writes(OP_FOR_NUMBER_LOOP), 0);
}

/// Legacy dead_store use-check (from dead_store.rs — which opcodes read a given slot).
fn legacy_dead_store_reads(op: u8, instr: Instr) -> Vec<u32> {
    match op {
        OP_LOAD_LOCAL
        | OP_LOAD_LOCAL_PUSH_INT
        | OP_WHILE_LT_INT
        | OP_WHILE_LT_INT_WIDE
        | OP_IF_LT_INT
        | OP_IF_EQ_INT => vec![instr.a()],
        OP_ADD_LOCAL_STORE | OP_SUB_LOCAL_STORE | OP_MUL_LOCAL_STORE | OP_DIV_LOCAL_STORE => {
            vec![instr.a(), instr.b() as u32]
        }
        OP_ADD_LOCAL_LOCAL_STORE
        | OP_SUB_LOCAL_LOCAL_STORE
        | OP_MUL_LOCAL_LOCAL_STORE
        | OP_DIV_LOCAL_LOCAL_STORE
        | OP_MOD_LOCAL_LOCAL_STORE => {
            vec![instr.b() as u32, instr.c() as u32]
        }
        OP_ADD_INT_LOCAL | OP_SUB_INT_LOCAL | OP_MUL_INT_LOCAL | OP_ADD_FLOAT_LOCAL
        | OP_SUB_FLOAT_LOCAL | OP_MUL_FLOAT_LOCAL | OP_DIV_FLOAT_LOCAL => {
            vec![instr.a()]
        }
        OP_INC_LOCAL | OP_DEC_LOCAL | OP_INC_LOCAL_INT => vec![instr.a()],
        // LOAD_LOCAL_STORE_LOCAL: reads slot a (src)
        OP_LOAD_LOCAL_STORE_LOCAL => vec![instr.a()],
        OP_ADD_LOCAL_LOCAL | OP_SUB_LOCAL_LOCAL | OP_MUL_LOCAL_LOCAL | OP_DIV_LOCAL_LOCAL
        | OP_LT_LOCAL_LOCAL => vec![instr.a(), instr.b() as u32],
        OP_GET_INDEX_LOCAL_LOCAL_STORE => vec![instr.b() as u32, instr.c() as u32],
        OP_GET_FIELD_LOCAL_STORE => vec![instr.c() as u32],
        OP_ADD_FIELD_LOCAL_STORE => vec![instr.a(), instr.c() as u32],
        OP_ARRAY_PUSH_LOCAL => vec![instr.a(), instr.b() as u32],
        OP_WHILE_LT_LOCAL => vec![instr.a(), instr.b() as u32],
        OP_FOR_NUMBER_LOOP => vec![instr.a() >> 16],
        OP_LOAD_LOCAL_LOAD_LOCAL => vec![instr.a(), instr.b() as u32],
        // ADD_INT_STORE_LOCAL: reads slot b (src)
        OP_ADD_INT_STORE_LOCAL => vec![instr.b() as u32],
        // MOD_EQ_INT: reads slot a
        OP_MOD_EQ_INT => vec![instr.a()],
        _ => Vec::new(),
    }
}

#[test]
fn slot_metadata_matches_legacy_dead_store() {
    let instr = Instr::make(0, 1, 2, 3);

    for op in 0..=255u8 {
        if op == OP_FOR_NUMBER_LOOP {
            continue;
        }

        let meta_reads = mask_to_slots(meta::reads(op), instr);
        let legacy_reads = legacy_dead_store_reads(op, instr);

        assert_eq!(
            meta_reads, legacy_reads,
            "OP {} (0x{:02x}): dead_store reads mismatch: meta={:?}, legacy={:?}",
            op, op, meta_reads, legacy_reads
        );
    }

    // OP_FOR_NUMBER_LOOP special case
    assert_eq!(meta::reads(OP_FOR_NUMBER_LOOP), 0);
}
